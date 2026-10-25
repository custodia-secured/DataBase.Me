// SPDX-License-Identifier: AGPL-3.0-or-later
//! The hash chain over the op log (ADR-0012, FORMAT.md §5.3).
//!
//! Each op's `hash` is the BLAKE3 hash of a fixed byte encoding of the op and of the previous op's
//! `hash`, so changing, removing or reordering any op breaks every hash after it. The encoding
//! hashes the stored text of each column exactly as stored, so a reader never has to re-encode JSON
//! the same way to check it.
//!
//! The bytes are, in order:
//!
//! 1. the ASCII text `dbme-op-1` followed by one zero byte;
//! 2. for each of `prev_hash`, `op_id`, `group_id`, `undo_of`, `actor_id`, `device_id`: the
//!    column as text (see 3);
//! 3. a text column is a single `0x00` byte when NULL, otherwise `0x01`, its UTF-8 length as an
//!    unsigned 64-bit big-endian integer, then its UTF-8 bytes;
//! 4. `ts` as a signed 64-bit big-endian integer;
//! 5. `kind`, `target_id` and `payload_json`, each as text (see 3).
//!
//! `seq` and `signature` are not hashed: `seq` is a local row number, and the signature (M15) signs
//! the hash.

use serde::{Deserialize, Serialize};

use crate::hash::ContentHasher;
use crate::op::Op;
use crate::{ContentHash, CoreError, Id};

/// The prefix that separates op hashes from every other use of BLAKE3 in a vault.
pub const DOMAIN: &[u8] = b"dbme-op-1\0";

/// The hashed content of one op-log entry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpEntry {
    /// The op's own ID.
    pub op_id: Id,
    /// The undo step the op belongs to: every op of one transaction shares it.
    pub group_id: Id,
    /// For an undo or redo, the step it reverses.
    pub undo_of: Option<Id>,
    /// Who made the change.
    pub actor_id: Id,
    /// On which device.
    pub device_id: Id,
    /// When, in milliseconds since the Unix epoch.
    pub ts: i64,
    /// The op kind, `<entity>.<action>`.
    pub kind: String,
    /// The changed row, or its object for `object_type` and `attachment` rows.
    pub target_id: Id,
    /// The payload as stored.
    pub payload_json: String,
}

fn text(hasher: &mut ContentHasher, value: Option<&str>) {
    match value {
        None => {
            hasher.update(&[0]);
        }
        Some(text) => {
            hasher.update(&[1]);
            hasher.update(&(text.len() as u64).to_be_bytes());
            hasher.update(text.as_bytes());
        }
    }
}

impl OpEntry {
    /// The entry's hash when it follows `prev_hash` (`None` for the first op of a vault).
    pub fn hash(&self, prev_hash: Option<&ContentHash>) -> ContentHash {
        let mut hasher = ContentHasher::new();
        hasher.update(DOMAIN);
        text(&mut hasher, prev_hash.map(ContentHash::as_str));
        text(&mut hasher, Some(&self.op_id.to_string()));
        text(&mut hasher, Some(&self.group_id.to_string()));
        text(
            &mut hasher,
            self.undo_of.map(|id| id.to_string()).as_deref(),
        );
        text(&mut hasher, Some(&self.actor_id.to_string()));
        text(&mut hasher, Some(&self.device_id.to_string()));
        hasher.update(&self.ts.to_be_bytes());
        text(&mut hasher, Some(&self.kind));
        text(&mut hasher, Some(&self.target_id.to_string()));
        text(&mut hasher, Some(&self.payload_json));
        hasher.finalize()
    }

    /// The op this entry records.
    pub fn op(&self) -> Result<Op, CoreError> {
        Op::decode(&self.kind, &self.payload_json)
    }
}

/// A row of the `ops` table.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpRecord {
    /// Position in this vault's log.
    pub seq: i64,
    /// The hashed content.
    #[serde(flatten)]
    pub entry: OpEntry,
    /// The previous op's hash; `None` for the first op.
    pub prev_hash: Option<ContentHash>,
    /// This op's hash.
    pub hash: ContentHash,
    /// Reserved for signed ops in shared spaces (M15).
    pub signature: Option<String>,
}

