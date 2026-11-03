// SPDX-License-Identifier: AGPL-3.0-or-later
//! Writing a plan to the vault: one transaction, which is the undo step, or several past
//! [`TRANSACTION_OBJECTS`] objects; blobs stored, objects, blocks, attachments and provenance
//! rows written; the receipt note when no parent was named (M6 Decisions 4 and 7).

use dbme_core::Id;
use dbme_store::{
    NewAttachment, NewBlock, NewField, NewObject, NewProvenance, NewRelation, NewType, Position,
    Txn, Vault,
};
use serde::{Deserialize, Serialize};

use crate::importer::{AttachmentDraft, BlockDraft, ExtractedObject, TypeDraft, type_named};
use crate::plan::{DuplicatePolicy, ImportPlan, PlanEntry, batches};
use crate::{EXTRACTOR, EXTRACTOR_VERSION, IngestError};

/// What an apply did.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ImportRun {
    /// The run's ULID, as in the plan and in every provenance row.
    pub run_id: Id,
    /// The parent the objects landed under: the one named, or the receipt note.
    pub parent: Id,
    /// The receipt note, when the run made one.
    pub receipt: Option<Id>,
    /// The top-level objects made, one per plan entry, in order.
    pub created: Vec<Id>,
    /// Every object made, children included.
    pub objects: usize,
    /// How many transactions it took: how many undo steps.
    pub transactions: usize,
    /// Duplicates related to the parent under the `attach` policy.
    pub attached: Vec<Id>,
    /// What the apply itself found worth saying: a folder's inbox entries, what an importer
    /// could not add to a dropped file. The plan's warnings are not repeated here.
    pub warnings: Vec<String>,
}

/// Writes `plan` to `vault`. The first transaction makes the receipt note when no parent was
/// named; every transaction holds whole plan entries up to [`TRANSACTION_OBJECTS`] objects. A
/// failure leaves the transactions already committed as undo steps and returns the error.
pub fn apply(vault: &mut Vault, plan: &ImportPlan) -> Result<ImportRun, IngestError> {
    let read = vault.read();
    if let Some(parent) = plan.options.parent {
        let live = read
            .object(parent)
            .ok()
            .is_some_and(|o| o.deleted_at.is_none());
        if !live {
            return Err(IngestError::NoSuchParent(parent));
        }
    }
    // Every type the plan names must exist, or be one the plan creates, before anything is
    // written, so a bad plan writes nothing at all.
    for entry in &plan.entries {
        let drafted: Vec<&str> = entry
            .extract
            .types
            .iter()
            .map(|t| t.name.as_str())
            .collect();
        check_types(&read, &drafted, &entry.extract.objects)?;
    }

    let mut run = ImportRun {
        run_id: plan.run_id,
        parent: plan.options.parent.unwrap_or(plan.run_id),
        receipt: None,
        created: Vec::new(),
        objects: 0,
        transactions: 0,
        attached: Vec::new(),
        warnings: Vec::new(),
    };

    let batches: Vec<Vec<&PlanEntry>> = batches(&plan.entries)
        .into_iter()
        .map(|batch| batch.into_iter().map(|i| &plan.entries[i]).collect())
        .collect();

    for (n, batch) in batches.iter().enumerate() {
        let first = n == 0;
        if batch.is_empty() && !(first && plan.options.parent.is_none()) {
            continue;
        }
        // The blobs go into the store before the transaction, which only records them: the
        // store deduplicates by hash, and a blob outlives an undo as every blob does.
        for entry in batch {
            store_blobs(vault, &entry.extract.objects)?;
        }
        let (made, committed) = vault.transact(|txn| {
            let mut made = Vec::new();
            if first && plan.options.parent.is_none() {
                let receipt = make_receipt(txn, plan).map_err(store_error)?;
                run.parent = receipt;
                run.receipt = Some(receipt);
            }
            for entry in batch {
                for draft in &entry.extract.types {
                    ensure_type(txn, draft)?;
                }
                for object in &entry.extract.objects {
                    let id = write_object(txn, plan.run_id, run.parent, &entry.importer, object)
                        .map_err(store_error)?;
                    made.push(id);
                    run.objects += object.count();
                }
            }
            if first && plan.options.duplicates == DuplicatePolicy::Attach {
                for duplicate in &plan.duplicates {
                    if let Some(existing) = duplicate.existing {
                        txn.create_relation(NewRelation {
                            src_id: run.parent,
                            dst_id: existing,
                            rel_type: "related".into(),
                            field_id: None,
                            src_block_id: None,
                            label: Some("already imported".into()),
                            props: Default::default(),
                        })?;
                        run.attached.push(existing);
                    }
                }
            }
            Ok(made)
        })?;
        run.created.extend(made);
        if committed.is_some() {
            run.transactions += 1;
        }
        // The folders of this batch: the files folder's engine takes them in, under the parent
        // the transaction above made or the one named, as undo steps of its own.
        for entry in batch {
            if let Some(folder) = &entry.extract.folder {
                crate::folders::import_folder(vault, &mut run, folder, &entry.importer)?;
            }
        }
    }
    Ok(run)
}

