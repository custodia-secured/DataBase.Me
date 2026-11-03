// SPDX-License-Identifier: AGPL-3.0-or-later
//! Export and import (FORMAT.md §7, M5 Decision 18).
//!
//! [`export`] writes the reader files, for other tools: `schema/types.json`, one
//! `views/<id>.json` per saved view and one `data/<Type>.csv` per type. With `full`, it adds what
//! makes the folder a copy of the vault: `vault_meta.json`, `ops.jsonl` (the op log, one op per
//! line in `seq` order, every column) and `blobs/` in the vault's own layout. [`import`] creates a
//! vault from such a folder under the exported `vault_id`, without the built-in types, replays
//! every op and copies the blobs; the hash chain must come out identical or nothing is left behind.

use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use dbme_core::chain::OpRecord;
use dbme_core::files::paths;
use dbme_core::model::{FieldRow, ObjectRow, TypeRow};
use dbme_store::{BLOBS_FOLDER, StoreError, Vault, VaultMeta, VaultOptions};

/// Every type with its fields, as one JSON array.
pub const SCHEMA_FILE: &str = "schema/types.json";
/// One JSON file per saved view, named by the view's ID.
pub const VIEWS_DIR: &str = "views";
/// One CSV per type, named by the type.
pub const DATA_DIR: &str = "data";
/// The `vault_meta` table as an object (full export).
pub const META_FILE: &str = "vault_meta.json";
/// The op log, one op per line (full export).
pub const OPS_FILE: &str = "ops.jsonl";
/// The blob store (full export), laid out as in the vault.
pub const BLOBS_DIR: &str = "blobs";

/// How many ops one query reads while writing `ops.jsonl`.
const OPS_PAGE: u32 = 1_000;

/// Everything that can stop an export or an import.
#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    /// The store refused or failed.
    #[error(transparent)]
    Store(#[from] StoreError),

    /// A file could not be read or written.
    #[error("{action} {path}: {source}")]
    Io {
        /// What was being done, for example `writing`.
        action: &'static str,
        /// The file or folder.
        path: PathBuf,
        /// The operating system's error.
        source: std::io::Error,
    },

    /// A file of the export is not the JSON it should be.
    #[error("{path}: {source}")]
    Json {
        /// The file.
        path: PathBuf,
        /// What was wrong with it.
        source: serde_json::Error,
    },

    /// The folders given cannot be used as asked.
    #[error("{0}")]
    Rule(String),
}

/// The crate's result type.
pub type Result<T> = std::result::Result<T, ExportError>;

fn io(action: &'static str, path: &Path) -> impl FnOnce(std::io::Error) -> ExportError {
    let path = path.to_owned();
    move |source| ExportError::Io {
        action,
        path,
        source,
    }
}

/// One entry of `schema/types.json`: the type's row with its own fields (not inherited ones).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TypeExport {
    /// The type.
    #[serde(flatten)]
    pub type_row: TypeRow,
    /// Its fields, in display order.
    pub fields: Vec<FieldRow>,
}

/// What an export wrote.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ExportReport {
    /// Types in `schema/types.json`, each with a CSV under `data/`.
    pub types: usize,
    /// Files under `views/`.
    pub views: usize,
    /// Rows across every CSV (an object with two types is counted twice).
    pub objects: usize,
    /// Lines of `ops.jsonl`, for a full export.
    pub ops: Option<u64>,
    /// Files copied under `blobs/`, for a full export.
    pub blobs: Option<u64>,
}

