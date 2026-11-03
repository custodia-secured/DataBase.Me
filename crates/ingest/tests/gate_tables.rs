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

// ---- a stored zip, enough for an .xlsx ------------------------------------------------------

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in bytes {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

fn le16(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn le32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

/// A zip archive with every entry stored (method 0), which is all a reader needs.
fn zip_store(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, data) in entries {
        let offset = out.len() as u32;
        let crc = crc32(data);
        let size = data.len() as u32;
        let name_len = name.len() as u16;
        // Local file header.
        le32(&mut out, 0x0403_4B50);
        le16(&mut out, 20);
        le16(&mut out, 0);
        le16(&mut out, 0);
        le16(&mut out, 0);
        le16(&mut out, 0x21); // 1980-01-01
        le32(&mut out, crc);
        le32(&mut out, size);
        le32(&mut out, size);
        le16(&mut out, name_len);
        le16(&mut out, 0);
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(data);
        // Central directory entry.
        le32(&mut central, 0x0201_4B50);
        le16(&mut central, 20);
        le16(&mut central, 20);
        le16(&mut central, 0);
        le16(&mut central, 0);
        le16(&mut central, 0);
        le16(&mut central, 0x21);
        le32(&mut central, crc);
        le32(&mut central, size);
        le32(&mut central, size);
        le16(&mut central, name_len);
        le16(&mut central, 0);
        le16(&mut central, 0);
        le16(&mut central, 0);
        le16(&mut central, 0);
        le32(&mut central, 0);
        le32(&mut central, offset);
        central.extend_from_slice(name.as_bytes());
    }
    let central_offset = out.len() as u32;
    let central_size = central.len() as u32;
    out.extend_from_slice(&central);
    le32(&mut out, 0x0605_4B50);
    le16(&mut out, 0);
    le16(&mut out, 0);
    le16(&mut out, entries.len() as u16);
    le16(&mut out, entries.len() as u16);
    le32(&mut out, central_size);
    le32(&mut out, central_offset);
    le16(&mut out, 0);
    out
}

const NS_MAIN: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const NS_REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const NS_PKG_REL: &str = "http://schemas.openxmlformats.org/package/2006/relationships";

fn inline(cell: &str, text: &str) -> String {
    format!(r#"<c r="{cell}" t="inlineStr"><is><t>{text}</t></is></c>"#)
}

/// Two sheets: `People` with a header row, two full rows (a date cell styled with the built-in
/// date format 14, a number, a formula with its cached value, an empty column) and a ragged
/// third row; `Totals` with one number.
fn workbook() -> Vec<u8> {
    let content_types = format!(
        concat!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
            r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">"#,
            r#"<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>"#,
            r#"<Default Extension="xml" ContentType="application/xml"/>"#,
            r#"<Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>"#,
            r#"<Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>"#,
            r#"<Override PartName="/xl/worksheets/sheet2.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>"#,
            r#"<Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/>"#,
            "</Types>{}"
        ),
        ""
    );
    let rels = format!(
        r#"<Relationships xmlns="{NS_PKG_REL}"><Relationship Id="rId1" Type="{NS_REL}/officeDocument" Target="xl/workbook.xml"/></Relationships>"#
    );
    let workbook = format!(
        r#"<workbook xmlns="{NS_MAIN}" xmlns:r="{NS_REL}"><sheets><sheet name="People" sheetId="1" r:id="rId1"/><sheet name="Totals" sheetId="2" r:id="rId2"/></sheets></workbook>"#
    );
    let workbook_rels = format!(
        concat!(
            r#"<Relationships xmlns="{ns}">"#,
            r#"<Relationship Id="rId1" Type="{rel}/worksheet" Target="worksheets/sheet1.xml"/>"#,
            r#"<Relationship Id="rId2" Type="{rel}/worksheet" Target="worksheets/sheet2.xml"/>"#,
            r#"<Relationship Id="rId3" Type="{rel}/styles" Target="styles.xml"/>"#,
            "</Relationships>"
        ),
        ns = NS_PKG_REL,
        rel = NS_REL
    );
    let styles = format!(
        concat!(
            r#"<styleSheet xmlns="{ns}"><fonts count="1"><font/></fonts><fills count="1"><fill/></fills>"#,
            r#"<borders count="1"><border/></borders><cellStyleXfs count="1"><xf numFmtId="0"/></cellStyleXfs>"#,
            r#"<cellXfs count="2"><xf numFmtId="0" xfId="0"/><xf numFmtId="14" xfId="0" applyNumberFormat="1"/></cellXfs>"#,
            "</styleSheet>"
        ),
        ns = NS_MAIN
    );
    let sheet1 = format!(
        concat!(
            r#"<worksheet xmlns="{ns}"><sheetData>"#,
            r#"<row r="1">{h1}{h2}{h3}{h4}{h5}</row>"#,
            r#"<row r="2">{a2}<c r="B2" s="1"><v>45992</v></c><c r="C2"><v>12.5</v></c><c r="D2"><f>C2*2</f><v>25</v></c></row>"#,
            r#"<row r="3">{a3}<c r="B3" s="1"><v>45993.5</v></c><c r="C3"><v>3</v></c><c r="D3"><f>C3*2</f><v>6</v></c></row>"#,
            r#"<row r="4">{a4}</row>"#,
            "</sheetData></worksheet>"
        ),
        ns = NS_MAIN,
        h1 = inline("A1", "Name"),
        h2 = inline("B1", "When"),
        h3 = inline("C1", "Amount"),
        h4 = inline("D1", "Total"),
        h5 = inline("E1", "Empty"),
        a2 = inline("A2", "Ada"),
        a3 = inline("A3", "Bob"),
        a4 = inline("A4", "Cy"),
    );
    let sheet2 = format!(
        r#"<worksheet xmlns="{NS_MAIN}"><sheetData><row r="1">{}</row><row r="2"><c r="A2"><v>42</v></c></row></sheetData></worksheet>"#,
        inline("A1", "Sum")
    );
    zip_store(&[
        ("[Content_Types].xml", content_types.as_bytes()),
        ("_rels/.rels", rels.as_bytes()),
        ("xl/workbook.xml", workbook.as_bytes()),
        ("xl/_rels/workbook.xml.rels", workbook_rels.as_bytes()),
        ("xl/styles.xml", styles.as_bytes()),
        ("xl/worksheets/sheet1.xml", sheet1.as_bytes()),
        ("xl/worksheets/sheet2.xml", sheet2.as_bytes()),
    ])
}

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
