// SPDX-License-Identifier: AGPL-3.0-or-later
//! Gate 1c (M6): what `pdf-extract` 0.12 does with a text PDF, a scanned (image-only) PDF, a
//! truncated file, a file whose page tree refers to itself, and a page tree that multiplies a
//! few objects into a million pages. Every PDF is built here from a handful of objects, so the
//! fixtures are ours and under 2 KB each. Each case runs on its own thread under a time cap, so
//! a hang is a recorded outcome and not a stuck test; a panic on the thread is recorded too.
//! The gate's claims: text or `Err`, never a panic, under the caps (M6.md Design).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "gate probes: a failure here is the finding"
)]

use std::fmt::Debug;
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// The extraction budget the importer will hold a PDF to (M6.md Design: 10 s).
const CAP: Duration = Duration::from_secs(10);

/// How a probe ended.
#[derive(Debug)]
enum Outcome<T> {
    Done(T, Duration),
    Panicked,
    TimedOut,
}

/// Runs `work` on its own thread and waits at most `CAP` for it.
fn probe<T: Send + Debug + 'static>(
    label: &str,
    work: impl FnOnce() -> T + Send + 'static,
) -> Outcome<T> {
    let (tx, rx) = mpsc::channel();
    let started = Instant::now();
    std::thread::spawn(move || {
        let result = work();
        // The receiver may have given up; nothing to do then.
        let _ = tx.send(result);
    });
    let outcome = match rx.recv_timeout(CAP) {
        Ok(result) => Outcome::Done(result, started.elapsed()),
        Err(mpsc::RecvTimeoutError::Disconnected) => Outcome::Panicked,
        Err(mpsc::RecvTimeoutError::Timeout) => Outcome::TimedOut,
    };
    println!("{label}: {outcome:?}");
    outcome
}

// ---- hand-made PDFs ---------------------------------------------------------------------------

/// A PDF from numbered objects (object 1 is the catalog) with a correct xref table.
fn pdf(objects: &[Vec<u8>]) -> Vec<u8> {
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for offset in offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}

fn obj(text: &str) -> Vec<u8> {
    text.as_bytes().to_vec()
}

fn stream(dict: &str, data: &[u8]) -> Vec<u8> {
    let mut out = format!("<< {dict} /Length {} >>\nstream\n", data.len()).into_bytes();
    out.extend_from_slice(data);
    out.extend_from_slice(b"\nendstream");
    out
}

fn page(parent: u32, contents: u32, resources: &str) -> Vec<u8> {
    obj(&format!(
        "<< /Type /Page /Parent {parent} 0 R /MediaBox [0 0 612 792] /Contents {contents} 0 R /Resources {resources} >>"
    ))
}

fn text_content(text: &str) -> Vec<u8> {
    stream(
        "",
        format!("BT /F1 24 Tf 72 700 Td ({text}) Tj ET").as_bytes(),
    )
}

const HELVETICA: &str =
    "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>";
const FONTS: &str = "<< /Font << /F1 5 0 R >> >>";

/// Two pages of Helvetica text.
fn text_pdf() -> Vec<u8> {
    pdf(&[
        obj("<< /Type /Catalog /Pages 2 0 R >>"),
        obj("<< /Type /Pages /Kids [3 0 R 6 0 R] /Count 2 >>"),
        page(2, 4, FONTS),
        text_content("Hello gate one"),
        obj(HELVETICA),
        page(2, 7, FONTS),
        text_content("Page two here"),
    ])
}