/// Writes the export of `vault` into `dir`, which must not exist yet or be empty. With `full`,
/// the op log, the vault's facts and the blobs are included, so [`import`] can rebuild it.
pub fn export(vault: &Vault, dir: &Path, full: bool) -> Result<ExportReport> {
    ensure_empty_dir(dir)?;
    let read = vault.read();
    let mut report = ExportReport::default();

    // schema/types.json
    let types = read.types()?;
    let mut entries = Vec::with_capacity(types.len());
    for type_row in &types {
        entries.push(TypeExport {
            fields: read.fields(type_row.id)?,
            type_row: type_row.clone(),
        });
    }
    write_json(&dir.join(SCHEMA_FILE), &entries)?;
    report.types = entries.len();

    // views/<id>.json
    let views_dir = dir.join(VIEWS_DIR);
    fs::create_dir_all(&views_dir).map_err(io("creating", &views_dir))?;
    for view in read.views()? {
        write_json(&views_dir.join(format!("{}.json", view.id)), &view)?;
        report.views += 1;
    }

    // data/<Type>.csv
    let data_dir = dir.join(DATA_DIR);
    fs::create_dir_all(&data_dir).map_err(io("creating", &data_dir))?;
    let mut taken: BTreeSet<String> = BTreeSet::new();
    for type_row in &types {
        let stem = paths::dedupe(&paths::file_stem(&type_row.name), &|candidate| {
            taken.contains(&paths::path_key(candidate))
        });
        taken.insert(paths::path_key(&stem));
        let fields = read.fields_of_types(&[type_row.id])?;
        let objects = read.objects_of_type(type_row.id)?;
        write_csv(&data_dir.join(format!("{stem}.csv")), &fields, &objects)?;
        report.objects += objects.len();
    }

    if full {
        write_json(&dir.join(META_FILE), &read.vault_meta_record()?)?;
        report.ops = Some(write_ops(vault, &dir.join(OPS_FILE))?);
        report.blobs = Some(copy_tree(
            &vault.root().join(BLOBS_FOLDER),
            &dir.join(BLOBS_DIR),
        )?);
    }
    Ok(report)
}

/// Whether `source` is a full export: it has `ops.jsonl` and `vault_meta.json`.
pub fn is_export(source: &Path) -> bool {
    source.join(OPS_FILE).is_file() && source.join(META_FILE).is_file()
}

/// Creates the vault at `path` from the full export in `source` and opens it. The ops are
/// replayed in order and must reproduce the exported hash chain; the blobs are copied in. On any
/// failure nothing is left at `path`.
pub fn import(source: &Path, path: &Path, options: VaultOptions) -> Result<Vault> {
    let meta_path = source.join(META_FILE);
    let meta_file = File::open(&meta_path).map_err(io("reading", &meta_path))?;
    let meta: VaultMeta =
        serde_json::from_reader(BufReader::new(meta_file)).map_err(|source| ExportError::Json {
            path: meta_path.clone(),
            source,
        })?;

    let ops_path = source.join(OPS_FILE);
    let ops_file = File::open(&ops_path).map_err(io("reading", &ops_path))?;
    let ops = BufReader::new(ops_file)
        .lines()
        .enumerate()
        .filter_map(|(index, line)| match line {
            Ok(text) if text.trim().is_empty() => None,
            Ok(text) => Some(
                serde_json::from_str::<OpRecord>(&text)
                    .map_err(|e| StoreError::Rule(format!("{OPS_FILE} line {}: {e}", index + 1))),
            ),
            Err(source) => Some(Err(StoreError::Io {
                action: "reading",
                path: ops_path.clone(),
                source,
            })),
        });
    let vault = Vault::create_from_ops(path, options, meta, ops)?;

    let blobs = source.join(BLOBS_DIR);
    if blobs.is_dir()
        && let Err(error) = copy_tree(&blobs, &vault.root().join(BLOBS_FOLDER))
    {
        drop(vault);
        let _ = fs::remove_dir_all(path);
        return Err(error);
    }
    Ok(vault)
}

fn ensure_empty_dir(dir: &Path) -> Result<()> {
    if dir.exists() {
        let mut entries = fs::read_dir(dir).map_err(io("reading", dir))?;
        if entries.next().is_some() {
            return Err(ExportError::Rule(format!(
                "{} exists and is not empty; export into a new or empty folder",
                dir.display()
            )));
        }
    }
    fs::create_dir_all(dir).map_err(io("creating", dir))
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(io("creating", parent))?;
    }
    let mut text = serde_json::to_string_pretty(value).map_err(|source| ExportError::Json {
        path: path.to_owned(),
        source,
    })?;
    text.push('\n');
    fs::write(path, text).map_err(io("writing", path))
}

