// SPDX-License-Identifier: AGPL-3.0-or-later
//! The files folder's commands (ADR-0021, M5 Task 6): enabling the folder, syncing it, the
//! Conflicts inbox and the export, plus the worker that keeps the folder and the vault in step
//! while the app runs.
//!
//! The engine, [`Mirror`], lives in the [`Session`] beside the vault and runs only while the
//! folder is enabled (`mirror_meta.folder_enabled`). Its location is `files/` inside the vault
//! folder unless an override file in the app's configuration folder names another place
//! (Decision 20: a machine path never goes inside the vault, which may move). The worker thread
//! writes the objects the store marked dirty after each commit, debounced, and feeds the watcher's
//! events back in; a shell without a worker (the e2e server) drives both with
//! [`mirror_sync_now`] instead (Decision 25).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use dbme_core::{Id, SystemClock};
use dbme_export::ExportError;
use dbme_mirror::{
    DEFAULT_DEBOUNCE, FileEvent, HoldDecision, Inbound, Mirror, MirrorError, NotifyWatcher,
    Outbound, StdFs,
};
use dbme_store::{Committed, Conflict, ConflictKind, FOLDER_ENABLED, MirrorFileState, Vault};

use crate::{IpcError, IpcErrorKind, Reply, Session};

pub use dbme_export::ExportReport;

/// Where a vault's files folder lives unless an override says otherwise: `files/` inside the
/// vault folder (FORMAT.md §1).
pub const DEFAULT_FOLDER: &str = "files";

/// The folder under the app's configuration folder that holds one override file per vault,
/// `<vault id>.txt`, whose content is the files folder's path.
pub const OVERRIDES_DIR: &str = "files-folders";

/// The engine while the folder is enabled.
pub(crate) struct Running {
    pub(crate) engine: Mirror<StdFs>,
    pub(crate) folder: PathBuf,
}

/// Where the files folder stands, for the status line and the `mirror-changed` event.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct MirrorStatus {
    /// Whether the folder is on for this vault.
    pub enabled: bool,
    /// The folder's path, when it is on.
    pub folder: Option<String>,
    /// Objects whose file is up to date.
    pub synced: u32,
    /// Objects whose write keeps failing; retried.
    pub pending: u32,
    /// Objects held by a parse error or a bulk-change hold.
    pub held: u32,
    /// Objects waiting to be written.
    pub dirty: u32,
    /// Entries of the Conflicts inbox not yet resolved.
    pub open_conflicts: u32,
    /// The open bulk-change hold, if any: the UI shows its banner.
    pub hold: Option<Id>,
    /// Whether every file is about to be written again (first enable, or a type change).
    pub full_rewrite_needed: bool,
}

/// What one sync did, in both directions.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct MirrorSync {
    /// Files written.
    pub written: u32,
    /// Files moved.
    pub renamed: u32,
    /// Files moved to `.dbme/trash/`.
    pub trashed_files: u32,
    /// Files removed for good.
    pub removed_files: u32,
    /// Objects whose write another program blocked.
    pub pending: u32,
    /// Objects changed from their files.
    pub applied: u32,
    /// Objects made for new files.
    pub created: u32,
    /// Objects moved or renamed from their files.
    pub moved: u32,
    /// Objects trashed because their file is gone.
    pub trashed_objects: u32,
    /// Inbox entries made.
    pub conflicts: u32,
    /// The hold made, when the batch was held and nothing applied.
    pub held: Option<Id>,
}

impl MirrorSync {
    fn count(ids: usize) -> u32 {
        u32::try_from(ids).unwrap_or(u32::MAX)
    }

    fn add_out(&mut self, out: &Outbound) {
        self.written += Self::count(out.written.len());
        self.renamed += Self::count(out.renamed.len());
        self.trashed_files += Self::count(out.trashed.len());
        self.removed_files += Self::count(out.removed.len());
        self.pending = Self::count(out.pending.len());
    }

    fn add_in(&mut self, inbound: &Inbound) {
        self.applied += Self::count(inbound.applied.len());
        self.created += Self::count(inbound.created.len());
        self.moved += Self::count(inbound.moved.len());
        self.trashed_objects += Self::count(inbound.trashed.len());
        self.conflicts += Self::count(inbound.conflicts.len());
        if inbound.held.is_some() {
            self.held = inbound.held;
        }
    }
}

