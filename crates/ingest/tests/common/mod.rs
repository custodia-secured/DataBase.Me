// SPDX-License-Identifier: AGPL-3.0-or-later
//! Helpers shared by the ingest crate's tests: a stored zip, enough for an .xlsx, and the
//! two-sheet workbook the gate and the importer tests read.

#![allow(dead_code, reason = "each test binary uses a different subset")]

use std::path::PathBuf;

/// `fixtures/<rel>` in the repository.
pub fn fixture(rel: &str) -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures")).join(rel)
}

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
pub fn zip_store(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, data) in entries {
        let offset = out.len() as u32;
        let crc = crc32(data);
        let size = data.len() as u32;
        let name_len = name.len() as u16;
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
/// third row; `Totals` with one number. Also `fixtures/xlsx/people.xlsx`.
pub fn workbook() -> Vec<u8> {
    let content_types = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">"#,
        r#"<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>"#,
        r#"<Default Extension="xml" ContentType="application/xml"/>"#,
        r#"<Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>"#,
        r#"<Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>"#,
        r#"<Override PartName="/xl/worksheets/sheet2.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>"#,
        r#"<Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/>"#,
        "</Types>"
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
