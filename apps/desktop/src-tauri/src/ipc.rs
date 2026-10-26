// SPDX-License-Identifier: AGPL-3.0-or-later
//! The commands the web UI calls, and the events it listens to (ADR-0014).
//!
//! Each vault command is a one-line wrapper around the function of the same name in `dbme-api`,
//! which the e2e test server calls too (ADR-0018). Only the app-level commands (`app_info`,
//! `app_ready`, and the commands that find a vault's folder) live here.
//!
//! Every command and every type it takes or returns is described to `tauri-specta`, which
//! generates the TypeScript bindings in `packages/client/src/bindings.ts`. The test
//! `bindings_are_current` fails when that file no longer matches the Rust code; run it with
//! `DBME_WRITE_BINDINGS=1` to regenerate the file.
//!
//! Commands that touch the vault are `async`, so they run on Tauri's thread pool rather than the
//! UI thread.

use std::path::PathBuf;
use std::sync::Arc;

use dbme_api::{self as api, BlockChange, IpcError, IpcErrorKind, Reply, Session, VaultInfo};
use dbme_core::model::{AttachmentRow, BlockRow, FieldRow, ObjectRow, TypeRow};
use dbme_core::{ContentHash, Id, SystemClock};
use dbme_store::{
    Backlink, Committed, Identity, NewField, NewObject, ObjectSummary, Position, Sort, Source,
    VaultOptions,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Manager, State};
use tauri_specta::Event;

use crate::Startup;

/// The name of the vault the app opens when the UI asks for the default one.
pub const DEFAULT_VAULT: &str = "Main.dbme";

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

/// The session of the main window: its vault, and the `vault-changed` events it sends.
pub(crate) fn session(app: &AppHandle) -> Session {
    let app = app.clone();
    Session::new(move |committed| {
        // The change is saved whether or not the event gets through; a lost event only delays the
        // UI's refresh.
        if let Err(error) = VaultChanged(committed.clone()).emit(&app) {
            tracing::warn!(%error, "could not send vault-changed");
        }
    })
}

fn storage(what: &str, error: impl std::fmt::Display) -> IpcError {
    IpcError::new(IpcErrorKind::Storage, format!("{what}: {error}"))
}

/// Where the identity file and the default vault live: the app's configuration and data folders,
/// or a temporary folder in smoke-test mode so the launch test never touches a real profile.
fn app_dir(app: &AppHandle, startup: &Startup, config: bool) -> Reply<PathBuf> {
    if let Some(dir) = &startup.smoke_dir {
        return Ok(dir.join(if config { "config" } else { "data" }));
    }
    let dir = if config {
        app.path().app_config_dir()
    } else {
        app.path().app_data_dir()
    };
    dir.map_err(|e| storage("cannot find the app's folders", e))
}

fn options(app: &AppHandle, startup: &Startup) -> Reply<VaultOptions> {
    let clock = Arc::new(SystemClock);
    let identity = Identity::load_or_create(&app_dir(app, startup, true)?, clock.as_ref())
        .map_err(IpcError::from)?;
    Ok(VaultOptions { identity, clock })
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
    api::vault_create(&session, &path, options(&app, &startup)?)
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
    api::vault_open(&session, &path, options(&app, &startup)?)
}

/// Opens the default vault in the app's data folder, creating it the first time (ADR-0017).
#[tauri::command]
#[specta::specta]
pub(crate) async fn vault_open_default(
    app: AppHandle,
    startup: State<'_, Startup>,
    session: State<'_, Session>,
) -> Reply<VaultInfo> {
    let folder = app_dir(&app, &startup, false)?.join("vaults");
    let path = folder.join(DEFAULT_VAULT);
    let options = options(&app, &startup)?;
    if path.exists() {
        return api::vault_open(&session, &path.display().to_string(), options);
    }
    std::fs::create_dir_all(&folder).map_err(|e| storage("cannot create the vaults folder", e))?;
    api::vault_create(&session, &path.display().to_string(), options)
}

/// Closes the open vault, if any.
#[tauri::command]
#[specta::specta]
pub(crate) async fn vault_close(session: State<'_, Session>) -> Reply<()> {
    api::vault_close(&session)
}

/// The open vault, or `None`.
#[tauri::command]
#[specta::specta]
pub(crate) async fn vault_info(session: State<'_, Session>) -> Reply<Option<VaultInfo>> {
    api::vault_info(&session)
}

/// Undoes the last step. Returns `None` if there is nothing to undo.
#[tauri::command]
#[specta::specta]
pub(crate) async fn undo(session: State<'_, Session>) -> Reply<Option<Committed>> {
    api::undo(&session)
}

