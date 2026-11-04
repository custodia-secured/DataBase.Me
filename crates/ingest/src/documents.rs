// SPDX-License-Identifier: AGPL-3.0-or-later
//! PDF text (M6 Task 5, ADR-0026): a PDF becomes a `Document` with the file attached, its text
//! page by page as paragraph blocks under a heading per page, and the page count and author as
//! fields; the document's own title, when it names one, is the object's. A file the parser
//! refuses, or one over [`MAX_PDF_BYTES`], is still imported, as the plain file with a warning
//! and a lower confidence: never dropped. Extraction stops at [`PDF_BUDGET`], checked between
//! pages, and says how many pages it reached. The parser runs on a thread of its own, because it
//! can get stuck inside one call on a hostile file, where that check never runs: past
//! [`PARSER_GRACE`] more it is left behind and the file is attached without its text.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
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
/// How long past the budget the parser may run before it is left behind: time to load the file,
/// and to finish the page it is on when the budget runs out.
pub const PARSER_GRACE: Duration = Duration::from_secs(10);
/// How many parsers may be stuck before PDFs are attached without trying. A thread cannot be
/// stopped from outside, so each one holds a core until the app closes, and a folder of hostile
/// files must not take them all.
const MAX_STUCK_PARSERS: usize = 2;
/// The parser's stack: a Linux main thread's, more than a spawned thread gets by default, since a
/// hostile file can nest deeply.
const PARSER_STACK_BYTES: usize = 8 << 20;
/// The parsers still running past their wait.
static STUCK_PARSERS: AtomicUsize = AtomicUsize::new(0);

/// What came of running a parser on its own thread.
#[derive(Debug, PartialEq, Eq)]
enum Bounded<T> {
    /// It finished in time.
    Done(T),
    /// It is still running, and has been left behind.
    Stuck,
    /// Too many are stuck already, so it was not started.
    Refused,
    /// The thread could not start, or died without an answer.
    Failed,
}

/// Runs `work` on its own thread and waits for it for `wait` at most. `stuck` counts the threads
/// left behind, and each takes itself off the count if it ever finishes.
fn bounded<T: Send + 'static>(
    stuck: &'static AtomicUsize,
    wait: Duration,
    work: impl FnOnce() -> T + Send + 'static,
) -> Bounded<T> {
    if stuck.load(Ordering::SeqCst) >= MAX_STUCK_PARSERS {
        return Bounded::Refused;
    }
    let (sender, receiver) = mpsc::channel();
    // Whichever side gets here second sees `true`: the wait running out and the work finishing
    // can race, and the count must not move unless the wait ran out first.
    let other_is_done = Arc::new(AtomicBool::new(false));
    let worker_sees = Arc::clone(&other_is_done);
    let spawned = thread::Builder::new()
        .name("pdf-text".into())
        .stack_size(PARSER_STACK_BYTES)
        .spawn(move || {
            // Nobody is listening once the wait has run out, and that is fine.
            let _unheard = sender.send(work());
            if worker_sees.swap(true, Ordering::SeqCst) {
                stuck.fetch_sub(1, Ordering::SeqCst);
            }
        });
    if spawned.is_err() {
        return Bounded::Failed;
    }
    match receiver.recv_timeout(wait) {
        Ok(done) => Bounded::Done(done),
        Err(RecvTimeoutError::Disconnected) => Bounded::Failed,
        Err(RecvTimeoutError::Timeout) => {
            if !other_is_done.swap(true, Ordering::SeqCst) {
                stuck.fetch_add(1, Ordering::SeqCst);
            }
            Bounded::Stuck
        }
    }
}

