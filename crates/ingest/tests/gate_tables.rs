// SPDX-License-Identifier: AGPL-3.0-or-later
//! Gate 1b (M6): what `calamine` 0.38 and `csv` 1.4 report for the table shapes the importers
//! have to recognise. The workbook is built here, byte by byte (a stored zip of the OOXML parts),
//! so the fixture is ours and under 10 KB; the CSVs are strings. Each test asserts what held on
//! the machine that ran it; both crates are pure Rust, so the other systems see the same.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "gate probes: a failure here is the finding"
)]

use std::io::Cursor;

use calamine::{Data, Reader, Xlsx, open_workbook_from_rs};

mod common;
use common::{workbook, zip_store};

fn open() -> Xlsx<Cursor<Vec<u8>>> {
    open_workbook_from_rs(Cursor::new(workbook())).expect("the hand-made workbook opens")
}

// Probe 1: the two sheets, in order, by name.
#[test]
fn calamine_lists_both_sheets_in_order() {
    let book = open();
    assert_eq!(book.sheet_names(), ["People", "Totals"]);
}

// Probe 2: the cell kinds: inline strings, a date by its style, floats, the formula's cached
// value, the empty column and the ragged row padded with `Empty`.
#[test]
fn calamine_reports_dates_numbers_formula_values_and_empties() {
    let mut book = open();
    let range = book.worksheet_range("People").unwrap();
    assert_eq!((range.height(), range.width()), (4, 5), "4 rows, 5 columns");
    let rows: Vec<&[Data]> = range.rows().collect();
    println!("People:");
    for row in &rows {
        println!("  {row:?}");
    }
    assert_eq!(rows[0][0], Data::String("Name".into()));
    assert_eq!(rows[0][4], Data::String("Empty".into()));

    // The date cell: calamine reads the style's number format and reports a DateTime.
    let Data::DateTime(when) = &rows[1][1] else {
        panic!("B2 is a date by style 14, got {:?}", rows[1][1]);
    };
    assert!(when.is_datetime());
    assert!(
        (when.as_f64() - 45992.0).abs() < f64::EPSILON,
        "the serial is kept"
    );
    let Data::DateTime(when) = &rows[2][1] else {
        panic!("B3 is a date, got {:?}", rows[2][1]);
    };
    assert!(
        (when.as_f64() - 45993.5).abs() < f64::EPSILON,
        "fractional day kept"
    );

    assert_eq!(rows[1][2], Data::Float(12.5));
    assert_eq!(
        rows[2][2],
        Data::Float(3.0),
        "an integer-valued number is a Float"
    );
    assert_eq!(rows[1][3], Data::Float(25.0), "the formula's cached value");
    assert_eq!(rows[1][4], Data::Empty, "the empty column");
    assert_eq!(rows[3][0], Data::String("Cy".into()));
    assert!(
        rows[3][1..].iter().all(|c| *c == Data::Empty),
        "the ragged row is padded with Empty"
    );
}

// Probe 3: formulas are read apart from values.
#[test]
fn calamine_reads_the_formula_text_separately() {
    let mut book = open();
    let formulas = book.worksheet_formula("People").unwrap();
    let texts: Vec<&String> = formulas.cells().map(|(_, _, f)| f).collect();
    println!("formulas: {texts:?}");
    assert!(texts.iter().any(|f| f.as_str() == "C2*2"));
    assert!(texts.iter().any(|f| f.as_str() == "C3*2"));
    let totals = book.worksheet_range("Totals").unwrap();
    assert_eq!(totals.get((1, 0)), Some(&Data::Float(42.0)));
}

// Probe 4: a sheet that does not exist is an error, not a panic.
#[test]
fn calamine_refuses_an_unknown_sheet() {
    let mut book = open();
    assert!(book.worksheet_range("Nope").is_err());
}

// Probe 5: a zip that is not a workbook is an error, not a panic.
#[test]
fn calamine_refuses_a_zip_without_a_workbook() {
    let bytes = zip_store(&[("hello.txt", b"not a workbook")]);
    let result = open_workbook_from_rs::<Xlsx<_>, _>(Cursor::new(bytes));
    println!("zip without a workbook: {:?}", result.as_ref().err());
    assert!(result.is_err());
    let result = open_workbook_from_rs::<Xlsx<_>, _>(Cursor::new(b"PK\x03\x04garbage".to_vec()));
    assert!(result.is_err());
}

// ---- csv ------------------------------------------------------------------------------------

// Probe 6: a UTF-8 BOM before the header, and a quoted cell with an embedded newline.
#[test]
fn csv_strips_the_bom_and_keeps_an_embedded_newline() {
    let text = "\u{FEFF}name,note\nAda,\"line one\nline two\"\n";
    let mut reader = csv::Reader::from_reader(text.as_bytes());
    let headers = reader.headers().unwrap().clone();
    println!("headers: {headers:?}");
    assert_eq!(
        headers.get(0),
        Some("name"),
        "the BOM is not part of the first header"
    );
    let records: Vec<csv::StringRecord> = reader.records().map(Result::unwrap).collect();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].get(1), Some("line one\nline two"));
}

// Probe 7: a ragged row is an error by default and a record when `flexible`.
#[test]
fn csv_ragged_rows_are_an_error_unless_flexible() {
    let text = "a,b\n1,2,3\n4\n";
    let strict: Vec<Result<csv::StringRecord, csv::Error>> =
        csv::Reader::from_reader(text.as_bytes())
            .records()
            .collect();
    println!("strict: {strict:?}");
    assert!(strict[0].is_err(), "a longer row is refused");
    let flexible: Vec<csv::StringRecord> = csv::ReaderBuilder::new()
        .flexible(true)
        .from_reader(text.as_bytes())
        .records()
        .map(Result::unwrap)
        .collect();
    assert_eq!(flexible[0].len(), 3);
    assert_eq!(flexible[1].len(), 1);
}

// Probe 8: `;` and tab as delimiters, set by hand; the importer sniffs them (Task 4).
#[test]
fn csv_takes_semicolons_and_tabs_as_delimiters() {
    for (delimiter, text) in [(b';', "a;b\n1;2\n"), (b'\t', "a\tb\n1\t2\n")] {
        let mut reader = csv::ReaderBuilder::new()
            .delimiter(delimiter)
            .from_reader(text.as_bytes());
        let record = reader.records().next().unwrap().unwrap();
        assert_eq!(record.len(), 2);
        assert_eq!(record.get(1), Some("2"));
    }
}

// Probe 9: a 1 MiB cell is read whole; the cap is the importer's (Design: 1 MiB).
#[test]
fn csv_reads_a_one_mebibyte_cell() {
    let big = "x".repeat(1 << 20);
    let text = format!("a,b\n{big},2\n");
    let mut reader = csv::Reader::from_reader(text.as_bytes());
    let record = reader.records().next().unwrap().unwrap();
    assert_eq!(record.get(0).map(str::len), Some(1 << 20));
}

// Probe 10: bytes that are not UTF-8 are a `ByteRecord`, and a `StringRecord` error.
#[test]
fn csv_reports_invalid_utf8_instead_of_panicking() {
    let bytes = b"a,b\n\xff\xfe,2\n";
    let mut reader = csv::Reader::from_reader(&bytes[..]);
    let result = reader.records().next().unwrap();
    println!(
        "invalid UTF-8 as a StringRecord: {:?}",
        result.as_ref().err()
    );
    assert!(result.is_err());
    let mut reader = csv::Reader::from_reader(&bytes[..]);
    let record = reader.byte_records().next().unwrap().unwrap();
    assert_eq!(record.get(0), Some(&b"\xff\xfe"[..]));
}
