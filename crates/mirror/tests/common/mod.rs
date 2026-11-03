// SPDX-License-Identifier: AGPL-3.0-or-later
//! Helpers shared by the mirror's integration tests: a vault, an engine over the in-memory file
//! system, and a few shapes of object.

#![allow(dead_code, reason = "each test binary uses a different subset")]

use std::path::PathBuf;
use std::sync::Arc;

use dbme_core::{Id, ManualClock};
use dbme_mirror::{MemFs, Mirror};
use dbme_store::{Identity, NewBlock, NewObject, Position, Vault, VaultOptions};
use serde_json::{Value, json};

/// 2026-10-25T08:21:11Z.
pub const START: i64 = 1_761_380_471_000;

pub struct World {
    pub dir: tempfile::TempDir,
    pub path: PathBuf,
    pub clock: Arc<ManualClock>,
    pub vault: Vault,
    pub mirror: Mirror<MemFs>,
}

pub fn identity() -> Identity {
    Identity {
        actor_id: Id::from_parts(1, 1),
        device_id: Id::from_parts(1, 2),
    }
}

pub fn new_world() -> World {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Test.dbme");
    let clock = Arc::new(ManualClock::new(START));
    let vault = Vault::create(
        &path,
        VaultOptions {
            identity: identity(),
            clock: clock.clone(),
        },
    )
    .unwrap();
    let mirror = Mirror::new(MemFs::new(), clock.clone());
    World {
        dir,
        path,
        clock,
        vault,
        mirror,
    }
}

impl World {
    pub fn tick(&self) {
        self.clock.advance(1);
    }

    pub fn object(&mut self, title: &str, parent: Option<Id>) -> Id {
        self.vault
            .transact(|txn| {
                txn.create_object(NewObject {
                    title: title.to_owned(),
                    parent_id: parent,
                    ..NewObject::default()
                })
            })
            .unwrap()
            .0
    }

    pub fn paragraph(&mut self, object_id: Id, text: &str) -> Id {
        self.block(object_id, "paragraph", json!({"inline": [{"text": text}]}))
    }

    pub fn block(&mut self, object_id: Id, kind: &str, content: Value) -> Id {
        self.vault
            .transact(|txn| {
                txn.create_block(NewBlock {
                    id: None,
                    object_id,
                    parent_block_id: None,
                    position: Position::Last,
                    kind: kind.to_owned(),
                    content,
                })
            })
            .unwrap()
            .0
    }

    pub fn retitle(&mut self, id: Id, title: &str) {
        self.vault
            .transact(|txn| txn.update_object(id, |o| o.title = title.to_owned()))
            .unwrap();
    }

    pub fn files(&self) -> Vec<String> {
        self.mirror.fs().paths()
    }

    pub fn text(&self, rel_path: &str) -> String {
        self.mirror
            .fs()
            .text(rel_path)
            .unwrap_or_else(|| panic!("no file at {rel_path}; have {:?}", self.files()))
    }

    pub fn pump_out(&mut self) -> dbme_mirror::Outbound {
        self.mirror.pump_out(&mut self.vault).unwrap()
    }
}