/// What to do with a held batch of changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum HoldChoice {
    /// Apply the held changes: the removals and rewrites were meant.
    Proceed,
    /// Put the folder back the way the vault has it.
    Restore,
}

impl From<HoldChoice> for HoldDecision {
    fn from(choice: HoldChoice) -> Self {
        match choice {
            HoldChoice::Proceed => Self::Proceed,
            HoldChoice::Restore => Self::Restore,
        }
    }
}

impl From<MirrorError> for IpcError {
    fn from(error: MirrorError) -> Self {
        match error {
            MirrorError::Store(e) => e.into(),
            MirrorError::Core(e) => e.into(),
            MirrorError::Io { .. } => Self::new(IpcErrorKind::Storage, error.to_string()),
        }
    }
}

impl From<ExportError> for IpcError {
    fn from(error: ExportError) -> Self {
        match error {
            ExportError::Store(e) => e.into(),
            ExportError::Rule(_) => Self::new(IpcErrorKind::Rule, error.to_string()),
            ExportError::Io { .. } | ExportError::Json { .. } => {
                Self::new(IpcErrorKind::Storage, error.to_string())
            }
        }
    }
}

fn storage(what: &str, error: impl std::fmt::Display) -> IpcError {
    IpcError::new(IpcErrorKind::Storage, format!("{what}: {error}"))
}

fn count(n: u64) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// The override file for a vault, if this session keeps settings.
fn override_file(config_dir: Option<&Path>, vault_id: Id) -> Option<PathBuf> {
    config_dir.map(|dir| dir.join(OVERRIDES_DIR).join(format!("{vault_id}.txt")))
}

/// Where a vault's files folder is: the override, or `files/` inside the vault.
pub(crate) fn folder_for(config_dir: Option<&Path>, vault: &Vault) -> PathBuf {
    override_file(config_dir, vault.id())
        .and_then(|file| std::fs::read_to_string(file).ok())
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| vault.root().join(DEFAULT_FOLDER))
}

/// Starts the engine over the vault's folder.
pub(crate) fn start(config_dir: Option<&Path>, vault: &Vault) -> Reply<Running> {
    let folder = folder_for(config_dir, vault);
    let fs = StdFs::new(&folder).map_err(|e| {
        storage(
            &format!("cannot use {} as the files folder", folder.display()),
            e,
        )
    })?;
    Ok(Running {
        engine: Mirror::new(fs, Arc::new(SystemClock)),
        folder,
    })
}

/// The folder as a person reads it: a canonical Windows path carries the `\\?\` verbatim prefix
/// (`\\?\UNC\` for a share), which is the file system's business, not the dialog's.
fn shown(folder: &Path) -> String {
    let text = folder.display().to_string();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = text.strip_prefix(r"\\?\") {
        rest.to_owned()
    } else {
        text
    }
}

/// The status, read from the vault's bookkeeping.
pub(crate) fn status_of(vault: &Vault, running: Option<&Running>) -> Reply<MirrorStatus> {
    let mut status = MirrorStatus {
        enabled: running.is_some(),
        folder: running.map(|r| shown(&r.folder)),
        ..MirrorStatus::default()
    };
    for state in vault.mirror_states(None)? {
        match state.state {
            MirrorFileState::Synced => status.synced += 1,
            MirrorFileState::Pending => status.pending += 1,
            MirrorFileState::Held => status.held += 1,
            MirrorFileState::Excluded | MirrorFileState::Trashed => {}
        }
    }
    status.dirty = count(vault.mirror_dirty_count()?);
    let open = vault.conflicts(true)?;
    status.open_conflicts = count(open.len() as u64);
    status.hold = open
        .iter()
        .find(|c| c.kind == ConflictKind::BulkChangeHold)
        .map(|c| c.id);
    status.full_rewrite_needed = vault
        .mirror_meta_get(dbme_store::FULL_REWRITE_NEEDED)?
        .is_some();
    Ok(status)
}

