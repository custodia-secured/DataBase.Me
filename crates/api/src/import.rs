// SPDX-License-Identifier: AGPL-3.0-or-later
//! Importing (ADR-0026, M6): the plan as a dry run, the apply, and an object's provenance. The
//! shell opens the Import dialog with the paths the person dropped on the window or typed; the
//! dialog shows the plan and applies it. Every transaction the apply makes reaches the UI as
//! `vault-changed`, as one record for the run.

use dbme_core::Id;
use dbme_core::model::ProvenanceRow;
use dbme_ingest::{ImportOptions, ImportPlan, ImportRun, Importers, IngestError};
use dbme_store::Committed;

use crate::mirror::latest_seq;
use crate::{IpcError, IpcErrorKind, Reply, Session};

impl From<IngestError> for IpcError {
    fn from(error: IngestError) -> Self {
        let kind = match error {
            IngestError::Store(store) => return IpcError::from(store),
            IngestError::Core(core) => return IpcError::from(core),
            IngestError::NotASource(_) | IngestError::TooLarge { .. } => IpcErrorKind::BadPath,
            IngestError::NoSuchType(_) | IngestError::NoSuchParent(_) => IpcErrorKind::NotFound,
            IngestError::Changed(_) | IngestError::Unreadable { .. } => IpcErrorKind::Invalid,
            IngestError::Io { .. } | IngestError::Mirror(_) => IpcErrorKind::Storage,
        };
        Self::new(kind, error.to_string())
    }
}

/// The dry run: what importing `sources` would do, and nothing written (M6 Decision 7). The
/// paths are as the person gave them; the plan carries them canonical.
pub fn import_plan(
    session: &Session,
    sources: Vec<String>,
    options: ImportOptions,
) -> Reply<ImportPlan> {
    let paths: Vec<std::path::PathBuf> = sources.into_iter().map(Into::into).collect();
    let vault = session.lock();
    let vault = vault
        .as_ref()
        .ok_or_else(|| IpcError::new(IpcErrorKind::NoVault, "no vault is open"))?;
    Ok(dbme_ingest::plan(vault, &paths, options, &Importers::v0())?)
}

/// Writes a plan to the open vault. The UI hears one `vault-changed` for the whole run, which
/// covers every undo step it took; the files folder's worker writes the new objects out.
pub fn import_apply(session: &Session, plan: ImportPlan) -> Reply<ImportRun> {
    let (run, committed) = {
        let mut vault = session.lock();
        let vault = vault
            .as_mut()
            .ok_or_else(|| IpcError::new(IpcErrorKind::NoVault, "no vault is open"))?;
        let before = latest_seq(vault)?;
        let run = dbme_ingest::apply(vault, &plan)?;
        (run.clone(), run_committed(vault, before, &run)?)
    };
    if let Some(committed) = committed {
        session.notify_committed(&committed);
    }
    Ok(run)
}

/// Where an object came from: its provenance rows, oldest first.
pub fn provenance_list(session: &Session, object_id: Id) -> Reply<Vec<ProvenanceRow>> {
    session.with(|vault| vault.read().provenance_of(object_id))
}

/// One `vault-changed` record for a run: the ops it appended, as one step, naming the parent
/// and every object made.
fn run_committed(
    vault: &dbme_store::Vault,
    before: i64,
    run: &ImportRun,
) -> Reply<Option<Committed>> {
    let newest = vault.read().history(1)?;
    let Some(last) = newest.first().filter(|r| r.seq > before) else {
        return Ok(None);
    };
    let mut changed: Vec<Id> = run.created.clone();
    changed.push(run.parent);
    changed.extend(run.attached.iter().copied());
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

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::{Arc, Mutex};

    use dbme_core::SystemClock;
    use dbme_ingest::ImportOptions;
    use dbme_store::{Identity, VaultOptions};

    use super::*;
    use crate::{IpcErrorKind, vault_create};

    #[test]
    fn a_plan_an_apply_and_the_provenance_through_the_session() {
        let dir = tempfile::tempdir().unwrap();
        let seen: Arc<Mutex<Vec<Committed>>> = Arc::new(Mutex::new(Vec::new()));
        let notified = Arc::clone(&seen);
        let session = Session::new(move |c| notified.lock().unwrap().push(c.clone()))
            .with_config_dir(dir.path().join("config"));
        assert!(matches!(
            import_plan(&session, vec!["x".into()], ImportOptions::default()),
            Err(IpcError {
                kind: IpcErrorKind::NoVault,
                ..
            })
        ));
        let clock = Arc::new(SystemClock);
        let options = VaultOptions {
            identity: Identity::generate(clock.as_ref()).unwrap(),
            clock,
        };
        vault_create(
            &session,
            &dir.path().join("Test.dbme").to_string_lossy(),
            options,
        )
        .unwrap();
        let notes = dir.path().join("notes.txt");
        fs::write(&notes, b"hello").unwrap();

        let plan = import_plan(
            &session,
            vec![notes.to_string_lossy().into_owned()],
            ImportOptions::default(),
        )
        .unwrap();
        assert_eq!(plan.entries.len(), 1);
        let before = seen.lock().unwrap().len();
        let run = import_apply(&session, plan).unwrap();
        assert_eq!(run.created.len(), 1);
        let after = seen.lock().unwrap();
        assert_eq!(after.len(), before + 1, "one vault-changed for the run");
        let committed = after.last().unwrap();
        assert!(committed.changed.contains(&run.created[0]));
        assert!(committed.changed.contains(&run.receipt.unwrap()));
        drop(after);

        let provenance = provenance_list(&session, run.created[0]).unwrap();
        assert_eq!(provenance.len(), 1);
        assert_eq!(provenance[0].source_kind, "import:file");
        assert_eq!(provenance[0].import_run_id, Some(run.run_id));

        // A dead parent is NotFound; a path that is nothing is a refusal in the plan, not an error.
        let bad = ImportOptions {
            parent: Some(Id::from_parts(9, 9)),
            ..ImportOptions::default()
        };
        assert!(matches!(
            import_plan(&session, vec![notes.to_string_lossy().into_owned()], bad),
            Err(IpcError {
                kind: IpcErrorKind::NotFound,
                ..
            })
        ));
        let plan = import_plan(
            &session,
            vec!["nowhere.txt".into()],
            ImportOptions::default(),
        )
        .unwrap();
        assert_eq!(plan.refused.len(), 1);
    }
}
