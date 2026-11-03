// SPDX-License-Identifier: AGPL-3.0-or-later
//! Running a folder entry of a plan: the files folder's engine imports it, every object it took
//! a file into gets its provenance, and the files that were not Markdown go through the matching
//! importer for what it can extract (the drop zone's handoff, ADR-0021 note).

use std::collections::BTreeSet;
use std::path::Path;

use dbme_core::Id;
use dbme_core::model::AttachmentRole;
use dbme_mirror::{Mirror, StdFs};
use dbme_store::{NewAttachment, NewBlock, NewProvenance, Position, Vault};

use crate::apply::{ImportRun, ensure_type};
use crate::importer::{BlobDraft, Ctx, FolderImport};
use crate::source::shown;
use crate::{EXTRACTOR, EXTRACTOR_VERSION, ImportOptions, Importers, IngestError, Source};

/// Imports `folder` under the run's parent and records what the engine did on the run.
pub(crate) fn import_folder(
    vault: &mut Vault,
    run: &mut ImportRun,
    folder: &FolderImport,
    importer_id: &str,
) -> Result<(), IngestError> {
    let fs = StdFs::new(&folder.root).map_err(|e| IngestError::io("opening", &folder.root, e))?;
    let mut mirror = Mirror::new(fs, vault.clock());
    let since = latest_seq(vault)?;
    let inbound = mirror.import_from(vault, Some(run.parent), folder.only.as_deref())?;

    // Provenance on every object a file was taken into, in one step.
    let files = inbound.files.clone();
    let root = shown(&folder.root);
    let run_id = run.run_id;
    let source_kind = format!("import:{importer_id}");
    if !files.is_empty() {
        vault.transact(|txn| {
            for (id, rel_path) in &files {
                let original = txn
                    .read()
                    .attachments(*id)?
                    .into_iter()
                    .find(|a| a.role == AttachmentRole::Original)
                    .map(|a| a.blob_hash);
                txn.add_provenance(NewProvenance {
                    object_id: *id,
                    source_kind: source_kind.clone(),
                    source_uri: Some(format!("{root}/{rel_path}")),
                    import_run_id: Some(run_id),
                    original_blob: original,
                    extractor: EXTRACTOR.into(),
                    extractor_version: EXTRACTOR_VERSION.into(),
                    confidence: 1.0,
                    accepted: true,
                })?;
            }
            Ok(())
        })?;
    }

    // The files that were not Markdown: what the matching importer can add.
    let importers = Importers::v0();
    let created: BTreeSet<Id> = inbound.created.iter().copied().collect();
    for (id, rel_path) in &files {
        if !rel_path.ends_with(".md") && created.contains(id) {
            let warnings = enrich_object(vault, *id, &folder.root.join(rel_path), &importers)?;
            run.warnings.extend(warnings);
        }
    }

    run.warnings
        .extend(inbound.warnings.iter().map(|w| format!("{root}: {w}")));
    run.created.extend(inbound.created.iter().copied());
    run.objects += inbound.created.len() + inbound.applied.len();
    run.transactions += steps_since(vault, since)?;
    Ok(())
}

/// Runs the importer that takes `path` on an object that already holds the file as its original
/// (made by the files folder's drop zone or a folder import), and adds what it extracted: the
/// fields, the blocks the object lacks, the derived attachments, when and where. The title stays
/// the object's own. Returns the importer's warnings. The generic importer adds nothing.
pub fn enrich_object(
    vault: &mut Vault,
    object_id: Id,
    path: &Path,
    importers: &Importers,
) -> Result<Vec<String>, IngestError> {
    let source = Source::open(path)?;
    let Some(sniffed) = source.sniffed.as_ref() else {
        return Ok(Vec::new());
    };
    let importer = importers.choose(sniffed);
    if importer.id() == "file" || importer.id() == "markdown" {
        return Ok(Vec::new());
    }
    let extract = {
        let ctx = Ctx {
            read: vault.read(),
            options: ImportOptions::default(),
        };
        match importer.plan(&source, &ctx) {
            Ok(extract) => extract,
            Err(e) => return Ok(vec![format!("{}: {e}", source.uri())]),
        }
    };
    let mut warnings: Vec<String> = extract
        .warnings
        .iter()
        .map(|w| format!("{}: {w}", source.uri()))
        .collect();
    let Some(object) = extract.objects.first() else {
        return Ok(warnings);
    };
    if object.type_name == "Dataset" {
        // A table's rows belong to a run of their own, not under a dropped file's object.
        warnings.push(format!(
            "{}: a table dropped into a folder is attached as a file; import it on its own for its rows",
            source.uri()
        ));
        return Ok(warnings);
    }
    for attachment in &object.attachments {
        if let BlobDraft::Derived(bytes) = &attachment.blob {
            vault.blobs().put(bytes)?;
        }
    }
    let types = extract.types.clone();
    let object = object.clone();
    vault.transact(|txn| {
        for draft in &types {
            ensure_type(txn, draft)?;
        }
        let have_image = txn
            .read()
            .blocks(object_id)?
            .iter()
            .any(|b| b.kind == "image");
        for block in &object.blocks {
            if block.kind == "image" && have_image {
                continue;
            }
            txn.create_block(NewBlock {
                id: None,
                object_id,
                parent_block_id: None,
                position: Position::Last,
                kind: block.kind.clone(),
                content: block.content.clone(),
            })?;
        }
        for attachment in &object.attachments {
            if let BlobDraft::Derived(_) = &attachment.blob {
                txn.attach(NewAttachment {
                    object_id,
                    blob_hash: attachment.hash.clone(),
                    role: attachment.role,
                    filename: attachment.filename.clone(),
                    mime: attachment.mime.clone(),
                })?;
            }
        }
        if !object.props.is_empty()
            || object.captured_at.is_some()
            || (object.geo_lat.is_some() && object.geo_lon.is_some())
        {
            txn.update_object(object_id, |row| {
                for (key, value) in &object.props {
                    row.props_json.insert(key.clone(), value.clone());
                }
                if object.captured_at.is_some() {
                    row.captured_at = object.captured_at;
                }
                if object.geo_lat.is_some() && object.geo_lon.is_some() {
                    row.geo_lat = object.geo_lat;
                    row.geo_lon = object.geo_lon;
                }
            })?;
        }
        Ok(())
    })?;
    Ok(warnings)
}

/// The `seq` of the vault's newest op, or 0.
fn latest_seq(vault: &Vault) -> Result<i64, IngestError> {
    Ok(vault.read().history(1)?.first().map_or(0, |r| r.seq))
}

/// How many undo steps (distinct op groups) were written after `seq`.
fn steps_since(vault: &Vault, seq: i64) -> Result<usize, IngestError> {
    let read = vault.read();
    let mut groups = BTreeSet::new();
    let mut after = seq;
    loop {
        let page = read.ops_after(after, 1_000)?;
        let Some(last) = page.last() else {
            break;
        };
        after = last.seq;
        groups.extend(page.iter().map(|r| r.entry.group_id));
    }
    Ok(groups.len())
}