/// The `seq` of the vault's newest op, or 0.
fn latest_seq(vault: &Vault) -> Reply<i64> {
    Ok(vault.read().history(1)?.first().map_or(0, |r| r.seq))
}

/// One `vault-changed` record for an inbound batch: the ops it appended, as one step.
fn inbound_committed(vault: &Vault, before: i64, inbound: &Inbound) -> Reply<Option<Committed>> {
    let newest = vault.read().history(1)?;
    let Some(last) = newest.first().filter(|r| r.seq > before) else {
        return Ok(None);
    };
    let mut changed: Vec<Id> = inbound
        .applied
        .iter()
        .chain(&inbound.created)
        .chain(&inbound.moved)
        .chain(&inbound.trashed)
        .copied()
        .collect();
    changed.sort_unstable();
    changed.dedup();
    Ok(Some(Committed {
        group_id: last.entry.group_id,
        undo_of: None,
        first_seq: before + 1,
        last_seq: last.seq,
        changed,
    }))
}

impl Session {
    /// Runs `work` with the open vault and the engine's slot (`None` while the folder is off).
    fn with_running<T>(
        &self,
        work: impl FnOnce(&mut Vault, &mut Option<Running>) -> Reply<T>,
    ) -> Reply<T> {
        let mut vault = self.lock();
        let vault = vault
            .as_mut()
            .ok_or_else(|| IpcError::new(IpcErrorKind::NoVault, "no vault is open"))?;
        let mut running = self.lock_mirror();
        work(vault, &mut running)
    }

    /// Runs `work` with the open vault and the running engine.
    fn with_engine<T>(&self, work: impl FnOnce(&mut Vault, &mut Running) -> Reply<T>) -> Reply<T> {
        self.with_running(|vault, running| {
            let running = running.as_mut().ok_or_else(|| {
                IpcError::new(IpcErrorKind::Rule, "the files folder is not enabled")
            })?;
            work(vault, running)
        })
    }

    /// Tells the shell the folder's status changed.
    fn mirror_changed(&self) {
        if let Ok(status) = self.with_running(|vault, running| status_of(vault, running.as_ref())) {
            (self.mirror_notify)(&status);
        }
    }

    /// The folder the engine runs over, if it runs.
    pub fn mirror_folder(&self) -> Option<PathBuf> {
        self.lock_mirror().as_ref().map(|r| r.folder.clone())
    }

    /// Writes every object the store marked dirty. `None` while the folder is off. The worker
    /// calls this after each commit; a shell without a worker calls [`mirror_sync_now`].
    pub fn mirror_pump_out(&self) -> Reply<Option<Outbound>> {
        let out = self.with_running(|vault, running| match running {
            Some(running) => Ok(Some(running.engine.pump_out(vault)?)),
            None => Ok(None),
        })?;
        if let Some(out) = &out
            && (!out.written.is_empty()
                || !out.renamed.is_empty()
                || !out.trashed.is_empty()
                || !out.removed.is_empty()
                || !out.pending.is_empty())
        {
            self.mirror_changed();
        }
        Ok(out)
    }

    /// Applies a batch of the folder's events to the vault. `None` while the folder is off. The
    /// inbound commits are one `vault-changed` to the UI, and anything written back is pumped out.
    pub fn mirror_pump_in(&self, events: Vec<FileEvent>) -> Reply<Option<Inbound>> {
        let result = self.with_running(|vault, running| match running {
            Some(running) => {
                let before = latest_seq(vault)?;
                let inbound = running.engine.pump_in(vault, events)?;
                let committed = inbound_committed(vault, before, &inbound)?;
                Ok(Some((inbound, committed)))
            }
            None => Ok(None),
        })?;
        let Some((inbound, committed)) = result else {
            return Ok(None);
        };
        if let Some(committed) = committed {
            (self.notify)(&committed);
        }
        self.mirror_pump_out()?;
        self.mirror_changed();
        Ok(Some(inbound))
    }
}

// The commands.

/// Where the files folder stands.
pub fn mirror_status(session: &Session) -> Reply<MirrorStatus> {
    session.with_running(|vault, running| status_of(vault, running.as_ref()))
}