/// One page that draws a 1x1 grey image and no text, as a scan does.
fn scanned_pdf() -> Vec<u8> {
    pdf(&[
        obj("<< /Type /Catalog /Pages 2 0 R >>"),
        obj("<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        page(2, 4, "<< /XObject << /Im1 5 0 R >> >>"),
        stream("", b"q 300 0 0 300 100 400 cm /Im1 Do Q"),
        stream(
            "/Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8",
            &[0x80],
        ),
    ])
}

/// A page tree whose root lists itself among its kids, and a page whose parents form a loop.
fn recursive_pdf() -> Vec<u8> {
    pdf(&[
        obj("<< /Type /Catalog /Pages 2 0 R >>"),
        obj("<< /Type /Pages /Kids [2 0 R 3 0 R] /Count 2 >>"),
        obj("<< /Type /Page /Parent 6 0 R /MediaBox [0 0 612 792] /Contents 4 0 R >>"),
        text_content("Loop"),
        obj(HELVETICA),
        obj(
            "<< /Type /Pages /Parent 3 0 R /Kids [3 0 R] /Count 1 /Resources << /Font << /F1 5 0 R >> >> >>",
        ),
    ])
}

/// Three levels of a hundred references each: a million pages declared by a 10 KB file.
fn page_tree_bomb() -> Vec<u8> {
    let kids = |n: u32| format!("[{}]", format!("{n} 0 R ").repeat(100));
    pdf(&[
        obj("<< /Type /Catalog /Pages 2 0 R >>"),
        obj(&format!(
            "<< /Type /Pages /Kids {} /Count 1000000 >>",
            kids(6)
        )),
        page(7, 4, FONTS),
        text_content("Boom"),
        obj(HELVETICA),
        obj(&format!(
            "<< /Type /Pages /Parent 2 0 R /Kids {} /Count 10000 >>",
            kids(7)
        )),
        obj(&format!(
            "<< /Type /Pages /Parent 6 0 R /Kids {} /Count 100 >>",
            kids(3)
        )),
    ])
}

// ---- the probes -------------------------------------------------------------------------------

// Probe 1: a text PDF gives its text, whole and per page.
#[test]
fn a_text_pdf_gives_its_text_whole_and_by_page() {
    let bytes = text_pdf();
    let Outcome::Done(whole, took) = probe("text, whole", move || {
        pdf_extract::extract_text_from_mem(&bytes).map_err(|e| e.to_string())
    }) else {
        panic!("the text PDF must extract");
    };
    let whole = whole.expect("Ok");
    assert!(whole.contains("Hello gate one"), "{whole:?}");
    assert!(whole.contains("Page two here"), "{whole:?}");
    assert!(took < Duration::from_secs(2), "{took:?}");

    let bytes = text_pdf();
    let Outcome::Done(pages, _) = probe("text, by page", move || {
        pdf_extract::extract_text_from_mem_by_pages(&bytes).map_err(|e| e.to_string())
    }) else {
        panic!("the text PDF must extract by page");
    };
    let pages = pages.expect("Ok");
    assert_eq!(pages.len(), 2);
    assert!(pages[0].contains("Hello gate one") && !pages[0].contains("Page two"));
    assert!(pages[1].contains("Page two here"));
}

// Probe 2: a scanned PDF gives no text and no error, so the importer knows to attach it plain.
#[test]
fn a_scanned_pdf_gives_empty_text() {
    let bytes = scanned_pdf();
    let Outcome::Done(text, _) = probe("scanned", move || {
        pdf_extract::extract_text_from_mem(&bytes).map_err(|e| e.to_string())
    }) else {
        panic!("the scanned PDF must not hang or panic");
    };
    let text = text.expect("Ok");
    assert!(text.trim().is_empty(), "{text:?}");
}

// Probe 3: a file cut in the middle: text or an error, never a panic or a hang.
#[test]
fn a_truncated_pdf_is_text_or_an_error() {
    let whole = text_pdf();
    for fraction in [0.9, 0.5, 0.2] {
        let cut = (whole.len() as f64 * fraction) as usize;
        let bytes = whole[..cut].to_vec();
        let outcome = probe(&format!("truncated at {fraction}"), move || {
            pdf_extract::extract_text_from_mem(&bytes).map_err(|e| e.to_string())
        });
        assert!(matches!(outcome, Outcome::Done(..)), "{outcome:?}");
    }
    let outcome = probe("not a PDF at all", || {
        pdf_extract::extract_text_from_mem(b"hello, this is text").map_err(|e| e.to_string())
    });
    assert!(matches!(outcome, Outcome::Done(Err(_), _)), "{outcome:?}");
}

// Probe 4: a self-referencing page tree ends, one way or another, inside the cap.
#[test]
fn a_recursive_page_tree_ends_inside_the_cap() {
    let bytes = recursive_pdf();
    let outcome = probe("recursive page tree", move || {
        pdf_extract::extract_text_from_mem(&bytes).map_err(|e| e.to_string())
    });
    assert!(matches!(outcome, Outcome::Done(..)), "{outcome:?}");
}

// Probe 5: a million declared pages from 10 KB: what it costs, and whether the cap holds.
#[test]
fn a_page_tree_bomb_is_bounded() {
    let bytes = page_tree_bomb();
    println!("bomb size: {} bytes", bytes.len());
    let outcome = probe("page tree bomb", move || {
        pdf_extract::extract_text_from_mem_by_pages(&bytes)
            .map(|pages| (pages.len(), pages.iter().map(String::len).sum::<usize>()))
            .map_err(|e| e.to_string())
    });
    assert!(!matches!(outcome, Outcome::Panicked), "{outcome:?}");
}
