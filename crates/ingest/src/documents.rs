// SPDX-License-Identifier: AGPL-3.0-or-later
//! PDF text (M6 Task 5, ADR-0026): a PDF becomes a `Document` with the file attached, its text
//! page by page as paragraph blocks under a heading per page, and the page count and author as
//! fields; the document's own title, when it names one, is the object's. A file the parser
//! refuses, or one over [`MAX_PDF_BYTES`], is still imported, as the plain file with a warning
//! and a lower confidence: never dropped. Extraction stops at [`PDF_BUDGET`], checked between
//! pages, and says how many pages it reached.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::{Duration, Instant};

use dbme_core::ContentHash;
use dbme_core::model::FieldKind;
use pdf_extract::{Document as PdfDocument, Object, PlainTextOutput, output_doc_page};
use serde_json::json;

use crate::importer::{
    AttachmentDraft, BlockDraft, Ctx, Extract, ExtractedObject, FieldDraft, Importer,
    ProvenanceDraft, TypeDraft,
};
use crate::{IngestError, Sniffed, Source};

/// A PDF larger than this is attached without its text (M6.md Design, caps).
pub const MAX_PDF_BYTES: u64 = 64 << 20;
/// How long extraction may take; checked between pages.
pub const PDF_BUDGET: Duration = Duration::from_secs(10);

/// The PDF importer.
#[derive(Clone, Copy, Debug)]
pub struct Pdf {
    /// The extraction budget; [`PDF_BUDGET`] unless a test says otherwise.
    pub budget: Duration,
}

impl Default for Pdf {
    fn default() -> Self {
        Self { budget: PDF_BUDGET }
    }
}

/// The fields a PDF import puts on `Document`.
fn document_fields() -> TypeDraft {
    TypeDraft {
        name: "Document".into(),
        fields: vec![
            FieldDraft {
                key: "page_count".into(),
                label: "Pages".into(),
                kind: FieldKind::Number,
            },
            FieldDraft {
                key: "author".into(),
                label: "Author".into(),
                kind: FieldKind::Text,
            },
        ],
    }
}

impl Importer for Pdf {
    fn id(&self) -> &'static str {
        "pdf"
    }

    fn accepts(&self, sniffed: &Sniffed) -> bool {
        sniffed.has_extension(&["pdf"]) || sniffed.mime.as_deref() == Some("application/pdf")
    }

    fn plan(&self, source: &Source, _ctx: &Ctx<'_>) -> Result<Extract, IngestError> {
        Ok(self.extract(source, &source.read()?))
    }
}