/// Turns the files folder on for the open vault, at `folder` or, with `None`, at `files/` inside
/// the vault. Every object is then written (the worker does it; a shell without one calls
/// [`mirror_sync_now`]). A folder other than the default is remembered in the app's configuration
/// folder, never in the vault.
pub fn mirror_enable(session: &Session, folder: Option<String>) -> Reply<MirrorStatus> {
    let status = session.with_running(|vault, running| {
        if let Some(file) = override_file(session.config_dir.as_deref(), vault.id()) {
            match &folder {
                Some(path) => {
                    if let Some(parent) = file.parent() {
                        std::fs::create_dir_all(parent)
                            .map_err(|e| storage("cannot save the folder setting", e))?;
                    }
                    std::fs::write(&file, format!("{path}\n"))
                        .map_err(|e| storage("cannot save the folder setting", e))?;
                }
                None => {
                    if file.exists() {
                        std::fs::remove_file(&file)
                            .map_err(|e| storage("cannot clear the folder setting", e))?;
                    }
                }
            }
        } else if folder.is_some() {
            return Err(IpcError::new(
                IpcErrorKind::Rule,
                "this shell keeps no settings, so only the default folder can be used",
            ));
        }
        let started = start(session.config_dir.as_deref(), vault)?;
        vault.mirror_meta_set(FOLDER_ENABLED, Some("1"))?;
        started.engine.request_full_rewrite(vault)?;
        *running = Some(started);
        tracing::info!(vault = %vault.id(), "files folder enabled");
        status_of(vault, running.as_ref())
    })?;
    session.wake_mirror();
    (session.mirror_notify)(&status);
    Ok(status)
}

/// Turns the files folder off. The files stay where they are; the bookkeeping stays too, so
/// turning it on again picks up where it left off.
pub fn mirror_disable(session: &Session) -> Reply<MirrorStatus> {
    let status = session.with_running(|vault, running| {
        vault.mirror_meta_set(FOLDER_ENABLED, None)?;
        *running = None;
        tracing::info!(vault = %vault.id(), "files folder disabled");
        status_of(vault, None)
    })?;
    (session.mirror_notify)(&status);
    Ok(status)
}

/// Syncs both ways now: writes what the vault changed, reads what the folder changed (from a
/// rescan, so no watcher is needed), and writes back what that asked for.
pub fn mirror_sync_now(session: &Session) -> Reply<MirrorSync> {
    let mut sync = MirrorSync::default();
    let events = session.with_engine(|vault, running| {
        sync.add_out(&running.engine.pump_out(vault)?);
        Ok(running.engine.rescan(vault)?)
    })?;
    if let Some(inbound) = session.mirror_pump_in(events)? {
        sync.add_in(&inbound);
    }
    if let Some(out) = session.mirror_pump_out()? {
        sync.add_out(&out);
    }
    session.mirror_changed();
    Ok(sync)
}

/// The Conflicts inbox: every entry, or only those not yet resolved.
pub fn conflicts_list(session: &Session, open_only: bool) -> Reply<Vec<Conflict>> {
    session.with(|vault| vault.conflicts(open_only))
}

/// Marks an inbox entry resolved, with how (`kept_app`, `kept_file`, `dismissed`, …). A
/// bulk-change hold is decided with [`mirror_hold_decide`] instead. Returns `false` if the entry
/// was already resolved.
pub fn conflict_resolve(session: &Session, id: Id, resolution: String) -> Reply<bool> {
    let resolved = session.transact(|txn| {
        let conflict = txn.conflict(id)?;
        if conflict.kind == ConflictKind::BulkChangeHold {
            return Err(dbme_store::StoreError::Rule(
                "a hold is decided, not resolved: use mirror_hold_decide".into(),
            ));
        }
        txn.conflict_resolve(id, &resolution)
    })?;
    session.mirror_changed();
    Ok(resolved)
}

