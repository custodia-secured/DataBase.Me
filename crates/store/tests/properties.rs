// SPDX-License-Identifier: AGPL-3.0-or-later
//! Property tests: random sequences of edits, undos and redos.
//!
//! Whatever the sequence, the vault must verify (the hash chain holds and replaying the log
//! reproduces every table), undo must never be refused within one session, undoing everything must
//! give back the starting state, and redoing everything must give back the final state.

#![allow(
    clippy::unwrap_used,
    reason = "test helpers outside #[test] functions may panic"
)]

mod common;

use dbme_core::Id;
use dbme_core::model::Props;
use dbme_store::{NewBlock, NewObject, NewRelation, Position, StoreError, Txn};
use proptest::prelude::*;
use serde_json::json;

use common::{new_vault, snapshot};

#[derive(Clone, Debug)]
enum Action {
    Create {
        parent: Option<usize>,
        first: bool,
    },
    Rename {
        target: usize,
        title: String,
    },
    SetProp {
        target: usize,
        value: Option<i64>,
    },
    Move {
        target: usize,
        parent: Option<usize>,
        first: bool,
    },
    Trash {
        target: usize,
    },
    Restore {
        target: usize,
    },
    Delete {
        target: usize,
    },
    AddBlock {
        target: usize,
    },
    Link {
        from: usize,
        to: usize,
    },
    Bulk {
        count: u8,
    },
    Undo,
    Redo,
}

fn action() -> impl Strategy<Value = Action> {
    let index = 0..16_usize;
    prop_oneof![
        3 => (proptest::option::of(index.clone()), any::<bool>()).prop_map(|(parent, first)| Action::Create { parent, first }),
        2 => (index.clone(), "[a-z ]{0,8}").prop_map(|(target, title)| Action::Rename { target, title }),
        1 => (index.clone(), proptest::option::of(-5_i64..5)).prop_map(|(target, value)| Action::SetProp { target, value }),
        2 => (index.clone(), proptest::option::of(index.clone()), any::<bool>())
            .prop_map(|(target, parent, first)| Action::Move { target, parent, first }),
        1 => index.clone().prop_map(|target| Action::Trash { target }),
        1 => index.clone().prop_map(|target| Action::Restore { target }),
        1 => index.clone().prop_map(|target| Action::Delete { target }),
        1 => index.clone().prop_map(|target| Action::AddBlock { target }),
        1 => (index.clone(), index).prop_map(|(from, to)| Action::Link { from, to }),
        1 => (1_u8..6).prop_map(|count| Action::Bulk { count }),
        2 => Just(Action::Undo),
        1 => Just(Action::Redo),
    ]
}

fn pick(ids: &[Id], index: usize) -> Option<Id> {
    (!ids.is_empty()).then(|| ids[index % ids.len()])
}

fn position(first: bool) -> Position {
    if first {
        Position::First
    } else {
        Position::Last
    }
}

/// Runs one edit. Edits that break a rule fail and change nothing, like they would in the app.
fn edit(txn: &mut Txn<'_>, ids: &[Id], action: &Action) -> Result<Vec<Id>, StoreError> {
    let target =
        |index: &usize| pick(ids, *index).ok_or_else(|| StoreError::Rule("no objects yet".into()));
    let mut created = Vec::new();
    match action {
        Action::Create { parent, first } => {
            let parent = parent.and_then(|p| pick(ids, p));
            created.push(txn.create_object(NewObject {
                title: "new".into(),
                parent_id: parent,
                position: position(*first),
                ..Default::default()
            })?);
        }
        Action::Rename { target: t, title } => {
            txn.update_object(target(t)?, |o| o.title.clone_from(title))?;
        }
        Action::SetProp { target: t, value } => {
            txn.update_object(target(t)?, |o| match value {
                Some(v) => {
                    o.props_json.insert("n".into(), json!(v));
                }
                None => {
                    o.props_json.remove("n");
                }
            })?;
        }
        Action::Move {
            target: t,
            parent,
            first,
        } => {
            txn.move_object(
                target(t)?,
                parent.and_then(|p| pick(ids, p)),
                position(*first),
            )?;
        }
        Action::Trash { target: t } => txn.trash_object(target(t)?)?,
        Action::Restore { target: t } => txn.restore_object(target(t)?)?,
        Action::Delete { target: t } => txn.delete_object(target(t)?)?,
        Action::AddBlock { target: t } => {
            txn.create_block(NewBlock {
                id: None,
                object_id: target(t)?,
                parent_block_id: None,
                position: Position::Last,
                kind: "paragraph".into(),
                content: json!({"inline": [{"text": "x"}]}),
            })?;
        }
        Action::Link { from, to } => {
            txn.create_relation(NewRelation {
                src_id: target(from)?,
                dst_id: target(to)?,
                rel_type: "links_to".into(),
                field_id: None,
                src_block_id: None,
                label: None,
                props: Props::new(),
            })?;
        }
        Action::Bulk { count } => {
            for _ in 0..*count {
                created.push(txn.create_object(NewObject {
                    title: "bulk".into(),
                    ..Default::default()
                })?);
            }
        }
        Action::Undo | Action::Redo => {}
    }
    Ok(created)
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 48, ..ProptestConfig::default() })]

    #[test]
    fn random_histories_verify_and_undo_round_trips(actions in proptest::collection::vec(action(), 1..40)) {
        let mut t = new_vault();
        let start = snapshot(&t.path);
        let mut ids: Vec<Id> = Vec::new();
        for action in &actions {
            t.tick();
            match action {
                Action::Undo => {
                    let undone = t.vault.undo();
                    prop_assert!(undone.is_ok(), "undo was refused: {:?}", undone);
                }
                Action::Redo => {
                    let redone = t.vault.redo();
                    prop_assert!(redone.is_ok(), "redo was refused: {:?}", redone);
                }
                _ => {
                    if let Ok((created, _)) = t.vault.transact(|txn| edit(txn, &ids, action)) {
                        ids.extend(created);
                    }
                }
            }
        }
        let report = t.vault.verify(false).unwrap();
        prop_assert!(report.is_ok(), "{:?}", report.problems);

        let end = snapshot(&t.path);
        let mut undone = 0;
        while t.vault.can_undo() {
            t.tick();
            prop_assert!(t.vault.undo().is_ok());
            undone += 1;
        }
        prop_assert_eq!(&snapshot(&t.path), &start, "undoing all {} steps gives back the start", undone);
        // The redo stack may also hold steps undone before the end; redo only the ones just undone.
        for _ in 0..undone {
            t.tick();
            prop_assert!(t.vault.redo().is_ok());
        }
        prop_assert_eq!(&snapshot(&t.path), &end);
        let report = t.vault.verify(false).unwrap();
        prop_assert!(report.is_ok(), "{:?}", report.problems);
    }
}
