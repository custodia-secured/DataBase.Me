// SPDX-License-Identifier: AGPL-3.0-or-later
//! The importer contract (build plan line 359): `id`, `accepts`, `plan`. An importer reads its
//! source and says what it would make, as an [`Extract`]; it writes nothing. The [`Importers`]
//! registry asks each in turn and falls back to the generic one.

use std::path::Path;

use dbme_core::model::{AttachmentRole, Props};
use dbme_core::{ContentHash, Id};
use dbme_store::Reader;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{IngestError, Sniffed, Source};

/// What an importer may look at while it plans: the vault, read only, and the run's options.
pub struct Ctx<'a> {
    /// The vault as it is now.
    pub read: Reader<'a>,
    /// The options the run was asked for.
    pub options: crate::ImportOptions,
}

/// One importer: a kind of file it knows how to turn into objects.
pub trait Importer: Send + Sync {
    /// A short identifier, written as `import:<id>` into `source_kind`.
    fn id(&self) -> &'static str;
    /// Whether this importer takes the file.
    fn accepts(&self, sniffed: &Sniffed) -> bool;
    /// Reads the source and says what it would make. Writes nothing. Never panics: what it
    /// cannot read is an [`IngestError`].
    fn plan(&self, source: &Source, ctx: &Ctx<'_>) -> Result<Extract, IngestError>;
}

/// The importers in the order they are asked. The last one, the generic importer, accepts every
/// file.
pub struct Importers {
    importers: Vec<Box<dyn Importer>>,
}

impl Default for Importers {
    fn default() -> Self {
        Self::v0()
    }
}

impl Importers {
    /// The importers of this version.
    pub fn v0() -> Self {
        Self {
            importers: vec![
                Box::new(crate::markdown::Markdown),
                Box::new(crate::tables::Csv),
                Box::new(crate::tables::Xlsx),
                Box::new(crate::documents::Pdf::default()),
                Box::new(crate::images::Image),
                Box::new(crate::generic::Generic),
            ],
        }
    }

    /// A registry with `importer` asked before the ones already there.
    pub fn with_first(mut self, importer: Box<dyn Importer>) -> Self {
        self.importers.insert(0, importer);
        self
    }

    /// The importer for a sniffed file: the first that accepts it. The generic importer accepts
    /// everything, so there is always one.
    pub fn choose(&self, sniffed: &Sniffed) -> &dyn Importer {
        self.importers
            .iter()
            .find(|i| i.accepts(sniffed))
            .map_or(&crate::generic::Generic as &dyn Importer, |i| i.as_ref())
    }

    /// The importer for a folder: the Markdown one.
    pub fn folder(&self) -> &dyn Importer {
        self.importers
            .iter()
            .find(|i| i.id() == "markdown")
            .map_or(&crate::markdown::Markdown as &dyn Importer, |i| i.as_ref())
    }

    /// The ids, in order.
    pub fn ids(&self) -> Vec<&'static str> {
        self.importers.iter().map(|i| i.id()).collect()
    }
}

/// What an importer would make of one source.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct Extract {
    /// The objects, in order; each may have children.
    pub objects: Vec<ExtractedObject>,
    /// Types the objects need that the vault may not have: created, or their missing fields
    /// added, before the objects are written (a table's type, one field per column).
    #[serde(default)]
    pub types: Vec<TypeDraft>,
    /// A folder the files folder's engine imports at apply time (Markdown and Obsidian, M6 Task
    /// 6): the plan counts its files and writes nothing; `objects` is then empty.
    #[serde(default)]
    pub folder: Option<FolderImport>,
    /// Things the person should know: a page that gave no text, a cell that did not fit.
    pub warnings: Vec<String>,
}

/// A folder for the files folder's engine (`Mirror::import_from`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct FolderImport {
    /// The folder, canonical.
    pub root: std::path::PathBuf,
    /// Only these paths inside it (a single file dropped in), or every file.
    pub only: Option<Vec<String>>,
    /// How many `.md` files the plan counted.
    pub markdown_files: usize,
    /// How many other files, which become plain objects with the file attached and then go
    /// through the matching importer for what it can extract.
    pub other_files: usize,
}

/// A type an import needs, by name. An existing type of that name is reused and only the fields
/// it lacks are added; the kinds of the fields it has rule (M6 Task 4).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct TypeDraft {
    /// The type's name.
    pub name: String,
    /// Its fields, in column order.
    pub fields: Vec<FieldDraft>,
}

