// SPDX-License-Identifier: AGPL-3.0-or-later
//! Gate 1d (M6): `image` 0.25 under `Limits` on a 64-megapixel PNG and on a PNG whose header
//! claims a billion pixels; a JPEG thumbnail; `kamadak-exif` 0.6 on orientation and GPS. The
//! pictures are made here: the big PNG by the crate's own encoder, the bomb and the EXIF JPEG
//! byte by byte, so nothing is copied from anywhere.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "gate probes: a failure here is the finding"
)]

use std::io::Cursor;
use std::time::Instant;

use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::{DynamicImage, GrayImage, ImageError, ImageFormat, ImageReader, Limits, RgbImage};

/// The importer's limits (M6.md Design): 64 megapixels as a side bound, 256 MiB of allocation.
fn limits() -> Limits {
    let mut limits = Limits::default();
    limits.max_image_width = Some(16_384);
    limits.max_image_height = Some(16_384);
    limits.max_alloc = Some(256 << 20);
    limits
}

fn reader(bytes: Vec<u8>) -> ImageReader<Cursor<Vec<u8>>> {
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .unwrap();
    reader.limits(limits());
    reader
}

fn be32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_be_bytes());
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

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    let mut body = kind.to_vec();
    body.extend_from_slice(data);
    be32(out, data.len() as u32);
    out.extend_from_slice(&body);
    be32(out, crc32(&body));
}

/// A PNG whose header claims `width` x `height` 8-bit grey, with an empty IDAT and an IEND: the
/// header reads, and there is nothing to decode.
fn png_header_only(width: u32, height: u32) -> Vec<u8> {
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut ihdr = Vec::new();
    be32(&mut ihdr, width);
    be32(&mut ihdr, height);
    ihdr.extend_from_slice(&[8, 0, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &[]);
    chunk(&mut out, b"IEND", &[]);
    out
}

// Probe 1: a 64-megapixel grey PNG (8000 x 8000) decodes under the limits, and how long it takes.
#[test]
fn a_64_megapixel_png_decodes_under_the_limits() {
    let started = Instant::now();
    let mut bytes = Vec::new();
    let encoder =
        PngEncoder::new_with_quality(&mut bytes, CompressionType::Fast, FilterType::NoFilter);
    GrayImage::new(8000, 8000)
        .write_with_encoder(encoder)
        .unwrap();
    println!(
        "encoded 8000x8000 grey: {} bytes in {:?}",
        bytes.len(),
        started.elapsed()
    );

    let (w, h) = reader(bytes.clone()).into_dimensions().unwrap();
    assert_eq!(
        (w, h),
        (8000, 8000),
        "dimensions read from the header alone"
    );

    let started = Instant::now();
    let decoded = reader(bytes).decode().unwrap();
    println!("decoded in {:?}", started.elapsed());
    assert_eq!((decoded.width(), decoded.height()), (8000, 8000));
}

// Probe 2: a header claiming 100,000 x 100,000 is refused by the limits at the header read,
// before any allocation and before even the dimensions come back; a reader without limits
// reads the header alone, which is how the importer will say why it refused.
#[test]
fn a_png_bomb_is_refused_by_the_limits() {
    let bytes = png_header_only(100_000, 100_000);
    let refused = reader(bytes.clone())
        .into_dimensions()
        .expect_err("refused");
    println!("bomb, dimensions under limits: {refused}");
    assert!(matches!(refused, ImageError::Limits(_)), "{refused:?}");
    let (w, h) = ImageReader::new(Cursor::new(bytes.clone()))
        .with_guessed_format()
        .unwrap()
        .into_dimensions()
        .unwrap();
    assert_eq!(
        (w, h),
        (100_000, 100_000),
        "readable without limits, header only"
    );
    let error = reader(bytes).decode().expect_err("refused");
    println!("bomb, decode: {error}");
    assert!(matches!(error, ImageError::Limits(_)), "{error:?}");

    // Inside the side bounds and exactly at the allocation bound (16384 x 16384 grey is 256
    // MiB): `Limits` lets it through, and only the missing data stops it. So the importer's own
    // pixel count (64 megapixels, M6.md Design) is the guard that holds, not `max_alloc` alone.
    let bytes = png_header_only(16_384, 16_384);
    let error = reader(bytes).decode().expect_err("no data");
    println!("16384 square grey, 256 MiB exactly: {error}");
    assert!(
        !matches!(error, ImageError::Limits(_)),
        "passes the limits: {error:?}"
    );
    // One row more is over the side bound and refused by the limits.
    let bytes = png_header_only(16_384, 16_385);
    let error = reader(bytes).into_dimensions().expect_err("refused");
    assert!(matches!(error, ImageError::Limits(_)), "{error:?}");
}

// Probe 3: a JPEG round trip and a 256-pixel thumbnail.
#[test]
fn a_jpeg_round_trips_and_shrinks_to_a_thumbnail() {
    let mut picture = RgbImage::new(640, 480);
    for (x, y, pixel) in picture.enumerate_pixels_mut() {
        *pixel = image::Rgb([(x % 256) as u8, (y % 256) as u8, 128]);
    }
    let mut bytes = Vec::new();
    DynamicImage::ImageRgb8(picture)
        .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Jpeg)
        .unwrap();
    println!("640x480 JPEG: {} bytes", bytes.len());
    let decoded = reader(bytes).decode().unwrap();
    let started = Instant::now();
    let thumb = decoded.thumbnail(256, 256);
    println!("thumbnail in {:?}", started.elapsed());
    assert_eq!((thumb.width(), thumb.height()), (256, 192));
    let mut out = Vec::new();
    thumb
        .write_to(&mut Cursor::new(&mut out), ImageFormat::Jpeg)
        .unwrap();
    assert!(out.len() < 20_000, "{} bytes", out.len());
}

