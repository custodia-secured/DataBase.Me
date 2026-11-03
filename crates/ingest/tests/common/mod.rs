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

// ---- fixtures on disk -------------------------------------------------------------------------

/// `fixtures/<rel>`, written from `bytes` when absent, and checked against them otherwise, so
/// the file in the repository is always the builder's output.
pub fn fixture_file(rel: &str, bytes: &[u8]) -> PathBuf {
    let path = fixture(rel);
    if !path.exists() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, bytes).unwrap();
    }
    assert_eq!(
        std::fs::read(&path).unwrap(),
        bytes,
        "{rel} is the builder's output"
    );
    path
}

// ---- PDFs -------------------------------------------------------------------------------------

/// A PDF from numbered objects (object 1 is the catalog) with a correct xref table, and an Info
/// dictionary when `info` is given.
pub fn pdf(objects: &[Vec<u8>], info: Option<&str>) -> Vec<u8> {
    let mut all: Vec<Vec<u8>> = objects.to_vec();
    if let Some(info) = info {
        all.push(info.as_bytes().to_vec());
    }
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in all.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", all.len() + 1).as_bytes());
    for offset in offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    let info_ref = if info.is_some() {
        format!(" /Info {} 0 R", all.len())
    } else {
        String::new()
    };
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R{info_ref} >>\nstartxref\n{xref}\n%%EOF\n",
            all.len() + 1
        )
        .as_bytes(),
    );
    out
}

pub fn obj(text: &str) -> Vec<u8> {
    text.as_bytes().to_vec()
}

pub fn stream(dict: &str, data: &[u8]) -> Vec<u8> {
    let mut out = format!("<< {dict} /Length {} >>\nstream\n", data.len()).into_bytes();
    out.extend_from_slice(data);
    out.extend_from_slice(b"\nendstream");
    out
}

pub fn page(parent: u32, contents: u32, resources: &str) -> Vec<u8> {
    obj(&format!(
        "<< /Type /Page /Parent {parent} 0 R /MediaBox [0 0 612 792] /Contents {contents} 0 R /Resources {resources} >>"
    ))
}

pub fn text_content(text: &str) -> Vec<u8> {
    stream(
        "",
        format!("BT /F1 24 Tf 72 700 Td ({text}) Tj ET").as_bytes(),
    )
}

pub const HELVETICA: &str =
    "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>";
pub const FONTS: &str = "<< /Font << /F1 5 0 R >> >>";

/// Two pages of Helvetica text, titled "Two pages" by "DataBase.Me tests".
pub fn two_pages_pdf() -> Vec<u8> {
    pdf(
        &[
            obj("<< /Type /Catalog /Pages 2 0 R >>"),
            obj("<< /Type /Pages /Kids [3 0 R 6 0 R] /Count 2 >>"),
            page(2, 4, FONTS),
            text_content("Hello gate one"),
            obj(HELVETICA),
            page(2, 7, FONTS),
            text_content("Page two here"),
        ],
        Some("<< /Title (Two pages) /Author (DataBase.Me tests) >>"),
    )
}

/// One page that draws a 1x1 grey image and no text, as a scan does.
pub fn scanned_pdf() -> Vec<u8> {
    pdf(
        &[
            obj("<< /Type /Catalog /Pages 2 0 R >>"),
            obj("<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
            page(2, 4, "<< /XObject << /Im1 5 0 R >> >>"),
            stream("", b"q 300 0 0 300 100 400 cm /Im1 Do Q"),
            stream(
                "/Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8",
                &[0x80],
            ),
        ],
        None,
    )
}

// ---- pictures ---------------------------------------------------------------------------------

fn be32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_be_bytes());
}

fn png_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    let mut body = kind.to_vec();
    body.extend_from_slice(data);
    be32(out, data.len() as u32);
    out.extend_from_slice(&body);
    be32(out, crc32(&body));
}