/// Redoes the last undone step. Returns `None` if there is nothing to redo.
#[tauri::command]
#[specta::specta]
pub(crate) async fn redo(session: State<'_, Session>) -> Reply<Option<Committed>> {
    api::redo(&session)
}

/// Every type, by name.
#[tauri::command]
#[specta::specta]
pub(crate) async fn types_list(session: State<'_, Session>) -> Reply<Vec<TypeRow>> {
    api::types_list(&session)
}

/// The fields the objects of `source` can show, one per key.
#[tauri::command]
#[specta::specta]
pub(crate) async fn fields_list(
    session: State<'_, Session>,
    source: Source,
) -> Reply<Vec<FieldRow>> {
    api::fields_list(&session, source)
}

/// Adds a field to a type and returns its ID.
#[tauri::command]
#[specta::specta]
pub(crate) async fn field_create(session: State<'_, Session>, field: NewField) -> Reply<Id> {
    api::field_create(&session, field)
}

/// One object.
#[tauri::command]
#[specta::specta]
pub(crate) async fn object_get(session: State<'_, Session>, id: Id) -> Reply<ObjectRow> {
    api::object_get(&session, id)
}

/// The live children of `parent`, or the top-level objects, in order.
#[tauri::command]
#[specta::specta]
pub(crate) async fn object_children(
    session: State<'_, Session>,
    parent: Option<Id>,
) -> Reply<Vec<ObjectRow>> {
    api::object_children(&session, parent)
}

/// The objects above `id`, from the top level down to its parent.
#[tauri::command]
#[specta::specta]
pub(crate) async fn object_ancestors(session: State<'_, Session>, id: Id) -> Reply<Vec<ObjectRow>> {
    api::object_ancestors(&session, id)
}

/// The summaries of `ids`, in the same order, skipping objects that do not exist.
#[tauri::command]
#[specta::specta]
pub(crate) async fn object_summaries(
    session: State<'_, Session>,
    ids: Vec<Id>,
) -> Reply<Vec<ObjectSummary>> {
    api::object_summaries(&session, &ids)
}

/// Up to `limit` objects of `source` in `sort` order, starting at `offset`.
#[tauri::command]
#[specta::specta]
pub(crate) async fn objects_page(
    session: State<'_, Session>,
    source: Source,
    sort: Sort,
    offset: u32,
    limit: u32,
) -> Reply<Vec<ObjectSummary>> {
    api::objects_page(&session, source, &sort, offset, limit)
}

/// How many objects `source` lists.
#[tauri::command]
#[specta::specta]
pub(crate) async fn objects_count(session: State<'_, Session>, source: Source) -> Reply<u32> {
    api::objects_count(&session, source)
}

/// Live objects whose title contains `text`, prefixes first; with no text, the most recently
/// changed.
#[tauri::command]
#[specta::specta]
pub(crate) async fn object_search(
    session: State<'_, Session>,
    text: String,
    limit: u32,
) -> Reply<Vec<ObjectSummary>> {
    api::object_search(&session, &text, limit)
}

/// The live relations to an object, with their sources.
#[tauri::command]
#[specta::specta]
pub(crate) async fn backlinks(session: State<'_, Session>, id: Id) -> Reply<Vec<Backlink>> {
    api::backlinks(&session, id)
}

/// Creates an object and returns its ID.
#[tauri::command]
#[specta::specta]
pub(crate) async fn object_create(session: State<'_, Session>, object: NewObject) -> Reply<Id> {
    api::object_create(&session, object)
}

/// Renames an object. Returns `false` if the title was already `title`.
#[tauri::command]
#[specta::specta]
pub(crate) async fn object_rename(
    session: State<'_, Session>,
    id: Id,
    title: String,
) -> Reply<bool> {
    api::object_rename(&session, id, title)
}

/// Sets the value of field `key` on an object, or clears it when `value` is `null`. Returns
/// `false` if nothing changed.
#[tauri::command]
#[specta::specta]
pub(crate) async fn object_set_prop(
    session: State<'_, Session>,
    id: Id,
    key: String,
    value: Option<Value>,
) -> Reply<bool> {
    api::object_set_prop(&session, id, key, value)
}

/// Moves an object under `parent` (`null`: the top level), at `position` among its new
/// siblings. Returns `false` if it did not move.
#[tauri::command]
#[specta::specta]
pub(crate) async fn object_move(
    session: State<'_, Session>,
    id: Id,
    parent: Option<Id>,
    position: Position,
) -> Reply<bool> {
    api::object_move(&session, id, parent, position)
}

/// Moves an object, and everything below it, to the Trash.
#[tauri::command]
#[specta::specta]
pub(crate) async fn object_trash(session: State<'_, Session>, id: Id) -> Reply<()> {
    api::object_trash(&session, id)
}