/// A field of a drafted type.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct FieldDraft {
    /// The key, an identifier.
    pub key: String,
    /// The label, the column's header as written.
    pub label: String,
    /// The kind inferred from the column.
    pub kind: dbme_core::model::FieldKind,
}

/// One object to make.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ExtractedObject {
    /// The title.
    pub title: String,
    /// The type, by name. A system type or one the importer creates (Task 4).
    pub type_name: String,
    /// Field values by key.
    #[serde(default)]
    pub props: Props,
    /// The body, top-level blocks in order.
    #[serde(default)]
    pub blocks: Vec<BlockDraft>,
    /// Files to attach: the original and anything derived from it.
    #[serde(default)]
    pub attachments: Vec<AttachmentDraft>,
    /// Objects under this one (a folder's files, a workbook's sheets).
    #[serde(default)]
    pub children: Vec<ExtractedObject>,
    /// When the thing was captured (a photo taken), milliseconds since the epoch.
    #[serde(default)]
    pub captured_at: Option<i64>,
    /// Where, when the source says: latitude.
    #[serde(default)]
    pub geo_lat: Option<f64>,
    /// And longitude.
    #[serde(default)]
    pub geo_lon: Option<f64>,
    /// Where it came from, for the provenance row.
    pub provenance: ProvenanceDraft,
}

/// A block of the body.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct BlockDraft {
    /// The block kind (`paragraph`, `heading`, `image`).
    pub kind: String,
    /// Its content, as `blocks.content_json` (FORMAT.md §3.5).
    pub content: Value,
}

impl BlockDraft {
    /// A paragraph of plain text.
    pub fn paragraph(text: &str) -> Self {
        Self {
            kind: "paragraph".into(),
            content: serde_json::json!({ "inline": [{ "text": text }] }),
        }
    }

    /// A heading of `level` with plain text.
    pub fn heading(level: u8, text: &str) -> Self {
        Self {
            kind: "heading".into(),
            content: serde_json::json!({ "level": level, "inline": [{ "text": text }] }),
        }
    }
}

/// A file to attach, by its bytes or by the source file it is.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct AttachmentDraft {
    /// Where the bytes come from.
    pub blob: BlobDraft,
    /// The hash of those bytes, computed by the plan.
    pub hash: ContentHash,
    /// How many bytes.
    pub size: u64,
    /// What the blob is to the object.
    pub role: AttachmentRole,
    /// The file name to keep.
    pub filename: Option<String>,
    /// The media type.
    pub mime: Option<String>,
}

/// The bytes of an attachment: a file on disk, read again at apply and checked against the
/// planned hash, or bytes the importer made (a thumbnail, extracted text), carried in the plan.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum BlobDraft {
    /// The file at this path.
    File(std::path::PathBuf),
    /// Bytes made by the importer, base64 in the plan's JSON.
    Derived(#[serde(with = "base64_bytes")] Vec<u8>),
}

impl AttachmentDraft {
    /// The source file itself, attached as the original.
    pub fn original(source: &Source, hash: ContentHash) -> Self {
        let sniffed = source.sniffed.as_ref();
        Self {
            blob: BlobDraft::File(source.path.clone()),
            hash,
            size: sniffed.map_or(0, |s| s.size),
            role: AttachmentRole::Original,
            filename: sniffed.map(|s| s.file_name.clone()),
            mime: sniffed.and_then(|s| s.mime.clone()),
        }
    }

    /// Bytes the importer made.
    pub fn derived(bytes: Vec<u8>, role: AttachmentRole, filename: &str, mime: &str) -> Self {
        Self {
            hash: ContentHash::of(&bytes),
            size: bytes.len() as u64,
            blob: BlobDraft::Derived(bytes),
            role,
            filename: Some(filename.to_owned()),
            mime: Some(mime.to_owned()),
        }
    }

    /// The bytes, read from disk (and checked against the planned hash) or carried.
    pub(crate) fn bytes(&self) -> Result<Vec<u8>, IngestError> {
        match &self.blob {
            BlobDraft::Derived(bytes) => Ok(bytes.clone()),
            BlobDraft::File(path) => {
                let bytes = std::fs::read(path).map_err(|e| IngestError::io("reading", path, e))?;
                if ContentHash::of(&bytes) != self.hash {
                    return Err(IngestError::Changed(path.clone()));
                }
                Ok(bytes)
            }
        }
    }
}

