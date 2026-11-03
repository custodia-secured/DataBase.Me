// SPDX-License-Identifier: AGPL-3.0-or-later
//! Markdown and Obsidian folders (M6 Task 6, ADR-0026), through the files folder's engine
//! (`Mirror::import_from`): every `.md` file is read by M5's rules (frontmatter fields, `id:`
//! adopted or re-issued, wikilinks resolved within the batch, `#tags`, embeds), nested files land
//! under their folder's owner, other files become plain objects with the file attached and are
//! then handed to the matching importer for what it can extract, and nothing is written back to
//! the folder. The plan only counts the files; the engine runs at apply time, one undo step per
//! file (M6 Decision 16).

use std::path::Path;

use dbme_mirror::{Fs, StdFs};

use crate::importer::{Ctx, Extract, FolderImport, Importer};
use crate::{IngestError, Sniffed, Source, SourceKind};

/// The Markdown importer: folders, and single `.md` files.
#[derive(Clone, Copy, Debug, Default)]
pub struct Markdown;

/// Whether a path has a component the engine never reads as an object: a dot path, or the
/// attachments the files folder writes itself.
fn skipped(rel_path: &str) -> bool {
    rel_path.split('/').any(|c| c.starts_with('.')) || rel_path.starts_with("attachments/")
}

impl Importer for Markdown {
    fn id(&self) -> &'static str {
        "markdown"
    }

    fn accepts(&self, sniffed: &Sniffed) -> bool {
        sniffed.has_extension(&["md", "markdown"])
    }

    fn plan(&self, source: &Source, _ctx: &Ctx<'_>) -> Result<Extract, IngestError> {
        let folder = match source.kind {
            SourceKind::Folder => {
                let fs = StdFs::new(&source.path)
                    .map_err(|e| IngestError::io("opening", &source.path, e))?;
                let entries = fs
                    .list()
                    .map_err(|e| IngestError::io("listing", &source.path, e))?;
                let paths: Vec<&str> = entries
                    .iter()
                    .map(|e| e.rel_path.as_str())
                    .filter(|p| !skipped(p))
                    .collect();
                let markdown_files = paths.iter().filter(|p| p.ends_with(".md")).count();
                FolderImport {
                    root: source.path.clone(),
                    only: None,
                    markdown_files,
                    other_files: paths.len() - markdown_files,
                }
            }
            SourceKind::File => {
                let root = source
                    .path
                    .parent()
                    .map(Path::to_path_buf)
                    .ok_or_else(|| IngestError::NotASource(source.path.clone()))?;
                let name = source
                    .sniffed
                    .as_ref()
                    .map(|s| s.file_name.clone())
                    .unwrap_or_default();
                FolderImport {
                    root,
                    only: Some(vec![name]),
                    markdown_files: 1,
                    other_files: 0,
                }
            }
        };
        Ok(Extract {
            folder: Some(folder),
            ..Extract::default()
        })
    }
}
