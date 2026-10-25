// SPDX-License-Identifier: AGPL-3.0-or-later
//! The commands the web UI calls, and the events it listens to (ADR-0014).
//!
//! Every command and every type it takes or returns is described to `tauri-specta`, which
//! generates the TypeScript bindings in `apps/desktop/src/bindings.ts`. The test
//! `bindings_are_current` fails when that file no longer matches the Rust code; run it with
//! `DBME_WRITE_BINDINGS=1` to regenerate the file.
//!
//! Commands that touch the vault are `async`, so they run on Tauri's thread pool rather than the
//! UI thread.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use dbme_core::model::{ObjectRow, TypeRow};
use dbme_core::{CoreError, Id, SystemClock};
use dbme_store::{Committed, Identity, NewObject, StoreError, Vault, VaultOptions};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};
use tauri_specta::Event;

use crate::Startup;

/// The vault open in this window, if any.
#[derive(Default)]
pub(crate) struct Session {
    vault: Mutex<Option<Vault>>,
}

/// What went wrong, in a form the UI can act on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum IpcErrorKind {
    /// No vault is open.
    NoVault,
    /// The requested row does not exist.
    NotFound,
    /// The change breaks a rule of the vault.
    Rule,
    /// Undo or redo would overwrite a later change.
    UndoBlocked,
    /// Another window or process has the vault open.
    Locked,
    /// The folder is not a vault, or not named `<name>.dbme`, or already in use.
    BadPath,
    /// The vault needs a newer version of the app.
    TooNew,
    /// A value breaks the data model.
    Invalid,
    /// Reading or writing failed.
    Storage,
}

/// An error returned by a command.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, specta::Type)]
pub struct IpcError {
    /// What kind of problem it is.
    pub kind: IpcErrorKind,
    /// A sentence for people.
    pub message: String,
}