/// Why a log failed verification.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("op {seq}: {reason}")]
pub struct ChainBreak {
    /// The first op that does not fit.
    pub seq: i64,
    /// What is wrong with it.
    pub reason: String,
}

/// Checks a log one op at a time, in `seq` order.
#[derive(Debug, Default)]
pub struct ChainVerifier {
    previous: Option<(i64, ContentHash)>,
    count: u64,
}

impl ChainVerifier {
    /// A verifier that has seen no ops.
    pub fn new() -> Self {
        Self::default()
    }

    /// Checks the next op: it links to the previous op's hash, its own hash is right, its `seq`
    /// increases and its op decodes.
    pub fn check(&mut self, record: &OpRecord) -> Result<(), ChainBreak> {
        let fail = |reason: String| {
            Err(ChainBreak {
                seq: record.seq,
                reason,
            })
        };
        let expected_prev = self.previous.as_ref().map(|(_, hash)| hash);
        if record.prev_hash.as_ref() != expected_prev {
            return fail(match expected_prev {
                Some(hash) => format!("prev_hash should be {hash}"),
                None => "the first op must not have a prev_hash".to_owned(),
            });
        }
        if let Some((seq, _)) = &self.previous
            && record.seq <= *seq
        {
            return fail(format!("seq does not increase after {seq}"));
        }
        let expected = record.entry.hash(record.prev_hash.as_ref());
        if record.hash != expected {
            return fail(format!("hash should be {expected}"));
        }
        if let Err(error) = record.entry.op() {
            return fail(format!("payload does not decode: {error}"));
        }
        self.previous = Some((record.seq, record.hash.clone()));
        self.count += 1;
        Ok(())
    }

    /// How many ops have passed.
    pub fn count(&self) -> u64 {
        self.count
    }