/// One op per line, in `seq` order.
fn write_ops(vault: &Vault, path: &Path) -> Result<u64> {
    let file = File::create(path).map_err(io("writing", path))?;
    let mut out = BufWriter::new(file);
    let read = vault.read();
    let mut after = 0;
    let mut count = 0;
    loop {
        let page = read.ops_after(after, OPS_PAGE)?;
        let Some(last) = page.last() else { break };
        after = last.seq;
        for record in &page {
            serde_json::to_writer(&mut out, record).map_err(|source| ExportError::Json {
                path: path.to_owned(),
                source,
            })?;
            out.write_all(b"\n").map_err(io("writing", path))?;
            count += 1;
        }
    }
    out.flush().map_err(io("writing", path))?;
    Ok(count)
}

/// Copies every file under `from` to the same path under `to`, making folders as needed.
/// Symbolic links are not followed. Returns how many files were copied.
fn copy_tree(from: &Path, to: &Path) -> Result<u64> {
    fs::create_dir_all(to).map_err(io("creating", to))?;
    let mut count = 0;
    for entry in fs::read_dir(from).map_err(io("reading", from))? {
        let entry = entry.map_err(io("reading", from))?;
        let source = entry.path();
        let kind = fs::symlink_metadata(&source)
            .map_err(io("reading", &source))?
            .file_type();
        let target = to.join(entry.file_name());
        if kind.is_dir() {
            count += copy_tree(&source, &target)?;
        } else if kind.is_file() {
            fs::copy(&source, &target).map_err(io("copying", &source))?;
            count += 1;
        }
    }
    Ok(count)
}

/// RFC 4180 with LF line ends: `id`, `title`, `parent_id`, `created_at`, `updated_at`, then the
/// fields in display order. A value that is a list or a map is one JSON cell.
fn write_csv(path: &Path, fields: &[FieldRow], objects: &[ObjectRow]) -> Result<()> {
    let mut text = String::new();
    let mut header: Vec<&str> = vec!["id", "title", "parent_id", "created_at", "updated_at"];
    header.extend(fields.iter().map(|f| f.key.as_str()));
    csv_line(&mut text, header.into_iter());
    for object in objects {
        let parent = object
            .parent_id
            .map(|id| id.to_string())
            .unwrap_or_default();
        let mut cells = vec![
            object.id.to_string(),
            object.title.clone(),
            parent,
            object.created_at.to_string(),
            object.updated_at.to_string(),
        ];
        for field in fields {
            cells.push(match object.props_json.get(&field.key) {
                None | Some(serde_json::Value::Null) => String::new(),
                Some(serde_json::Value::String(s)) => s.clone(),
                Some(other) => other.to_string(),
            });
        }
        csv_line(&mut text, cells.iter().map(String::as_str));
    }
    fs::write(path, text).map_err(io("writing", path))
}

fn csv_line<'a>(out: &mut String, cells: impl Iterator<Item = &'a str>) {
    for (n, cell) in cells.enumerate() {
        if n > 0 {
            out.push(',');
        }
        if cell.contains([',', '"', '\n', '\r']) {
            out.push('"');
            out.push_str(&cell.replace('"', "\"\""));
            out.push('"');
        } else {
            out.push_str(cell);
        }
    }
    out.push('\n');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_cells_are_quoted_only_when_they_need_it() {
        let mut out = String::new();
        csv_line(
            &mut out,
            ["plain", "a,b", "say \"hi\"", "two\nlines", ""].into_iter(),
        );
        assert_eq!(out, "plain,\"a,b\",\"say \"\"hi\"\"\",\"two\nlines\",\n");
    }
}