/// The provenance an importer claims for an object.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ProvenanceDraft {
    /// The source: the file, or `path#sheet=<name>`, or `path#page=<n>`.
    pub source_uri: String,
    /// The hash of the source file as attached, when it is.
    pub original_blob: Option<ContentHash>,
    /// How sure the importer is of what it extracted, 0 to 1.
    pub confidence: f64,
}

impl ProvenanceDraft {
    /// Provenance from a source file, with full confidence.
    pub fn from_source(source: &Source, original_blob: Option<ContentHash>) -> Self {
        Self {
            source_uri: source.uri(),
            original_blob,
            confidence: 1.0,
        }
    }
}

impl ExtractedObject {
    /// An object with a title and a type, nothing else yet.
    pub fn new(title: &str, type_name: &str, provenance: ProvenanceDraft) -> Self {
        Self {
            title: title.to_owned(),
            type_name: type_name.to_owned(),
            props: Props::new(),
            blocks: Vec::new(),
            attachments: Vec::new(),
            children: Vec::new(),
            captured_at: None,
            geo_lat: None,
            geo_lon: None,
            provenance,
        }
    }

    /// This object and every descendant.
    pub fn count(&self) -> usize {
        1 + self
            .children
            .iter()
            .map(ExtractedObject::count)
            .sum::<usize>()
    }
}

/// A file name for a derived blob beside its source: `<stem>.<suffix>`.
pub fn derived_name(path: &Path, suffix: &str) -> String {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".to_owned());
    format!("{stem}.{suffix}")
}

/// The ID of a system type by name, or an error naming it.
pub(crate) fn type_named(read: &Reader<'_>, name: &str) -> Result<Id, IngestError> {
    read.types()?
        .into_iter()
        .find(|t| t.name == name)
        .map(|t| t.id)
        .ok_or_else(|| IngestError::NoSuchType(name.to_owned()))
}

mod base64_bytes {
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&STANDARD.encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
        let text = String::deserialize(deserializer)?;
        STANDARD.decode(text).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use dbme_core::model::AttachmentRole;

    use super::{
        AttachmentDraft, BlobDraft, BlockDraft, ExtractedObject, Importers, ProvenanceDraft,
        derived_name,
    };
    use crate::Sniffed;

    #[test]
    fn derived_blobs_carry_their_bytes_as_base64_and_read_back() {
        let draft = AttachmentDraft::derived(
            vec![1, 2, 3, 250],
            AttachmentRole::Thumbnail,
            "a.jpg",
            "image/jpeg",
        );
        assert_eq!(draft.size, 4);
        assert_eq!(draft.filename.as_deref(), Some("a.jpg"));
        let json = serde_json::to_string(&draft).unwrap();
        assert!(json.contains("\"derived\":\"AQID+g==\""), "{json}");
        let back: AttachmentDraft = serde_json::from_str(&json).unwrap();
        assert_eq!(back, draft);
        assert_eq!(back.bytes().unwrap(), [1, 2, 3, 250]);
        let bad = json.replace("AQID+g==", "not base64!");
        assert!(serde_json::from_str::<AttachmentDraft>(&bad).is_err());
        assert!(matches!(back.blob, BlobDraft::Derived(_)));
    }

    #[test]
    fn blocks_names_and_counts() {
        let heading = BlockDraft::heading(2, "Page 1");
        assert_eq!(heading.kind, "heading");
        assert_eq!(heading.content["level"], 2);
        assert_eq!(heading.content["inline"][0]["text"], "Page 1");
        assert_eq!(derived_name(Path::new("C:/in/Scan.PDF"), "txt"), "Scan.txt");
        assert_eq!(derived_name(Path::new("/"), "txt"), "file.txt");
        let provenance = ProvenanceDraft {
            source_uri: "x".into(),
            original_blob: None,
            confidence: 1.0,
        };
        let mut root = ExtractedObject::new("Root", "Note", provenance.clone());
        let mut child = ExtractedObject::new("Child", "Note", provenance.clone());
        child
            .children
            .push(ExtractedObject::new("Grandchild", "Note", provenance));
        root.children.push(child);
        assert_eq!(root.count(), 3);
        let registry = Importers::default();
        assert_eq!(
            registry.ids(),
            ["markdown", "csv", "xlsx", "pdf", "image", "file"]
        );
        let sniffed = Sniffed::from_parts(Path::new("x.bin"), 0, b"");
        assert_eq!(registry.choose(&sniffed).id(), "file");
    }
}