impl Pdf {
    /// What the file's bytes become: a `Document` with the original attached, its text as
    /// blocks when the parser takes the file, a warning when it does not.
    pub(crate) fn extract(&self, source: &Source, bytes: &[u8]) -> Extract {
        let hash = ContentHash::of(bytes);
        let mut extract = Extract {
            types: vec![document_fields()],
            ..Extract::default()
        };
        let mut object = ExtractedObject::new(
            &source.stem(),
            "Document",
            ProvenanceDraft::from_source(source, Some(hash.clone())),
        );
        object
            .attachments
            .push(AttachmentDraft::original(source, hash));

        if bytes.len() as u64 > MAX_PDF_BYTES {
            extract.warnings.push(format!(
                "{} bytes, over the {MAX_PDF_BYTES}-byte limit for text: attached without its text",
                bytes.len()
            ));
            object.provenance.confidence = 0.5;
        } else {
            // The parser panics on some malformed files (found by the proptest layer, M6 Task 9:
            // a failed assertion on an operator's operands, unwraps on missing objects). A panic
            // is contained here, so one bad file costs its text and not the import.
            let outcome = catch_unwind(AssertUnwindSafe(|| {
                PdfDocument::load_mem(bytes)
                    .map(|doc| self.extract_pages(&doc, &mut object, &mut extract.warnings))
            }));
            match outcome {
                Ok(Ok(())) => {}
                Ok(Err(e)) => {
                    extract.warnings.push(format!(
                        "not a PDF the parser takes ({e}): attached without its text"
                    ));
                    object.provenance.confidence = 0.5;
                }
                Err(_) => {
                    extract.warnings.push(
                        "the PDF parser failed on this file: attached without its text".into(),
                    );
                    object.blocks.clear();
                    object.provenance.confidence = 0.5;
                }
            }
        }
        extract.objects.push(object);
        extract
    }
    /// The title and author from the Info dictionary, the page count, and the text of each page
    /// as blocks, inside the budget.
    fn extract_pages(
        &self,
        doc: &PdfDocument,
        object: &mut ExtractedObject,
        warnings: &mut Vec<String>,
    ) {
        if let Some(title) = info_text(doc, b"Title")
            && !title.is_empty()
        {
            object.title = title.chars().take(1_000).collect();
        }
        if let Some(author) = info_text(doc, b"Author")
            && !author.is_empty()
        {
            object.props.insert(
                "author".into(),
                json!(author.chars().take(1_000).collect::<String>()),
            );
        }
        let pages = doc.get_pages();
        object.props.insert("page_count".into(), json!(pages.len()));

        let started = Instant::now();
        let mut reached = 0usize;
        let mut with_text = 0usize;
        let mut failed = 0usize;
        let mut panicked = 0usize;
        for (n, page_number) in pages.keys().enumerate() {
            if started.elapsed() > self.budget {
                warnings.push(format!(
                    "stopped after {n} of {} pages: the {} s budget for text is spent",
                    pages.len(),
                    self.budget.as_secs()
                ));
                break;
            }
            reached += 1;
            // One page's panic in the parser costs that page, not the rest (see `extract`).
            let result = catch_unwind(AssertUnwindSafe(|| {
                let mut text = String::new();
                let mut output = PlainTextOutput::new(&mut text);
                output_doc_page(doc, &mut output, *page_number).map(|()| text)
            }));
            let text = match result {
                Ok(Ok(text)) => text,
                Ok(Err(_)) => {
                    failed += 1;
                    continue;
                }
                Err(_) => {
                    panicked += 1;
                    continue;
                }
            };
            let paragraphs: Vec<String> = text
                .split("\n\n")
                .map(|p| p.split_whitespace().collect::<Vec<_>>().join(" "))
                .filter(|p| !p.is_empty())
                .collect();
            if paragraphs.is_empty() {
                continue;
            }
            with_text += 1;
            object
                .blocks
                .push(BlockDraft::heading(2, &format!("Page {page_number}")));
            object
                .blocks
                .extend(paragraphs.iter().map(|p| BlockDraft::paragraph(p)));
        }
        if failed > 0 {
            warnings.push(format!("{failed} page(s) could not be read for text"));
        }
        if panicked > 0 {
            warnings.push(format!(
                "{panicked} page(s) made the PDF parser fail; their text was left out"
            ));
        }
        if with_text == 0 && reached > 0 {
            warnings.push(format!(
                "no text in {reached} page(s): a scan, or pictures only"
            ));
            object.provenance.confidence = 0.5;
        } else {
            object.provenance.confidence = 0.9;
        }
    }
}

/// A text string of the Info dictionary, decoded: UTF-16BE behind a byte-order mark, else one
/// byte per character (PDFDocEncoding is Latin-1 for the characters that matter here).
fn info_text(doc: &PdfDocument, key: &[u8]) -> Option<String> {
    let info = doc.trailer.get(b"Info").ok()?;
    let (_, info) = doc.dereference(info).ok()?;
    let value = info.as_dict().ok()?.get(key).ok()?;
    let (_, value) = doc.dereference(value).ok()?;
    let bytes = match value {
        Object::String(bytes, _) => bytes.as_slice(),
        _ => return None,
    };
    Some(decode_pdf_text(bytes))
}

/// The text of a PDF string object.
pub fn decode_pdf_text(bytes: &[u8]) -> String {
    let text = if let Some(utf16) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        let units: Vec<u16> = utf16
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_be_bytes(*c))
            .collect();
        String::from_utf16_lossy(&units)
    } else {
        bytes.iter().map(|&b| char::from(b)).collect()
    };
    text.trim().to_owned()
}

#[cfg(test)]
mod tests {
    use super::decode_pdf_text;

    #[test]
    fn pdf_strings_decode_both_ways() {
        assert_eq!(decode_pdf_text(b"  Plain title "), "Plain title");
        assert_eq!(decode_pdf_text(b"caf\xe9"), "café");
        assert_eq!(
            decode_pdf_text(&[0xFE, 0xFF, 0x00, 0x48, 0x00, 0x69, 0x20, 0xAC]),
            "Hi€"
        );
        assert_eq!(
            decode_pdf_text(&[0xFE, 0xFF, 0x00]),
            "",
            "an odd tail is dropped"
        );
    }
}