// ---- EXIF -------------------------------------------------------------------------------------

fn le16(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn le32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn entry(out: &mut Vec<u8>, tag: u16, kind: u16, count: u32, value: u32) {
    le16(out, tag);
    le16(out, kind);
    le32(out, count);
    le32(out, value);
}

/// A JPEG that is only SOI, an APP1 Exif segment and EOI: orientation 6 (rotate 90° clockwise),
/// 40°26'46.2" N, 79°58'56" W, in a little-endian TIFF.
fn exif_jpeg() -> Vec<u8> {
    let mut tiff = b"II".to_vec();
    le16(&mut tiff, 42);
    le32(&mut tiff, 8);
    // IFD0 at 8: two entries, 30 bytes; the GPS IFD follows at 38.
    le16(&mut tiff, 2);
    entry(&mut tiff, 0x0112, 3, 1, 6);
    entry(&mut tiff, 0x8825, 4, 1, 38);
    le32(&mut tiff, 0);
    // GPS IFD at 38: four entries, 54 bytes; the rationals follow at 92.
    le16(&mut tiff, 4);
    entry(&mut tiff, 0x0001, 2, 2, u32::from_le_bytes(*b"N\0\0\0"));
    entry(&mut tiff, 0x0002, 5, 3, 92);
    entry(&mut tiff, 0x0003, 2, 2, u32::from_le_bytes(*b"W\0\0\0"));
    entry(&mut tiff, 0x0004, 5, 3, 116);
    le32(&mut tiff, 0);
    for (num, den) in [(40, 1), (26, 1), (462, 10), (79, 1), (58, 1), (56, 1)] {
        le32(&mut tiff, num);
        le32(&mut tiff, den);
    }
    assert_eq!(tiff.len(), 140);

    let mut app1 = b"Exif\0\0".to_vec();
    app1.extend_from_slice(&tiff);
    let mut jpeg = vec![0xFF, 0xD8, 0xFF, 0xE1];
    jpeg.extend_from_slice(&((app1.len() + 2) as u16).to_be_bytes());
    jpeg.extend_from_slice(&app1);
    jpeg.extend_from_slice(&[0xFF, 0xD9]);
    jpeg
}

fn degrees(value: &exif::Value) -> f64 {
    let exif::Value::Rational(parts) = value else {
        panic!("{value:?} is not a rational");
    };
    parts[0].to_f64() + parts[1].to_f64() / 60.0 + parts[2].to_f64() / 3600.0
}

// Probe 4: orientation and GPS come out of the APP1 segment, with no image data at all.
#[test]
fn exif_gives_orientation_and_gps() {
    let bytes = exif_jpeg();
    let exif = exif::Reader::new()
        .read_from_container(&mut Cursor::new(&bytes))
        .unwrap();
    for field in exif.fields() {
        println!(
            "  {} {} = {}",
            field.ifd_num,
            field.tag,
            field.display_value()
        );
    }
    let orientation = exif
        .get_field(exif::Tag::Orientation, exif::In::PRIMARY)
        .and_then(|f| f.value.get_uint(0));
    assert_eq!(orientation, Some(6));

    let lat = exif
        .get_field(exif::Tag::GPSLatitude, exif::In::PRIMARY)
        .unwrap();
    let lat_ref = exif
        .get_field(exif::Tag::GPSLatitudeRef, exif::In::PRIMARY)
        .unwrap();
    let lon = exif
        .get_field(exif::Tag::GPSLongitude, exif::In::PRIMARY)
        .unwrap();
    let lon_ref = exif
        .get_field(exif::Tag::GPSLongitudeRef, exif::In::PRIMARY)
        .unwrap();
    assert_eq!(lat_ref.display_value().to_string(), "N");
    assert_eq!(lon_ref.display_value().to_string(), "W");
    assert!((degrees(&lat.value) - 40.446_166_7).abs() < 1e-6);
    assert!((degrees(&lon.value) - 79.982_222_2).abs() < 1e-6);
}

// Probe 5: a JPEG without EXIF and a file that is not a picture are errors, not panics; and
// `infer` names the kinds the drop zone meets.
#[test]
fn exif_errors_cleanly_and_infer_names_the_kinds() {
    let plain = {
        let mut bytes = Vec::new();
        DynamicImage::ImageRgb8(RgbImage::new(4, 4))
            .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Jpeg)
            .unwrap();
        bytes
    };
    let none = exif::Reader::new().read_from_container(&mut Cursor::new(&plain));
    println!("JPEG without EXIF: {:?}", none.as_ref().err());
    assert!(none.is_err());
    let junk = exif::Reader::new().read_from_container(&mut Cursor::new(b"not a picture"));
    assert!(junk.is_err());

    assert_eq!(
        infer::get(&plain).map(|t| t.mime_type()),
        Some("image/jpeg")
    );
    assert_eq!(
        infer::get(&png_header_only(1, 1)).map(|t| t.mime_type()),
        Some("image/png")
    );
    assert_eq!(
        infer::get(b"%PDF-1.4\n").map(|t| t.mime_type()),
        Some("application/pdf")
    );
    assert_eq!(
        infer::get(b"name,note\nAda,x\n"),
        None,
        "CSV has no magic: by extension"
    );
    assert_eq!(
        infer::get(b"PK\x03\x04\x14\0\0\0\0\0").map(|t| t.mime_type()),
        Some("application/zip"),
        "an XLSX is a zip to infer; the extension and the workbook part decide"
    );
}