impl IpcError {
    fn new(kind: IpcErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

impl From<StoreError> for IpcError {
    fn from(error: StoreError) -> Self {
        let kind = match &error {
            StoreError::NotFound { .. } => IpcErrorKind::NotFound,
            StoreError::Rule(_) => IpcErrorKind::Rule,
            StoreError::UndoBlocked(_) => IpcErrorKind::UndoBlocked,
            StoreError::Locked(_) => IpcErrorKind::Locked,
            StoreError::NotAVault(_)
            | StoreError::AlreadyExists(_)
            | StoreError::BadVaultName(_) => IpcErrorKind::BadPath,
            StoreError::TooNew { .. } => IpcErrorKind::TooNew,
            StoreError::Core(CoreError::Conflict { .. }) => IpcErrorKind::UndoBlocked,
            StoreError::Core(_) => IpcErrorKind::Invalid,
            StoreError::Sql(_) | StoreError::Io { .. } | StoreError::MissingBlob(_) => {
                IpcErrorKind::Storage
            }
        };
        Self::new(kind, error.to_string())
    }
}

type Reply<T> = Result<T, IpcError>;

/// Sent after every change to the open vault, including undo and redo.
#[derive(Clone, Debug, Serialize, Deserialize, specta::Type, tauri_specta::Event)]
pub struct VaultChanged(pub Committed);

/// About the running app.
#[derive(Clone, Debug, Serialize, specta::Type)]
pub struct AppInfo {
    /// The product name.
    pub product: String,
    /// The release.
    pub version: String,
    /// The vault format version this release writes.
    pub format_version: u32,
    /// In smoke-test mode, where the launch test should create its vault.
    pub smoke_vault: Option<String>,
}

/// About the open vault.
#[derive(Clone, Debug, Serialize, specta::Type)]
pub struct VaultInfo {
    /// The vault's ID.
    pub id: Id,
    /// Its folder.
    pub path: String,
    /// Whether there is a step to undo.
    pub can_undo: bool,
    /// Whether there is a step to redo.
    pub can_redo: bool,
}

impl Session {
    fn lock(&self) -> MutexGuard<'_, Option<Vault>> {
        // A panic while the lock was held cannot leave the vault half-changed: SQLite rolls
        // back the unfinished transaction. So a poisoned lock is safe to use.
        self.vault
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Closes the open vault, if any.
    pub(crate) fn close(&self) {
        *self.lock() = None;
    }

    fn with<T>(&self, use_vault: impl FnOnce(&mut Vault) -> Result<T, StoreError>) -> Reply<T> {
        let mut guard = self.lock();
        let vault = guard
            .as_mut()
            .ok_or_else(|| IpcError::new(IpcErrorKind::NoVault, "no vault is open"))?;
        Ok(use_vault(vault)?)
    }

    /// Runs a change and tells the UI what changed.
    fn change<T>(
        &self,
        app: &AppHandle,
        change: impl FnOnce(&mut Vault) -> Result<(T, Option<Committed>), StoreError>,
    ) -> Reply<T> {
        let (value, committed) = self.with(change)?;
        if let Some(committed) = committed {
            // The change is saved whether or not the event gets through; a lost event only
            // delays the UI's refresh.
            if let Err(error) = VaultChanged(committed).emit(app) {
                tracing::warn!(%error, "could not send vault-changed");
            }
        }
        Ok(value)
    }
}

fn info(vault: &Vault) -> VaultInfo {
    VaultInfo {
        id: vault.id(),
        path: vault.root().display().to_string(),
        can_undo: vault.can_undo(),
        can_redo: vault.can_redo(),
    }
}

/// Where the identity file lives: the app's configuration folder, or a temporary one in smoke-test
/// mode so the launch test never touches a real profile.
fn config_dir(app: &AppHandle, startup: &Startup) -> Reply<PathBuf> {
    if let Some(dir) = &startup.smoke_dir {
        return Ok(dir.join("config"));
    }
    app.path().app_config_dir().map_err(|e| {
        IpcError::new(
            IpcErrorKind::Storage,
            format!("cannot find the configuration folder: {e}"),
        )
    })
}

fn options(app: &AppHandle, startup: &Startup) -> Reply<VaultOptions> {
    let clock = Arc::new(SystemClock);
    let identity = Identity::load_or_create(&config_dir(app, startup)?, clock.as_ref())?;
    Ok(VaultOptions { identity, clock })
}

/// Opens or creates a vault in place of the one open now, if any.
fn replace(
    session: &Session,
    open: impl FnOnce() -> Result<Vault, StoreError>,
) -> Reply<VaultInfo> {
    let mut guard = session.lock();
    // Close the old vault first: it may be the one being opened again.
    *guard = None;
    let vault = open()?;
    let info = info(&vault);
    // The ID only: paths can name people and projects (logs never hold user content).
    tracing::info!(vault = %info.id, "vault open");
    *guard = Some(vault);
    Ok(info)
}

/// Product, release and format versions, and smoke-test settings.
#[tauri::command]
#[specta::specta]
pub(crate) fn app_info(startup: State<'_, Startup>) -> AppInfo {
    AppInfo {
        product: dbme_core::PRODUCT_NAME.to_owned(),
        version: dbme_core::VERSION.to_owned(),
        format_version: dbme_core::FORMAT_VERSION,
        smoke_vault: startup
            .smoke_dir
            .as_ref()
            .map(|dir| dir.join("Smoke.dbme").display().to_string()),
    }
}

/// Called by the UI after its first render, with `error` set if its launch checks failed. Logs how
/// long startup took and, in smoke-test mode, closes the vault and exits.
#[tauri::command]
#[specta::specta]
pub(crate) fn app_ready(
    app: AppHandle,
    startup: State<'_, Startup>,
    session: State<'_, Session>,
    error: Option<String>,
) {
    crate::ready(&app, &startup, error, || session.close());
}

/// Creates a vault at `path` (a folder named `<name>.dbme`) and opens it.
#[tauri::command]
#[specta::specta]
pub(crate) async fn vault_create(
    app: AppHandle,
    startup: State<'_, Startup>,
    session: State<'_, Session>,
    path: String,
) -> Reply<VaultInfo> {
    let options = options(&app, &startup)?;
    replace(&session, || Vault::create(Path::new(&path), options))
}

/// Opens the vault at `path`.
#[tauri::command]
#[specta::specta]
pub(crate) async fn vault_open(
    app: AppHandle,
    startup: State<'_, Startup>,
    session: State<'_, Session>,
    path: String,
) -> Reply<VaultInfo> {
    let options = options(&app, &startup)?;
    replace(&session, || Vault::open(Path::new(&path), options))
}

/// Closes the open vault, if any.
#[tauri::command]
#[specta::specta]
pub(crate) async fn vault_close(session: State<'_, Session>) -> Reply<()> {
    session.close();
    Ok(())
}

/// The open vault, or `None`.
#[tauri::command]
#[specta::specta]
pub(crate) async fn vault_info(session: State<'_, Session>) -> Reply<Option<VaultInfo>> {
    Ok(session.lock().as_ref().map(info))
}

/// Every type, by name.
#[tauri::command]
#[specta::specta]
pub(crate) async fn types_list(session: State<'_, Session>) -> Reply<Vec<TypeRow>> {
    session.with(|vault| vault.read().types())
}

/// One object.
#[tauri::command]
#[specta::specta]
pub(crate) async fn object_get(session: State<'_, Session>, id: Id) -> Reply<ObjectRow> {
    session.with(|vault| vault.read().object(id))
}

/// The live children of `parent`, or the top-level objects, in order.
#[tauri::command]
#[specta::specta]
pub(crate) async fn object_children(
    session: State<'_, Session>,
    parent: Option<Id>,
) -> Reply<Vec<ObjectRow>> {
    session.with(|vault| vault.read().children(parent))
}

/// Creates an object and returns its ID.
#[tauri::command]
#[specta::specta]
pub(crate) async fn object_create(
    app: AppHandle,
    session: State<'_, Session>,
    object: NewObject,
) -> Reply<Id> {
    session.change(&app, |vault| {
        vault.transact(|txn| txn.create_object(object))
    })
}

/// Renames an object. Returns `false` if the title was already `title`.
#[tauri::command]
#[specta::specta]
pub(crate) async fn object_rename(
    app: AppHandle,
    session: State<'_, Session>,
    id: Id,
    title: String,
) -> Reply<bool> {
    session.change(&app, |vault| {
        vault.transact(|txn| txn.update_object(id, |o| o.title = title))
    })
}

/// Moves an object, and everything below it, to the Trash.
#[tauri::command]
#[specta::specta]
pub(crate) async fn object_trash(app: AppHandle, session: State<'_, Session>, id: Id) -> Reply<()> {
    session.change(&app, |vault| vault.transact(|txn| txn.trash_object(id)))
}

/// Takes an object out of the Trash.
#[tauri::command]
#[specta::specta]
pub(crate) async fn object_restore(
    app: AppHandle,
    session: State<'_, Session>,
    id: Id,
) -> Reply<()> {
    session.change(&app, |vault| vault.transact(|txn| txn.restore_object(id)))
}

/// Undoes the last step. Returns `None` if there is nothing to undo.
#[tauri::command]
#[specta::specta]
pub(crate) async fn undo(app: AppHandle, session: State<'_, Session>) -> Reply<Option<Committed>> {
    session.change(&app, |vault| vault.undo().map(|c| (c.clone(), c)))
}

/// Redoes the last undone step. Returns `None` if there is nothing to redo.
#[tauri::command]
#[specta::specta]
pub(crate) async fn redo(app: AppHandle, session: State<'_, Session>) -> Reply<Option<Committed>> {
    session.change(&app, |vault| vault.redo().map(|c| (c.clone(), c)))
}

/// The commands and events, for Tauri and for generating the TypeScript bindings.
pub(crate) fn builder() -> tauri_specta::Builder<tauri::Wry> {
    tauri_specta::Builder::<tauri::Wry>::new()
        .commands(tauri_specta::collect_commands![
            app_info,
            app_ready,
            vault_create,
            vault_open,
            vault_close,
            vault_info,
            types_list,
            object_get,
            object_children,
            object_create,
            object_rename,
            object_trash,
            object_restore,
            undo,
            redo,
        ])
        .events(tauri_specta::collect_events![VaultChanged])
        // Every 64-bit integer that crosses IPC is a timestamp in milliseconds or an op sequence
        // number, far below 2^53, so a JavaScript number holds it exactly (ADR-0014).
        .dangerously_cast_bigints_to_number()
}

/// How the bindings are written. IDs, sort keys and hashes are strings.
#[cfg(test)]
pub(crate) fn typescript() -> specta_typescript::Typescript {
    specta_typescript::Typescript::default()
        .header("// SPDX-License-Identifier: AGPL-3.0-or-later\n// Generated from apps/desktop/src-tauri/src/ipc.rs by the test `bindings_are_current`. Do not edit.\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    const BINDINGS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../src/bindings.ts");

    /// The checked-in bindings must match what the Rust code generates. Set `DBME_WRITE_BINDINGS`
    /// to regenerate them.
    #[test]
    fn bindings_are_current() {
        let dir = tempfile::tempdir().unwrap();
        let fresh = dir.path().join("bindings.ts");
        builder().export(typescript(), &fresh).unwrap();
        let fresh = std::fs::read_to_string(fresh).unwrap();
        if std::env::var_os("DBME_WRITE_BINDINGS").is_some() {
            std::fs::write(BINDINGS, &fresh).unwrap();
        }
        let checked_in = std::fs::read_to_string(BINDINGS)
            .unwrap_or_default()
            .replace("\r\n", "\n");
        assert!(
            checked_in == fresh.replace("\r\n", "\n"),
            "apps/desktop/src/bindings.ts is stale: run `DBME_WRITE_BINDINGS=1 cargo test -p dbme-desktop bindings_are_current` and commit the result"
        );
    }

    /// Every command is listed in build.rs (which generates its permission), granted to the main
    /// window, and registered with the builder.
    #[test]
    fn commands_are_declared_everywhere() {
        let build = include_str!("../build.rs");
        let list = &build[build.find("const COMMANDS").unwrap()..];
        let list = &list[..list.find("];").unwrap()];
        let commands: Vec<&str> = list.split('"').skip(1).step_by(2).collect();
        assert_eq!(commands.len(), 15);
        let capability = include_str!("../capabilities/default.json");
        let source = include_str!("ipc.rs");
        let registered = &source[source.find("collect_commands![").unwrap()..];
        let registered = &registered[..registered.find(']').unwrap()];
        for command in &commands {
            assert!(
                capability.contains(&format!("\"allow-{}\"", command.replace('_', "-"))),
                "{command} is not granted"
            );
            assert!(
                registered.contains(&format!("{command},")),
                "{command} is not registered"
            );
            assert!(
                source.contains(&format!("fn {command}(")),
                "{command} has no function"
            );
        }
        assert_eq!(
            registered.matches(',').count(),
            commands.len(),
            "a registered command is missing from build.rs"
        );
    }

    #[test]
    fn store_errors_map_to_kinds() {
        let cases = [
            (StoreError::Rule("no".into()), IpcErrorKind::Rule),
            (
                StoreError::UndoBlocked("no".into()),
                IpcErrorKind::UndoBlocked,
            ),
            (StoreError::Locked("a.dbme".into()), IpcErrorKind::Locked),
            (StoreError::BadVaultName("a".into()), IpcErrorKind::BadPath),
            (
                StoreError::TooNew {
                    found: 9,
                    supported: 1,
                },
                IpcErrorKind::TooNew,
            ),
            (
                StoreError::Core(CoreError::invalid("x", "y")),
                IpcErrorKind::Invalid,
            ),
            (
                StoreError::NotFound {
                    entity: "object",
                    id: "x".into(),
                },
                IpcErrorKind::NotFound,
            ),
            (
                StoreError::MissingBlob(dbme_core::ContentHash::of(b"x")),
                IpcErrorKind::Storage,
            ),
        ];
        for (error, kind) in cases {
            let message = error.to_string();
            assert_eq!(IpcError::from(error), IpcError { kind, message });
        }
    }
}