/// Decides a bulk-change hold: applies the held changes, or writes the held files again from the
/// vault. Returns what that did.
pub fn mirror_hold_decide(session: &Session, id: Id, choice: HoldChoice) -> Reply<MirrorSync> {
    let mut sync = MirrorSync::default();
    let (inbound, committed) = session.with_engine(|vault, running| {
        let before = latest_seq(vault)?;
        let inbound = running.engine.hold_decide(vault, id, choice.into())?;
        let committed = inbound_committed(vault, before, &inbound)?;
        Ok((inbound, committed))
    })?;
    sync.add_in(&inbound);
    if let Some(committed) = committed {
        (session.notify)(&committed);
    }
    if let Some(out) = session.mirror_pump_out()? {
        sync.add_out(&out);
    }
    session.mirror_changed();
    Ok(sync)
}

/// Writes the export of the open vault into `dir` (FORMAT.md §7): the reader files, and with
/// `full` the op log, the vault's facts and the blobs too.
pub fn export_run(session: &Session, dir: String, full: bool) -> Reply<ExportReport> {
    let vault = session.lock();
    let vault = vault
        .as_ref()
        .ok_or_else(|| IpcError::new(IpcErrorKind::NoVault, "no vault is open"))?;
    Ok(dbme_export::export(vault, Path::new(&dir), full)?)
}

// The worker.

/// What wakes the worker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Wake {
    /// The session committed a change: write the dirty objects.
    Changed,
    /// Stop.
    Stop,
}

/// The thread that keeps the files folder in step with the vault while the app runs. Dropping it
/// stops the thread.
pub struct MirrorWorker {
    tx: Sender<Wake>,
    thread: Option<JoinHandle<()>>,
}

/// Starts the worker for `session`: after each commit it writes the dirty objects, once the burst
/// has settled, and it watches the folder the session has enabled, feeding the events in. A session
/// has one worker; a second call replaces the first's wake-up channel.
pub fn spawn_mirror_worker(session: Arc<Session>) -> MirrorWorker {
    let (tx, rx) = mpsc::channel();
    *session.lock_waker() = Some(tx.clone());
    let thread = std::thread::Builder::new()
        .name("dbme-mirror".into())
        .spawn(move || run_worker(&session, &rx))
        .ok();
    MirrorWorker { tx, thread }
}

impl MirrorWorker {
    /// Wakes the worker, as a commit does.
    pub fn wake(&self) {
        let _ = self.tx.send(Wake::Changed);
    }
}

impl Drop for MirrorWorker {
    fn drop(&mut self) {
        let _ = self.tx.send(Wake::Stop);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl std::fmt::Debug for MirrorWorker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MirrorWorker").finish_non_exhaustive()
    }
}

fn run_worker(session: &Session, rx: &Receiver<Wake>) {
    // The watcher and the folder it was made for, as the session names it: the watcher's own
    // root is canonical, so it cannot be compared with the session's path.
    let mut watched: Option<(PathBuf, NotifyWatcher)> = None;
    loop {
        match rx.recv_timeout(DEFAULT_DEBOUNCE) {
            Ok(Wake::Stop) | Err(RecvTimeoutError::Disconnected) => break,
            Ok(Wake::Changed) => {
                // Let a burst of commits land, then write once.
                std::thread::sleep(DEFAULT_DEBOUNCE);
                loop {
                    match rx.try_recv() {
                        Ok(Wake::Changed) => {}
                        Ok(Wake::Stop) => return,
                        Err(_) => break,
                    }
                }
                if let Err(error) = session.mirror_pump_out() {
                    tracing::warn!(error = %error.message, "files folder: writing failed");
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
        }
        // The watcher follows the folder the session has enabled.
        let folder = session.mirror_folder();
        if watched.as_ref().map(|(root, _)| root) != folder.as_ref() {
            watched = folder.and_then(|root| match NotifyWatcher::new(&root, DEFAULT_DEBOUNCE) {
                Ok(watcher) => Some((root, watcher)),
                Err(error) => {
                    tracing::warn!(%error, "files folder: cannot watch it; changes are read on sync");
                    None
                }
            });
        }
        if let Some((_, watcher)) = watched.as_mut() {
            for error in watcher.take_errors() {
                tracing::warn!(error, "files folder: watcher");
            }
            let events = watcher.poll_timeout(Duration::ZERO);
            if !events.is_empty()
                && let Err(error) = session.mirror_pump_in(events)
            {
                tracing::warn!(error = %error.message, "files folder: reading failed");
            }
        }
    }
}