    /// The hash of the last op that passed: the head of the chain.
    pub fn head(&self) -> Option<&ContentHash> {
        self.previous.as_ref().map(|(_, hash)| hash)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::tests::{id, object};
    use crate::op::Row;

    fn entry(n: u128) -> OpEntry {
        let op = Op::create(Row::Object(object(n)));
        OpEntry {
            op_id: id(100 + n),
            group_id: id(200),
            undo_of: None,
            actor_id: id(300),
            device_id: id(400),
            ts: 1_761_380_471_000,
            kind: op.kind().to_string(),
            target_id: op.key().target_id(),
            payload_json: op.payload_json().unwrap(),
        }
    }

    fn chain(n: u128) -> Vec<OpRecord> {
        let mut records: Vec<OpRecord> = Vec::new();
        for i in 1..=n {
            let entry = entry(i);
            let prev_hash = records.last().map(|r| r.hash.clone());
            let hash = entry.hash(prev_hash.as_ref());
            records.push(OpRecord {
                seq: i64::try_from(i).unwrap(),
                entry,
                prev_hash,
                hash,
                signature: None,
            });
        }
        records
    }

    #[test]
    fn a_good_chain_verifies() {
        let mut verifier = ChainVerifier::new();
        for record in chain(5) {
            verifier.check(&record).unwrap();
        }
        assert_eq!(verifier.count(), 5);
        assert!(verifier.head().is_some());
    }

    #[test]
    fn every_hashed_column_matters() {
        let base = entry(1);
        let hash = base.hash(None);
        let variants = [
            OpEntry {
                op_id: id(9),
                ..base.clone()
            },
            OpEntry {
                group_id: id(9),
                ..base.clone()
            },
            OpEntry {
                undo_of: Some(id(9)),
                ..base.clone()
            },
            OpEntry {
                actor_id: id(9),
                ..base.clone()
            },
            OpEntry {
                device_id: id(9),
                ..base.clone()
            },
            OpEntry {
                ts: base.ts + 1,
                ..base.clone()
            },
            OpEntry {
                kind: "object.delete".into(),
                ..base.clone()
            },
            OpEntry {
                target_id: id(9),
                ..base.clone()
            },
            OpEntry {
                payload_json: format!("{} ", base.payload_json),
                ..base.clone()
            },
        ];
        for variant in variants {
            assert_ne!(variant.hash(None), hash, "{variant:?}");
        }
        assert_ne!(base.hash(Some(&ContentHash::of(b"x"))), hash);
    }

    #[test]
    fn the_encoding_is_pinned() {
        // Changing the encoding is a format change (FORMAT.md §5.3). This pins it.
        let entry = OpEntry {
            op_id: Id::parse("01JB3R7Q8Y5K3M2N4P6Q8R0S2T").unwrap(),
            group_id: Id::parse("01JB3R7Q8Y5K3M2N4P6Q8R0S2V").unwrap(),
            undo_of: None,
            actor_id: Id::parse("01JB3R7Q8Y5K3M2N4P6Q8R0S2W").unwrap(),
            device_id: Id::parse("01JB3R7Q8Y5K3M2N4P6Q8R0S2X").unwrap(),
            ts: 1_761_380_471_000,
            kind: "object.update".into(),
            target_id: Id::parse("01JB3R7Q8Y5K3M2N4P6Q8R0S2Y").unwrap(),
            payload_json:
                r#"{"id":"01JB3R7Q8Y5K3M2N4P6Q8R0S2Y","set":{"title":{"from":"a","to":"b"}}}"#
                    .into(),
        };
        let mut bytes = DOMAIN.to_vec();
        bytes.push(0);
        for text in ["01JB3R7Q8Y5K3M2N4P6Q8R0S2T", "01JB3R7Q8Y5K3M2N4P6Q8R0S2V"] {
            bytes.push(1);
            bytes.extend((text.len() as u64).to_be_bytes());
            bytes.extend(text.as_bytes());
        }
        bytes.push(0);
        for text in ["01JB3R7Q8Y5K3M2N4P6Q8R0S2W", "01JB3R7Q8Y5K3M2N4P6Q8R0S2X"] {
            bytes.push(1);
            bytes.extend((text.len() as u64).to_be_bytes());
            bytes.extend(text.as_bytes());
        }
        bytes.extend(1_761_380_471_000_i64.to_be_bytes());
        for text in [
            "object.update",
            "01JB3R7Q8Y5K3M2N4P6Q8R0S2Y",
            entry.payload_json.as_str(),
        ] {
            bytes.push(1);
            bytes.extend((text.len() as u64).to_be_bytes());
            bytes.extend(text.as_bytes());
        }
        assert_eq!(entry.hash(None), ContentHash::of(&bytes));
    }

    #[test]
    fn breaks_are_found_and_located() {
        let good = chain(3);

        let mut tampered = good.clone();
        tampered[1].entry.payload_json = tampered[1]
            .entry
            .payload_json
            .replace("Object 2", "Object X");
        let mut verifier = ChainVerifier::new();
        verifier.check(&tampered[0]).unwrap();
        let error = verifier.check(&tampered[1]).unwrap_err();
        assert_eq!(error.seq, 2);
        assert!(error.reason.contains("hash should be"), "{error}");

        let mut verifier = ChainVerifier::new();
        verifier.check(&good[0]).unwrap();
        assert!(
            verifier
                .check(&good[2])
                .unwrap_err()
                .reason
                .contains("prev_hash"),
            "a removed op is noticed"
        );

        let mut verifier = ChainVerifier::new();
        assert!(
            verifier
                .check(&good[1])
                .unwrap_err()
                .reason
                .contains("first op")
        );

        let mut reordered = good.clone();
        reordered[1].seq = 1;
        let mut verifier = ChainVerifier::new();
        verifier.check(&reordered[0]).unwrap();
        assert!(
            verifier
                .check(&reordered[1])
                .unwrap_err()
                .reason
                .contains("seq")
        );

        let mut undecodable = good[0].clone();
        undecodable.entry.payload_json = "{}".into();
        undecodable.hash = undecodable.entry.hash(None);
        assert!(
            ChainVerifier::new()
                .check(&undecodable)
                .unwrap_err()
                .reason
                .contains("decode")
        );
        assert!(
            format!(
                "{}",
                ChainBreak {
                    seq: 4,
                    reason: "x".into()
                }
            )
            .starts_with("op 4")
        );
    }

    #[test]
    fn records_serialise_flat() {
        let record = &chain(1)[0];
        let json = serde_json::to_value(record).unwrap();
        assert_eq!(json["kind"], "object.create");
        assert_eq!(serde_json::from_value::<OpRecord>(json).unwrap(), *record);
    }
}