/// Puts what the parser's thread came to on the object: its text and warnings when it finished,
/// and otherwise the reason there is no text.
fn settle(
    outcome: Bounded<(ExtractedObject, Vec<String>)>,
    wait: Duration,
    object: &mut ExtractedObject,
    warnings: &mut Vec<String>,
) {
    let problem = match outcome {
        Bounded::Done((parsed, found)) => {
            *object = parsed;
            warnings.extend(found);
            return;
        }
        Bounded::Stuck => format!(
            "the PDF parser did not finish in {} s: attached without its text",
            wait.as_secs()
        ),
        Bounded::Refused => {
            "the PDF parser is stuck on an earlier file until the app closes: attached without its \
             text"
                .to_owned()
        }
        Bounded::Failed => "the PDF parser failed on this file: attached without its text".into(),
    };
    warnings.push(problem);
    object.provenance.confidence = 0.5;
}

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
            let importer = *self;
            let bytes = bytes.to_vec();
            let mut parsed = object.clone();
            let wait = self.budget.saturating_add(PARSER_GRACE);
            let outcome = bounded(&STUCK_PARSERS, wait, move || {
                let mut found = Vec::new();
                importer.parse(&bytes, &mut parsed, &mut found);
                (parsed, found)
            });
            settle(outcome, wait, &mut object, &mut extract.warnings);
        }
        extract.objects.push(object);
        extract
    }

    /// The file's text onto `object`, on the parser's thread.
    fn parse(&self, bytes: &[u8], object: &mut ExtractedObject, warnings: &mut Vec<String>) {
        // The parser panics on some malformed files (found by the proptest layer, M6 Task 9:
        // a failed assertion on an operator's operands, unwraps on missing objects). A panic
        // is contained here, so one bad file costs its text and not the import.
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            PdfDocument::load_mem(bytes).map(|doc| self.extract_pages(&doc, object, warnings))
        }));
        match outcome {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                warnings.push(format!(
                    "not a PDF the parser takes ({e}): attached without its text"
                ));
                object.provenance.confidence = 0.5;
            }
            Err(_) => {
                warnings
                    .push("the PDF parser failed on this file: attached without its text".into());
                object.blocks.clear();
                object.provenance.confidence = 0.5;
            }
        }
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
    #![allow(clippy::unwrap_used, reason = "tests")]

    use super::*;

    const WAIT: Duration = Duration::from_millis(20);

    /// A worker that holds until its sender is dropped.
    fn held() -> (mpsc::Sender<()>, impl FnOnce() -> u8 + Send + 'static) {
        let (release, wait_for) = mpsc::channel::<()>();
        (release, move || {
            let _released = wait_for.recv();
            7
        })
    }

    #[test]
    fn stuck_parsers_are_left_behind_up_to_the_limit_and_counted_until_they_finish() {
        static STUCK: AtomicUsize = AtomicUsize::new(0);
        let (first, work) = held();
        assert_eq!(bounded(&STUCK, WAIT, work), Bounded::Stuck);
        let (second, work) = held();
        assert_eq!(bounded(&STUCK, WAIT, work), Bounded::Stuck);
        assert_eq!(STUCK.load(Ordering::SeqCst), MAX_STUCK_PARSERS);

        let (_third, work) = held();
        assert_eq!(bounded(&STUCK, WAIT, work), Bounded::Refused);

        drop(first);
        drop(second);
        let deadline = Instant::now() + Duration::from_secs(10);
        while STUCK.load(Ordering::SeqCst) > 0 {
            assert!(Instant::now() < deadline, "they never came off the count");
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(bounded(&STUCK, WAIT * 100, || 7), Bounded::Done(7));
        assert_eq!(
            STUCK.load(Ordering::SeqCst),
            0,
            "a finished one never counts"
        );
    }

    #[test]
    fn a_parser_thread_that_dies_is_a_failure_and_not_a_wait() {
        static STUCK: AtomicUsize = AtomicUsize::new(0);
        let started = Instant::now();
        let outcome: Bounded<u8> = bounded(&STUCK, Duration::from_secs(60), || {
            std::panic::resume_unwind(Box::new("the thread dies"))
        });
        assert_eq!(outcome, Bounded::Failed);
        assert!(started.elapsed() < Duration::from_secs(30));
        assert_eq!(STUCK.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_parser_that_does_not_finish_costs_the_text_and_says_so() {
        let source = Source::in_memory("t.pdf");
        let draft =
            || ExtractedObject::new("t", "Document", ProvenanceDraft::from_source(&source, None));
        let wait = Duration::from_secs(20);
        for (outcome, expected) in [
            (Bounded::Stuck, "did not finish in 20 s"),
            (Bounded::Refused, "stuck on an earlier file"),
            (Bounded::Failed, "failed on this file"),
        ] {
            let mut object = draft();
            let mut warnings = Vec::new();
            settle(outcome, wait, &mut object, &mut warnings);
            assert!(warnings[0].contains(expected), "{warnings:?}");
            assert!(warnings[0].contains("attached without its text"));
            assert!(object.blocks.is_empty());
            assert!((object.provenance.confidence - 0.5).abs() < f64::EPSILON);
        }

        let mut parsed = draft();
        parsed.title = "Parsed".into();
        let mut object = draft();
        let mut warnings = vec!["kept".to_owned()];
        settle(
            Bounded::Done((parsed, vec!["found".to_owned()])),
            wait,
            &mut object,
            &mut warnings,
        );
        assert_eq!(object.title, "Parsed");
        assert_eq!(warnings, ["kept", "found"]);
    }

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
