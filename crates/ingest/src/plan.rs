// SPDX-License-Identifier: AGPL-3.0-or-later
//! The dry run: every source read, the importer's extract, the duplicates found, nothing written.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use dbme_core::model::AttachmentRole;
use dbme_core::{ContentHash, Id, IdGenerator};
use dbme_store::Vault;
use serde::{Deserialize, Serialize};

use crate::importer::{Ctx, Extract, ExtractedObject};
use crate::source::shown;
use crate::{Importers, IngestError, Source, SourceKind, TRANSACTION_OBJECTS};

/// What to do with a source the vault already holds (M6 Decision 3).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum DuplicatePolicy {
    /// Leave it out and say so.
    #[default]
    Skip,
    /// Relate the existing object to the import's parent instead of making another.
    Attach,
}

/// How a run should go.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ImportOptions {
    /// Where the imported objects land. `None`: a new "Import <date>" note (M6 Decision 4).
    pub parent: Option<Id>,
    /// What to do with duplicates.
    pub duplicates: DuplicatePolicy,
    /// For tables, the column whose cells are the objects' titles, by header or key; the first
    /// column when `None`.
    pub title_column: Option<String>,
}

/// How the entries fall into transactions: whole entries in order, a new transaction when the
/// next entry would take the running count past [`TRANSACTION_OBJECTS`]. The plan and the apply
/// count the same way.
pub(crate) fn batches(entries: &[PlanEntry]) -> Vec<Vec<usize>> {
    let mut batches: Vec<Vec<usize>> = vec![Vec::new()];
    let mut in_batch = 0;
    for (index, entry) in entries.iter().enumerate() {
        if in_batch > 0 && in_batch + entry.objects > TRANSACTION_OBJECTS {
            batches.push(Vec::new());
            in_batch = 0;
        }
        if let Some(batch) = batches.last_mut() {
            batch.push(index);
        }
        in_batch += entry.objects;
    }
    batches
}

/// One source in the plan.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct PlanEntry {
    /// The source.
    pub source: Source,
    /// The importer that took it.
    pub importer: String,
    /// What it would make.
    pub extract: Extract,
    /// How many objects, children counted.
    pub objects: usize,
}

/// A source the vault already holds, or that another source of the same run is.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct Duplicate {
    /// The source left out.
    pub path: PathBuf,
    /// Its content hash.
    pub hash: ContentHash,
    /// The live object that already has it as an original, or `None` when the duplicate is
    /// another source of this run.
    pub existing: Option<Id>,
    /// The title of that object, or the other source's path.
    pub of: String,
}

/// A source the plan could not take, and why.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct Refused {
    /// The path as given.
    pub path: PathBuf,
    /// The reason.
    pub reason: String,
}

/// The dry run, and what [`crate::apply`] takes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ImportPlan {
    /// The run's ULID: every provenance row the apply writes names it.
    pub run_id: Id,
    /// The options the plan was made with.
    pub options: ImportOptions,
    /// The sources that will be imported, in order.
    pub entries: Vec<PlanEntry>,
    /// The sources left out as duplicates.
    pub duplicates: Vec<Duplicate>,
    /// The sources that could not be read.
    pub refused: Vec<Refused>,
    /// Everything the importers warned about, with the source named.
    pub warnings: Vec<String>,
    /// How many objects the apply will make, the receipt note not counted.
    pub objects: usize,
    /// How many transactions, and so undo steps, the apply will take (M6 Decision 7).
    pub transactions: usize,
}