/// Inside a transaction the store's error type rules; the types were checked and the blobs
/// stored before it began, so what is left is the store's own.
fn store_error(error: IngestError) -> dbme_store::StoreError {
    match error {
        IngestError::Store(e) => e,
        IngestError::Core(e) => dbme_store::StoreError::Core(e),
        other => dbme_store::StoreError::Rule(other.to_string()),
    }
}

fn check_types(
    read: &dbme_store::Reader<'_>,
    drafted: &[&str],
    objects: &[ExtractedObject],
) -> Result<(), IngestError> {
    for object in objects {
        if !drafted.contains(&object.type_name.as_str()) {
            type_named(read, &object.type_name)?;
        }
        check_types(read, drafted, &object.children)?;
    }
    Ok(())
}

/// Creates the drafted type, or adds the fields an existing type of that name lacks. An existing
/// field keeps its kind (the importer planned around it).
pub(crate) fn ensure_type(txn: &mut Txn<'_>, draft: &TypeDraft) -> dbme_store::Result<()> {
    let existing = txn
        .read()
        .types()?
        .into_iter()
        .find(|t| t.name == draft.name);
    let type_id = match existing {
        Some(t) => t.id,
        None => txn.create_type(NewType {
            name: draft.name.clone(),
            ..NewType::default()
        })?,
    };
    let have: Vec<String> = txn
        .read()
        .fields(type_id)?
        .into_iter()
        .map(|f| f.key)
        .collect();
    for field in &draft.fields {
        if !have.contains(&field.key) {
            txn.create_field(NewField {
                type_id,
                key: field.key.clone(),
                label: field.label.clone(),
                kind: field.kind,
                options: serde_json::json!({}),
                required: false,
            })?;
        }
    }
    Ok(())
}

/// The "Import <date>" note: a `Note` at the top level whose body is the run's receipt.
fn make_receipt(txn: &mut Txn<'_>, plan: &ImportPlan) -> Result<Id, IngestError> {
    let note = type_named(&txn.read(), "Note")?;
    let title = format!("Import {}", date_of(txn.now()));
    let id = txn.create_object(NewObject {
        title,
        type_ids: vec![note],
        ..NewObject::default()
    })?;
    for block in receipt_blocks(plan) {
        txn.create_block(NewBlock {
            id: None,
            object_id: id,
            parent_block_id: None,
            position: Position::Last,
            kind: block.kind,
            content: block.content,
        })?;
    }
    txn.add_provenance(NewProvenance {
        object_id: id,
        source_kind: "import:receipt".into(),
        source_uri: None,
        import_run_id: Some(plan.run_id),
        original_blob: None,
        extractor: EXTRACTOR.into(),
        extractor_version: EXTRACTOR_VERSION.into(),
        confidence: 1.0,
        accepted: true,
    })?;
    Ok(id)
}

