// SPDX-License-Identifier: AGPL-3.0-or-later
//! Applying an editor's changes to one object's blocks (ADR-0016).
//!
//! The editor works out what changed since its last save and sends the changes in an order that
//! keeps every step valid: creates, moves and updates in document order, so a parent is in place
//! before its children arrive, then deletes, then kind changes that take away the right to have
//! children (by then the block has none). This module applies them as one transaction and does not
//! reorder them.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use dbme_core::Id;
use dbme_store::{NewBlock, Position, StoreError, Txn};

use crate::LONG_SORT_KEY;

/// One change to an object's blocks.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum BlockChange {
    /// A new block, with the ID the editor gave it, under `parent` (`None`: the top level), just
    /// after sibling `after` (`None`: first).
    Create {
        /// The block's ID.
        id: Id,
        /// Its parent block.
        parent: Option<Id>,
        /// The sibling it follows.
        after: Option<Id>,
        /// Its kind.
        kind: String,
        /// Its content.
        content: Value,
    },
    /// A block's new kind and content.
    Update {
        /// The block.
        id: Id,
        /// Its kind.
        kind: String,
        /// Its content.
        content: Value,
    },
    /// A block's new place.
    Move {
        /// The block.
        id: Id,
        /// Its new parent block.
        parent: Option<Id>,
        /// The sibling it now follows.
        after: Option<Id>,
    },
    /// A deleted block, with the blocks below it.
    Delete {
        /// The block.
        id: Id,
    },
}

fn position(after: Option<Id>) -> Position {
    after.map_or(Position::First, Position::After)
}

/// Fails unless block `id` belongs to `object_id`.
fn check_owner(txn: &Txn<'_>, object_id: Id, id: Id) -> Result<(), StoreError> {
    if txn.read().block(id)?.object_id == object_id {
        Ok(())
    } else {
        Err(StoreError::Rule(format!(
            "block {id} does not belong to object {object_id}"
        )))
    }
}

pub(crate) fn apply(
    txn: &mut Txn<'_>,
    object_id: Id,
    changes: Vec<BlockChange>,
) -> Result<(), StoreError> {
    // Sibling groups where a create or move made a long key.
    let mut long: Vec<Option<Id>> = Vec::new();
    let mut placed = |txn: &Txn<'_>, id: Id, parent: Option<Id>| -> Result<(), StoreError> {
        if txn.read().block(id)?.sort_key.as_str().len() > LONG_SORT_KEY && !long.contains(&parent)
        {
            long.push(parent);
        }
        Ok(())
    };
    for change in changes {
        match change {
            BlockChange::Create {
                id,
                parent,
                after,
                kind,
                content,
            } => {
                txn.create_block(NewBlock {
                    id: Some(id),
                    object_id,
                    parent_block_id: parent,
                    position: position(after),
                    kind,
                    content,
                })?;
                placed(txn, id, parent)?;
            }
            BlockChange::Update { id, kind, content } => {
                check_owner(txn, object_id, id)?;
                txn.update_block(id, |block| {
                    block.kind = kind;
                    block.content_json = content;
                })?;
            }
            BlockChange::Move { id, parent, after } => {
                check_owner(txn, object_id, id)?;
                txn.move_block(id, parent, position(after))?;
                placed(txn, id, parent)?;
            }
            BlockChange::Delete { id } => {
                check_owner(txn, object_id, id)?;
                txn.delete_block(id)?;
            }
        }
    }
    for parent in long {
        txn.rebalance_blocks(object_id, parent)?;
    }
    Ok(())
}