impl ImportPlan {
    /// `true` if there is nothing to apply.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Reads every source and says what an apply would do. Writes nothing to the vault and stores
/// no blob: a dry run leaves no trace. A source that cannot be read is listed as refused and the
/// others go on; a folder is refused until the Markdown importer (Task 6) takes it.
pub fn plan(
    vault: &Vault,
    sources: &[PathBuf],
    options: ImportOptions,
    importers: &Importers,
) -> Result<ImportPlan, IngestError> {
    let read = vault.read();
    if let Some(parent) = options.parent {
        let live = read
            .object(parent)
            .ok()
            .is_some_and(|o| o.deleted_at.is_none());
        if !live {
            return Err(IngestError::NoSuchParent(parent));
        }
    }
    let ctx = Ctx {
        read,
        options: options.clone(),
    };
    let run_id = IdGenerator::new().next(vault.now_ms())?;
    let mut plan = ImportPlan {
        run_id,
        options,
        entries: Vec::new(),
        duplicates: Vec::new(),
        refused: Vec::new(),
        warnings: Vec::new(),
        objects: 0,
        transactions: 0,
    };
    // The originals this run attaches, by hash: a second source with the same bytes is a
    // duplicate of the first.
    let mut seen: HashMap<ContentHash, PathBuf> = HashMap::new();

    for given in sources {
        let source = match Source::open(given) {
            Ok(source) => source,
            Err(e) => {
                plan.refused.push(Refused {
                    path: given.clone(),
                    reason: e.to_string(),
                });
                continue;
            }
        };
        let Some(sniffed) = source.sniffed.as_ref() else {
            debug_assert_eq!(source.kind, SourceKind::Folder);
            plan.refused.push(Refused {
                path: given.clone(),
                reason: "folders are imported by the Markdown importer (M6 Task 6)".into(),
            });
            continue;
        };
        let importer = importers.choose(sniffed);
        let extract = match importer.plan(&source, &ctx) {
            Ok(extract) => extract,
            Err(e) => {
                plan.refused.push(Refused {
                    path: given.clone(),
                    reason: e.to_string(),
                });
                continue;
            }
        };
        plan.warnings.extend(
            extract
                .warnings
                .iter()
                .map(|w| format!("{}: {w}", shown(&source.path))),
        );

        // Dedupe by the originals the extract would attach.
        if let Some(duplicate) = duplicate_of(&ctx, &extract.objects, given, &mut seen)? {
            plan.duplicates.push(duplicate);
            continue;
        }
        let objects = extract.objects.iter().map(ExtractedObject::count).sum();
        plan.objects += objects;
        plan.entries.push(PlanEntry {
            source,
            importer: importer.id().to_owned(),
            extract,
            objects,
        });
    }
    plan.transactions = if plan.entries.is_empty() {
        usize::from(plan.options.parent.is_none())
    } else {
        batches(&plan.entries).len()
    };
    Ok(plan)
}

/// Whether the originals of `objects` are already held, by a live object or by an earlier source
/// of this run. Records the originals in `seen` otherwise.
fn duplicate_of(
    ctx: &Ctx<'_>,
    objects: &[ExtractedObject],
    path: &Path,
    seen: &mut HashMap<ContentHash, PathBuf>,
) -> Result<Option<Duplicate>, IngestError> {
    let mut originals = Vec::new();
    collect_originals(objects, &mut originals);
    for hash in &originals {
        if let Some(earlier) = seen.get(hash) {
            return Ok(Some(Duplicate {
                path: path.to_owned(),
                hash: hash.clone(),
                existing: None,
                of: shown(earlier),
            }));
        }
        if let Some(existing) = ctx
            .read
            .objects_with_blob(hash, AttachmentRole::Original)?
            .first()
        {
            let title = ctx.read.object(*existing)?.title;
            return Ok(Some(Duplicate {
                path: path.to_owned(),
                hash: hash.clone(),
                existing: Some(*existing),
                of: title,
            }));
        }
    }
    for hash in originals {
        seen.insert(hash, path.to_owned());
    }
    Ok(None)
}

fn collect_originals(objects: &[ExtractedObject], into: &mut Vec<ContentHash>) {
    for object in objects {
        into.extend(
            object
                .attachments
                .iter()
                .filter(|a| a.role == AttachmentRole::Original)
                .map(|a| a.hash.clone()),
        );
        collect_originals(&object.children, into);
    }
}
