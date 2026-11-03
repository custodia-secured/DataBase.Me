// SPDX-License-Identifier: AGPL-3.0-or-later
//! `dbme mirror-check` (M5 Decision 23): the files folder's bookkeeping read the way `verify`
//! reads the vault. It runs on a closed vault (an open one is `StoreError::Locked`), changes
//! nothing, and tells inconsistencies, which are problems, from pending work, which is counted.

use std::path::Path;
use std::sync::Arc;

use serde::Serialize;

use dbme_core::Clock;
use dbme_store::{FULL_REWRITE_NEEDED, MirrorFileState, StoreError, Vault};

use crate::engine::{FileEvent, Mirror, has_file};
use crate::fs::{Fs, StdFs};
use crate::{MirrorError, Result};

/// What a rescan of the folder would hand `pump_in`: changes made while nothing was watching.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Drift {
    /// Files at paths no row has.
    pub created: u64,
    /// Files whose size or modification time differs from their row.
    pub modified: u64,
    /// Rows whose file is gone.
    pub removed: u64,
    /// Folders that appeared.
    pub folders: u64,
}

/// The result of a mirror check.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct MirrorCheckReport {
    /// Rows of `mirror_state`.
    pub rows: u64,
    /// Rows per state.
    pub synced: u64,
    /// Rows waiting for a write that keeps failing.
    pub pending: u64,
    /// Rows held by a parse error or a bulk-change hold.
    pub held: u64,
    /// Rows of objects with no file of their own.
    pub excluded: u64,
    /// Rows of objects in the Trash.
    pub trashed: u64,
    /// Objects waiting to be written to their files.
    pub dirty: u64,
    /// Entries of the Conflicts inbox not yet resolved.
    pub open_conflicts: u64,
    /// Whether every file is due to be written again.
    pub full_rewrite_needed: bool,
    /// What the folder holds that the rows do not know, when a folder was given.
    pub drift: Option<Drift>,
    /// Everything inconsistent, in the order found. Empty for healthy bookkeeping.
    pub problems: Vec<String>,
}

impl MirrorCheckReport {
    /// `true` if nothing is inconsistent. Pending work is not a problem.
    pub fn is_ok(&self) -> bool {
        self.problems.is_empty()
    }
}

/// Checks `vault`'s files-folder bookkeeping, and, given the folder, counts its drift. Problems are
/// reported, not returned as errors; an error means the check itself could not run.
pub fn check(
    vault: &Vault,
    folder: Option<&Path>,
    clock: Arc<dyn Clock>,
) -> Result<MirrorCheckReport> {
    let read = vault.read();
    let mut report = MirrorCheckReport::default();
    let states = vault.mirror_states(None)?;
    report.rows = states.len() as u64;
    for state in &states {
        match state.state {
            MirrorFileState::Synced => report.synced += 1,
            MirrorFileState::Pending => report.pending += 1,
            MirrorFileState::Held => report.held += 1,
            MirrorFileState::Excluded => report.excluded += 1,
            MirrorFileState::Trashed => report.trashed += 1,
        }
        match read.object(state.object_id) {
            Ok(object) => {
                let in_trash = object.deleted_at.is_some();
                if in_trash && state.state != MirrorFileState::Trashed {
                    report.problems.push(format!(
                        "object {} is in the Trash but its file row is {} at {}",
                        state.object_id,
                        state.state.as_str(),
                        state.rel_path
                    ));
                } else if !in_trash && state.state == MirrorFileState::Trashed {
                    report.problems.push(format!(
                        "object {} is live but its file row is trashed at {}",
                        state.object_id, state.rel_path
                    ));
                }
            }
            Err(StoreError::NotFound { .. }) => report.problems.push(format!(
                "mirror_state row at {} names object {}, which does not exist",
                state.rel_path, state.object_id
            )),
            Err(error) => return Err(error.into()),
        }
    }

    let conflicts = vault.conflicts(true)?;
    report.open_conflicts = conflicts.len() as u64;
    for conflict in &conflicts {
        if let Some(id) = conflict.object_id {
            match read.object(id) {
                Ok(_) => {}
                Err(StoreError::NotFound { .. }) => report.problems.push(format!(
                    "open conflict {} ({}) names object {id}, which does not exist",
                    conflict.id,
                    conflict.kind.as_str()
                )),
                Err(error) => return Err(error.into()),
            }
        }
    }

    report.dirty = vault.mirror_dirty_count()?;
    report.full_rewrite_needed = vault.mirror_meta_get(FULL_REWRITE_NEEDED)?.is_some();

    if let Some(folder) = folder {
        let fs = StdFs::new(folder)
            .map_err(|e| MirrorError::io("opening", &folder.display().to_string(), e))?;
        let mut drift = Drift::default();
        for state in states.iter().filter(|s| has_file(s)) {
            let present = fs
                .stat(&state.rel_path)
                .map_err(|e| MirrorError::io("reading", &state.rel_path, e))?
                .is_some_and(|stat| !stat.is_dir);
            if !present {
                drift.removed += 1;
            }
        }
        for event in Mirror::new(fs, clock).rescan(vault)? {
            match event {
                FileEvent::Created(_) => drift.created += 1,
                FileEvent::Modified(_) => drift.modified += 1,
                FileEvent::DirCreated(_) => drift.folders += 1,
                // Counted above from the rows, so a missing file is not counted twice.
                FileEvent::Removed(_) | FileEvent::Renamed { .. } => {}
            }
        }
        report.drift = Some(drift);
    }
    Ok(report)
}
