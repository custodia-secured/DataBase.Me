// SPDX-License-Identifier: AGPL-3.0-or-later
//! Gate 1e (M6): the `provenance` entity's op shape against ADR-0012's rules, before Task 2 adds
//! it to `dbme-core`. The row is drafted here with the brief's columns; the probes check that
//! its payload is a whole row that round-trips through the stored JSON text, that `confidence`
//! survives as the number written, that the chain hashes an op of the new kind by the same bytes
//! as any other, and what `dbme-core` says about the kind today (nothing: Task 2 changes that).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "gate probes: a failure here is the finding"
)]

use dbme_core::chain::OpEntry;
use dbme_core::op::{Action, Entity, OpKind};
use dbme_core::{ContentHash, CoreError, Id};
use serde::{Deserialize, Serialize};

/// The row as planned (M6.md Design, brief §4): the columns FORMAT.md §3.4 will carry.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProvenanceRow {
    id: Id,
    object_id: Id,
    source_kind: String,
    source_uri: String,
    import_run_id: Id,
    original_blob: Option<ContentHash>,
    extractor: String,
    extractor_version: String,
    confidence: f64,
    accepted: bool,
    created_at: i64,
}

fn id(n: u8) -> Id {
    Id::parse(&format!(
        "01ARZ3NDEKTSV4RRFFQ69G5F{}V",
        char::from(b'A' + n)
    ))
    .unwrap()
}

fn row() -> ProvenanceRow {
    ProvenanceRow {
        id: id(0),
        object_id: id(1),
        source_kind: "import:csv".to_owned(),
        source_uri: "C:/people/team.csv#sheet=People".to_owned(),
        import_run_id: id(2),
        original_blob: Some(ContentHash::of(b"the file")),
        extractor: dbme_ingest::EXTRACTOR.to_owned(),
        extractor_version: dbme_ingest::EXTRACTOR_VERSION.to_owned(),
        confidence: 0.1,
        accepted: true,
        created_at: 1_762_128_000_000,
    }
}

fn entry(kind: &str, payload_json: String) -> OpEntry {
    OpEntry {
        op_id: id(3),
        group_id: id(4),
        undo_of: None,
        actor_id: id(5),
        device_id: id(6),
        ts: 1_762_128_000_000,
        kind: kind.to_owned(),
        target_id: id(1),
        payload_json,
    }
}

// Probe 1: the payload is the whole row and reads back equal; the stored text is what is hashed,
// so what matters is that the same row always writes the same text.
#[test]
fn the_payload_is_the_whole_row_and_round_trips() {
    let row = row();
    let json = serde_json::to_string(&row).unwrap();
    println!("{json}");
    let back: ProvenanceRow = serde_json::from_str(&json).unwrap();
    assert_eq!(back, row);
    assert_eq!(serde_json::to_string(&back).unwrap(), json, "stable text");
    for column in [
        "\"id\":",
        "\"object_id\":",
        "\"source_kind\":",
        "\"source_uri\":",
        "\"import_run_id\":",
        "\"original_blob\":",
        "\"extractor\":",
        "\"extractor_version\":",
        "\"confidence\":",
        "\"accepted\":",
        "\"created_at\":",
    ] {
        assert!(json.contains(column), "{column} is in the payload");
    }
    // A payload with a column the row does not have is refused, as every row's is.
    let extra = json.replacen("\"accepted\"", "\"flag\":1,\"accepted\"", 1);
    assert!(serde_json::from_str::<ProvenanceRow>(&extra).is_err());
}

// Probe 2: `confidence` is written as the number given and read back exactly (the workspace's
// serde_json keeps float round trips, ADR-0012). NaN and infinity are written as `null` without
// an error, and `null` does not read back as a number: a payload like that would never replay.
// So the row's validation must bound `confidence` to 0..=1 before an op is made.
#[test]
fn confidence_round_trips_exactly_and_nan_writes_a_null_that_cannot_be_read() {
    for value in [0.0, 0.1, 0.3, 1.0, 0.123_456_789_012_345_67] {
        let mut row = row();
        row.confidence = value;
        let json = serde_json::to_string(&row).unwrap();
        let back: ProvenanceRow = serde_json::from_str(&json).unwrap();
        assert_eq!(back.confidence.to_bits(), value.to_bits(), "{value}");
    }
    let json = serde_json::to_string(&row()).unwrap();
    assert!(json.contains("\"confidence\":0.1,"), "{json}");
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut row = row();
        row.confidence = value;
        let json = serde_json::to_string(&row).unwrap();
        println!("{value}: {json}");
        assert!(
            json.contains("\"confidence\":null,"),
            "written as null, no error"
        );
        let back = serde_json::from_str::<ProvenanceRow>(&json);
        assert!(back.is_err(), "and null does not read back as a number");
    }
}

// Probe 3: the chain hashes an op of the new kind by the same bytes as any other kind: the kind
// is text, so no change to the hash is needed, and a different confidence is a different hash.
#[test]
fn the_chain_hashes_a_provenance_op_like_any_other() {
    let payload = serde_json::to_string(&row()).unwrap();
    let create = entry("provenance.create", payload.clone());
    let first = create.hash(None);
    assert_eq!(
        first,
        entry("provenance.create", payload.clone()).hash(None)
    );
    let mut other = row();
    other.confidence = 0.2;
    let changed = entry("provenance.create", serde_json::to_string(&other).unwrap());
    assert_ne!(first, changed.hash(None));
    let delete = entry("provenance.delete", payload);
    assert_ne!(first, delete.hash(None), "create and delete hash apart");
    assert_ne!(
        first,
        create.hash(Some(&first)),
        "the previous hash is in it"
    );
}

// Probe 4: what `dbme-core` says today, which is what Task 2 changes: the kind is unknown, the
// entry does not decode, and the actions the entity will take are create and delete only.
#[test]
fn core_does_not_know_the_kind_yet_and_the_actions_are_create_and_delete() {
    let unknown = OpKind::parse("provenance.create");
    println!("today: {unknown:?}");
    assert!(matches!(unknown, Err(CoreError::UnknownOpKind(_))));
    let payload = serde_json::to_string(&row()).unwrap();
    assert!(entry("provenance.create", payload).op().is_err());
    assert_eq!(Entity::ALL.len(), 8, "eight entities before Task 2");

    // The rule for the new entity, by the pattern attachment set: create and delete exist,
    // update does not (a provenance row is immutable; `accepted` is set when it is written),
    // and move, trash and restore do not apply. Attachment is the entity it will mirror.
    assert!(OpKind::new(Entity::Attachment, Action::Create).is_ok());
    assert!(OpKind::new(Entity::Attachment, Action::Delete).is_ok());
    assert!(OpKind::new(Entity::Attachment, Action::Update).is_err());
    assert!(OpKind::new(Entity::Attachment, Action::Move).is_err());
    assert!(OpKind::new(Entity::Attachment, Action::Trash).is_err());
    let create = OpKind::new(Entity::Attachment, Action::Create).unwrap();
    assert_eq!(create.inverse().action(), Action::Delete);
    assert_eq!(create.inverse().inverse(), create);
}