/// Takes an object out of the Trash.
#[tauri::command]
#[specta::specta]
pub(crate) async fn object_restore(session: State<'_, Session>, id: Id) -> Reply<()> {
    api::object_restore(&session, id)
}

/// Gives an object another type. Returns `false` if it already had it.
#[tauri::command]
#[specta::specta]
pub(crate) async fn object_add_type(
    session: State<'_, Session>,
    id: Id,
    type_id: Id,
) -> Reply<bool> {
    api::object_add_type(&session, id, type_id)
}

/// Takes a type away from an object. Returns `false` if it did not have it.
#[tauri::command]
#[specta::specta]
pub(crate) async fn object_remove_type(
    session: State<'_, Session>,
    id: Id,
    type_id: Id,
) -> Reply<bool> {
    api::object_remove_type(&session, id, type_id)
}

/// All blocks of an object, in tree order.
#[tauri::command]
#[specta::specta]
pub(crate) async fn blocks_list(
    session: State<'_, Session>,
    object_id: Id,
) -> Reply<Vec<BlockRow>> {
    api::blocks_list(&session, object_id)
}

/// Applies an editor's changes to one object's blocks, in order, as one undo step.
#[tauri::command]
#[specta::specta]
pub(crate) async fn blocks_apply(
    session: State<'_, Session>,
    object_id: Id,
    changes: Vec<BlockChange>,
) -> Reply<()> {
    api::blocks_apply(&session, object_id, changes)
}

/// An object's attachments.
#[tauri::command]
#[specta::specta]
pub(crate) async fn attachments_list(
    session: State<'_, Session>,
    object_id: Id,
) -> Reply<Vec<AttachmentRow>> {
    api::attachments_list(&session, object_id)
}

/// Stores `data` (base64) and attaches it to an object as an original. Returns its hash.
#[tauri::command]
#[specta::specta]
pub(crate) async fn attachment_add(
    session: State<'_, Session>,
    object_id: Id,
    data: String,
    filename: Option<String>,
    mime: Option<String>,
) -> Reply<ContentHash> {
    api::attachment_add(&session, object_id, &data, filename, mime)
}

/// A blob's bytes, as base64.
#[tauri::command]
#[specta::specta]
pub(crate) async fn blob_read(session: State<'_, Session>, hash: ContentHash) -> Reply<String> {
    api::blob_read(&session, &hash)
}

/// The commands and events, for Tauri and for generating the TypeScript bindings.
pub(crate) fn builder() -> tauri_specta::Builder<tauri::Wry> {
    tauri_specta::Builder::<tauri::Wry>::new()
        .commands(tauri_specta::collect_commands![
            app_info,
            app_ready,
            vault_create,
            vault_open,
            vault_open_default,
            vault_close,
            vault_info,
            undo,
            redo,
            types_list,
            fields_list,
            field_create,
            object_get,
            object_children,
            object_ancestors,
            object_summaries,
            objects_page,
            objects_count,
            object_search,
            backlinks,
            object_create,
            object_rename,
            object_set_prop,
            object_move,
            object_trash,
            object_restore,
            object_add_type,
            object_remove_type,
            blocks_list,
            blocks_apply,
            attachments_list,
            attachment_add,
            blob_read,
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

    const BINDINGS: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../packages/client/src/bindings.ts"
    );

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
            "packages/client/src/bindings.ts is stale: run `DBME_WRITE_BINDINGS=1 cargo test -p dbme-desktop bindings_are_current` and commit the result"
        );
    }

    /// Every command is listed in build.rs (which generates its permission), granted to the main
    /// window, registered with the builder, and served by the e2e test server.
    #[test]
    fn commands_are_declared_everywhere() {
        let build = include_str!("../build.rs");
        let list = &build[build.find("const COMMANDS").unwrap()..];
        let list = &list[..list.find("];").unwrap()];
        let commands: Vec<&str> = list.split('"').skip(1).step_by(2).collect();
        assert_eq!(commands.len(), 33);
        let capability = include_str!("../capabilities/default.json");
        let source = include_str!("ipc.rs");
        let registered = &source[source.find("collect_commands![").unwrap()..];
        let registered = &registered[..registered.find(']').unwrap()];
        let served = include_str!("../../../../crates/e2e-server/src/dispatch.rs");
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
            assert!(
                served.contains(&format!("\"{command}\" =>")),
                "the e2e server does not serve {command}"
            );
        }
        assert_eq!(
            registered.matches(',').count(),
            commands.len(),
            "a registered command is missing from build.rs"
        );
        assert_eq!(
            capability.matches("\"allow-").count(),
            commands.len(),
            "the capability grants a command that does not exist"
        );
    }
}
