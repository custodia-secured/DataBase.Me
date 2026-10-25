// SPDX-License-Identifier: AGPL-3.0-or-later
//! Who is making changes, and on which device. Every op records both.
//!
//! Until device pairing and member keys exist (M13, M15), both are random IDs made on first launch
//! and kept in the app's configuration folder, outside any vault, so every vault this install opens
//! records the same author.

use std::fs;
use std::io::{ErrorKind, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};

use dbme_core::{Clock, CoreError, Id, IdGenerator};

use crate::{Result, StoreError};

/// The file in the configuration folder.
pub const IDENTITY_FILE: &str = "identity.json";

/// The author and device recorded in every op.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Identity {
    /// The person.
    pub actor_id: Id,
    /// This installation.
    pub device_id: Id,
}

impl Identity {
    /// A new identity.
    pub fn generate(clock: &dyn Clock) -> Result<Self> {
        let ids = IdGenerator::new();
        let now = clock.now_ms();
        Ok(Self {
            actor_id: ids.next(now)?,
            device_id: ids.next(now)?,
        })
    }

    /// The identity kept in `folder`, made and saved there on first use.
    pub fn load_or_create(folder: &Path, clock: &dyn Clock) -> Result<Self> {
        let path = folder.join(IDENTITY_FILE);
        match fs::read_to_string(&path) {
            Ok(text) => Ok(serde_json::from_str(&text).map_err(CoreError::from)?),
            Err(e) if e.kind() == ErrorKind::NotFound => {
                let identity = Self::generate(clock)?;
                fs::create_dir_all(folder).map_err(|e| StoreError::io("creating", folder, e))?;
                let mut temp = tempfile::NamedTempFile::new_in(folder)
                    .map_err(|e| StoreError::io("writing to", folder, e))?;
                let text = serde_json::to_string_pretty(&identity).map_err(CoreError::from)?;
                temp.write_all(text.as_bytes())
                    .map_err(|e| StoreError::io("writing", &path, e))?;
                temp.as_file()
                    .sync_all()
                    .map_err(|e| StoreError::io("writing", &path, e))?;
                match temp.persist_noclobber(&path) {
                    Ok(_) => Ok(identity),
                    // Another process made one first: use it.
                    Err(e) if e.error.kind() == ErrorKind::AlreadyExists => {
                        Self::load_or_create(folder, clock)
                    }
                    Err(e) => Err(StoreError::io("writing", path, e.error)),
                }
            }
            Err(e) => Err(StoreError::io("reading", path, e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dbme_core::ManualClock;

    #[test]
    fn made_once_then_reused() {
        let dir = tempfile::tempdir().unwrap();
        let clock = ManualClock::new(1_761_380_471_000);
        let folder = dir.path().join("config");
        let first = Identity::load_or_create(&folder, &clock).unwrap();
        assert_ne!(first.actor_id, first.device_id);
        assert_eq!(Identity::load_or_create(&folder, &clock).unwrap(), first);

        fs::write(folder.join(IDENTITY_FILE), "{").unwrap();
        assert!(matches!(
            Identity::load_or_create(&folder, &clock),
            Err(StoreError::Core(_))
        ));
        let not_a_folder = dir.path().join("file");
        fs::write(&not_a_folder, "").unwrap();
        assert!(matches!(
            Identity::load_or_create(&not_a_folder, &clock),
            Err(StoreError::Io { .. })
        ));
    }
}