/// A PNG whose header claims `width` x `height` 8-bit grey, with an empty IDAT and an IEND.
pub fn png_header_only(width: u32, height: u32) -> Vec<u8> {
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut ihdr = Vec::new();
    be32(&mut ihdr, width);
    be32(&mut ihdr, height);
    ihdr.extend_from_slice(&[8, 0, 0, 0, 0]);
    png_chunk(&mut out, b"IHDR", &ihdr);
    png_chunk(&mut out, b"IDAT", &[]);
    png_chunk(&mut out, b"IEND", &[]);
    out
}

fn entry(out: &mut Vec<u8>, tag: u16, kind: u16, count: u32, value: u32) {
    le16(out, tag);
    le16(out, kind);
    le32(out, count);
    le32(out, value);
}

/// A little-endian TIFF with make "Dbme", model "Cam 1", orientation 6, DateTimeOriginal
/// 2025:11:03 10:20:30, and 40°26'46.2" N, 79°58'56" W.
pub fn exif_tiff() -> Vec<u8> {
    let mut t = b"II".to_vec();
    le16(&mut t, 42);
    le32(&mut t, 8);
    // IFD0 at 8, five entries: ends at 74.
    le16(&mut t, 5);
    entry(&mut t, 0x010F, 2, 5, 146);
    entry(&mut t, 0x0110, 2, 6, 152);
    entry(&mut t, 0x0112, 3, 1, 6);
    entry(&mut t, 0x8769, 4, 1, 74);
    entry(&mut t, 0x8825, 4, 1, 92);
    le32(&mut t, 0);
    // Exif IFD at 74, one entry: ends at 92.
    le16(&mut t, 1);
    entry(&mut t, 0x9003, 2, 20, 158);
    le32(&mut t, 0);
    // GPS IFD at 92, four entries: ends at 146.
    le16(&mut t, 4);
    entry(&mut t, 0x0001, 2, 2, u32::from_le_bytes(*b"N\0\0\0"));
    entry(&mut t, 0x0002, 5, 3, 178);
    entry(&mut t, 0x0003, 2, 2, u32::from_le_bytes(*b"W\0\0\0"));
    entry(&mut t, 0x0004, 5, 3, 202);
    le32(&mut t, 0);
    assert_eq!(t.len(), 146);
    t.extend_from_slice(b"Dbme\0\0");
    t.extend_from_slice(b"Cam 1\0");
    t.extend_from_slice(b"2025:11:03 10:20:30\0");
    assert_eq!(t.len(), 178);
    for (num, den) in [(40, 1), (26, 1), (462, 10), (79, 1), (58, 1), (56, 1)] {
        le32(&mut t, num);
        le32(&mut t, den);
    }
    assert_eq!(t.len(), 226);
    t
}

/// The APP1 segment carrying [`exif_tiff`], marker included.
pub fn exif_app1() -> Vec<u8> {
    let mut app1 = b"Exif\0\0".to_vec();
    app1.extend_from_slice(&exif_tiff());
    let mut out = vec![0xFF, 0xE1];
    out.extend_from_slice(&((app1.len() + 2) as u16).to_be_bytes());
    out.extend_from_slice(&app1);
    out
}

/// A 64x32 gradient JPEG with the EXIF segment right after SOI: a landscape photo that EXIF says
/// to rotate.
pub fn photo_jpeg() -> Vec<u8> {
    let mut picture = image::RgbImage::new(64, 32);
    for (x, y, pixel) in picture.enumerate_pixels_mut() {
        *pixel = image::Rgb([(x * 4) as u8, (y * 8) as u8, 128]);
    }
    let mut plain = Vec::new();
    image::DynamicImage::ImageRgb8(picture)
        .write_to(
            &mut std::io::Cursor::new(&mut plain),
            image::ImageFormat::Jpeg,
        )
        .unwrap();
    let mut out = plain[..2].to_vec();
    out.extend_from_slice(&exif_app1());
    out.extend_from_slice(&plain[2..]);
    out
}

/// A 1x1 PNG.
pub fn pixel_png() -> Vec<u8> {
    let mut out = Vec::new();
    image::DynamicImage::ImageRgb8(image::RgbImage::new(1, 1))
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .unwrap();
    out
}