/// The receipt as blocks: what was imported, left out and warned about.
pub fn receipt_blocks(plan: &ImportPlan) -> Vec<BlockDraft> {
    let mut blocks = vec![BlockDraft::paragraph(&format!(
        "{} object{} from {} source{}, in {} step{}.",
        plan.objects,
        plural(plan.objects),
        plan.entries.len(),
        plural(plan.entries.len()),
        plan.transactions,
        plural(plan.transactions),
    ))];
    for entry in &plan.entries {
        blocks.push(BlockDraft::paragraph(&format!(
            "{}: {} object{} by {}",
            entry.source.uri(),
            entry.objects,
            plural(entry.objects),
            entry.importer
        )));
    }
    if !plan.duplicates.is_empty() {
        blocks.push(BlockDraft::heading(2, "Already in the vault"));
        for d in &plan.duplicates {
            blocks.push(BlockDraft::paragraph(&format!(
                "{}: the same file as {}",
                crate::source::shown(&d.path),
                d.of
            )));
        }
    }
    if !plan.refused.is_empty() {
        blocks.push(BlockDraft::heading(2, "Not imported"));
        for r in &plan.refused {
            blocks.push(BlockDraft::paragraph(&format!(
                "{}: {}",
                crate::source::shown(&r.path),
                r.reason
            )));
        }
    }
    if !plan.warnings.is_empty() {
        blocks.push(BlockDraft::heading(2, "Warnings"));
        for w in &plan.warnings {
            blocks.push(BlockDraft::paragraph(w));
        }
    }
    blocks
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

/// Writes one extracted object under `parent`, with its blocks, attachments, provenance and
/// children. Returns its ID.
fn write_object(
    txn: &mut Txn<'_>,
    run_id: Id,
    parent: Id,
    importer: &str,
    object: &ExtractedObject,
) -> Result<Id, IngestError> {
    let type_id = type_named(&txn.read(), &object.type_name)?;
    let id = txn.create_object(NewObject {
        title: object.title.clone(),
        parent_id: Some(parent),
        type_ids: vec![type_id],
        props: object.props.clone(),
        captured_at: object.captured_at,
        geo_lat: object.geo_lat,
        geo_lon: object.geo_lon,
        ..NewObject::default()
    })?;
    for block in &object.blocks {
        txn.create_block(NewBlock {
            id: None,
            object_id: id,
            parent_block_id: None,
            position: Position::Last,
            kind: block.kind.clone(),
            content: block.content.clone(),
        })?;
    }
    for attachment in &object.attachments {
        write_attachment(txn, id, attachment)?;
    }
    txn.add_provenance(NewProvenance {
        object_id: id,
        source_kind: format!("import:{importer}"),
        source_uri: Some(object.provenance.source_uri.clone()),
        import_run_id: Some(run_id),
        original_blob: object.provenance.original_blob.clone(),
        extractor: EXTRACTOR.into(),
        extractor_version: EXTRACTOR_VERSION.into(),
        confidence: object.provenance.confidence,
        accepted: true,
    })?;
    for child in &object.children {
        write_object(txn, run_id, id, importer, child)?;
    }
    Ok(id)
}

/// Puts every attachment's bytes of `objects` and their children into the blob store, reading a
/// file again and refusing one that changed since the plan.
fn store_blobs(vault: &Vault, objects: &[ExtractedObject]) -> Result<(), IngestError> {
    for object in objects {
        for attachment in &object.attachments {
            let bytes = attachment.bytes()?;
            let stored = vault.blobs().put(&bytes)?;
            debug_assert_eq!(stored.hash, attachment.hash);
        }
        store_blobs(vault, &object.children)?;
    }
    Ok(())
}

/// Attaches a blob [`store_blobs`] put in the store.
fn write_attachment(
    txn: &mut Txn<'_>,
    object_id: Id,
    attachment: &AttachmentDraft,
) -> Result<(), IngestError> {
    txn.attach(NewAttachment {
        object_id,
        blob_hash: attachment.hash.clone(),
        role: attachment.role,
        filename: attachment.filename.clone(),
        mime: attachment.mime.clone(),
    })?;
    Ok(())
}

/// `YYYY-MM-DD` in UTC for a time in milliseconds since the epoch (Howard Hinnant's civil-from-
/// days, so no date crate is needed).
pub fn date_of(ms: i64) -> String {
    let days = ms.div_euclid(86_400_000);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

/// `YYYY-MM-DDTHH:MM:SSZ` (with `.mmm` when the milliseconds are not zero) in UTC for a time in
/// milliseconds since the epoch: the `datetime` field value (FORMAT.md §3.3).
pub fn datetime_of(ms: i64) -> String {
    let day_ms = ms.rem_euclid(86_400_000);
    let (h, m, s, milli) = (
        day_ms / 3_600_000,
        day_ms / 60_000 % 60,
        day_ms / 1_000 % 60,
        day_ms % 1_000,
    );
    if milli == 0 {
        format!("{}T{h:02}:{m:02}:{s:02}Z", date_of(ms))
    } else {
        format!("{}T{h:02}:{m:02}:{s:02}.{milli:03}Z", date_of(ms))
    }
}

#[cfg(test)]
mod tests {
    use super::{date_of, datetime_of, store_error};
    use crate::IngestError;

    #[test]
    fn errors_inside_a_transaction_become_the_store_s() {
        let core = dbme_core::CoreError::invalid("x", "y");
        assert!(matches!(
            store_error(IngestError::Core(core)),
            dbme_store::StoreError::Core(_)
        ));
        let store = dbme_store::StoreError::Rule("r".into());
        assert!(matches!(
            store_error(IngestError::Store(store)),
            dbme_store::StoreError::Rule(r) if r == "r"
        ));
        assert!(matches!(
            store_error(IngestError::Changed("a".into())),
            dbme_store::StoreError::Rule(r) if r.contains("changed since")
        ));
    }

    #[test]
    fn dates_come_out_in_utc_with_leap_days() {
        assert_eq!(date_of(0), "1970-01-01");
        assert_eq!(date_of(-1), "1969-12-31");
        assert_eq!(date_of(951_782_400_000), "2000-02-29");
        assert_eq!(date_of(1_762_128_000_000), "2025-11-03");
        assert_eq!(date_of(1_762_127_999_999), "2025-11-02");
        assert_eq!(datetime_of(0), "1970-01-01T00:00:00Z");
        assert_eq!(datetime_of(1_762_127_999_999), "2025-11-02T23:59:59.999Z");
        assert_eq!(
            datetime_of(1_762_128_000_000 + 43_200_000),
            "2025-11-03T12:00:00Z"
        );
        assert_eq!(datetime_of(-1_000), "1969-12-31T23:59:59Z");
    }
}
