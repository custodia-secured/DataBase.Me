// SPDX-License-Identifier: AGPL-3.0-or-later
//! Pictures (M6 Task 5, ADR-0026): a picture becomes a `Media` with the file attached as its
//! original, an image block showing it, a 256-pixel JPEG thumbnail attached with the
//! `thumbnail` role (orientation applied), and what EXIF says: when it was taken as the object's
//! `captured_at`, where as `geo_lat` and `geo_lon`, and the camera, orientation and size as
//! fields. Decoding runs under `image::Limits` with the importer's own pixel count in front of it
//! (M6 Decision 14): a picture past [`MAX_PIXELS`], or one the decoder refuses, is still imported
//! with a warning and without its thumbnail.

use std::io::Cursor;

use dbme_core::ContentHash;
use dbme_core::model::{AttachmentRole, FieldKind};
use image::codecs::jpeg::JpegEncoder;
use image::{DynamicImage, ImageReader, Limits};
use serde_json::json;

use crate::importer::{
    AttachmentDraft, BlockDraft, Ctx, Extract, ExtractedObject, FieldDraft, Importer,
    ProvenanceDraft, TypeDraft,
};
use crate::{IngestError, Sniffed, Source};

/// More pixels than this are not decoded (M6 Decision 14).
pub const MAX_PIXELS: u64 = 64_000_000;
/// The thumbnail's longer side.
pub const THUMBNAIL: u32 = 256;
/// The thumbnail's JPEG quality.
const THUMBNAIL_QUALITY: u8 = 80;

/// The picture importer.
#[derive(Clone, Copy, Debug, Default)]
pub struct Image;

/// The fields a picture import puts on `Media`.
fn media_fields() -> TypeDraft {
    let text = |key: &str, label: &str| FieldDraft {
        key: key.into(),
        label: label.into(),
        kind: FieldKind::Text,
    };
    let number = |key: &str, label: &str| FieldDraft {
        key: key.into(),
        label: label.into(),
        kind: FieldKind::Number,
    };
    TypeDraft {
        name: "Media".into(),
        fields: vec![
            number("width", "Width"),
            number("height", "Height"),
            number("orientation", "Orientation"),
            text("camera_make", "Camera make"),
            text("camera_model", "Camera model"),
        ],
    }
}

/// The decoder's limits (M6.md Design): 16,384 a side, 256 MiB.
fn limits() -> Limits {
    let mut limits = Limits::default();
    limits.max_image_width = Some(16_384);
    limits.max_image_height = Some(16_384);
    limits.max_alloc = Some(256 << 20);
    limits
}

impl Importer for Image {
    fn id(&self) -> &'static str {
        "image"
    }

    fn accepts(&self, sniffed: &Sniffed) -> bool {
        sniffed.has_extension(&["jpg", "jpeg", "png", "gif", "webp"])
            || sniffed.mime_starts_with("image/")
    }

    fn plan(&self, source: &Source, _ctx: &Ctx<'_>) -> Result<Extract, IngestError> {
        let bytes = source.read()?;
        let hash = ContentHash::of(&bytes);
        let file_name = source
            .sniffed
            .as_ref()
            .map(|s| s.file_name.clone())
            .unwrap_or_default();
        let mut extract = Extract {
            types: vec![media_fields()],
            ..Extract::default()
        };
        let mut object = ExtractedObject::new(
            &source.stem(),
            "Media",
            ProvenanceDraft::from_source(source, Some(hash.clone())),
        );
        object
            .attachments
            .push(AttachmentDraft::original(source, hash.clone()));
        object.blocks.push(BlockDraft {
            kind: "image".into(),
            content: json!({ "blob": hash.to_string(), "alt": file_name }),
        });

        let orientation = read_exif(&bytes, &mut object);

        // The size from the header alone, then the decode under the limits.
        let dimensions = ImageReader::new(Cursor::new(&bytes))
            .with_guessed_format()
            .map_err(|e| e.to_string())
            .and_then(|r| r.into_dimensions().map_err(|e| e.to_string()));
        match dimensions {
            Err(e) => {
                extract.warnings.push(format!(
                    "not a picture the decoder knows ({e}): attached as it is"
                ));
                object.provenance.confidence = 0.7;
            }
            Ok((width, height)) => {
                object.props.insert("width".into(), json!(width));
                object.props.insert("height".into(), json!(height));
                let pixels = u64::from(width) * u64::from(height);
                if pixels > MAX_PIXELS {
                    extract.warnings.push(format!(
                        "{width}x{height} is {pixels} pixels, over the {MAX_PIXELS} limit: no thumbnail"
                    ));
                } else {
                    match decode(&bytes) {
                        Ok(picture) => {
                            let thumb = thumbnail(&picture, orientation);
                            object.attachments.push(AttachmentDraft::derived(
                                thumb,
                                AttachmentRole::Thumbnail,
                                &format!("{}.thumb.jpg", source.stem()),
                                "image/jpeg",
                            ));
                        }
                        Err(e) => extract
                            .warnings
                            .push(format!("could not be decoded ({e}): no thumbnail")),
                    }
                }
            }
        }
        extract.objects.push(object);
        Ok(extract)
    }
}

fn decode(bytes: &[u8]) -> Result<DynamicImage, String> {
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| e.to_string())?;
    reader.limits(limits());
    reader.decode().map_err(|e| e.to_string())
}

/// A JPEG at most [`THUMBNAIL`] pixels a side, the EXIF orientation applied so it shows upright.
fn thumbnail(picture: &DynamicImage, orientation: Option<u32>) -> Vec<u8> {
    let small = if picture.width() > THUMBNAIL || picture.height() > THUMBNAIL {
        picture.thumbnail(THUMBNAIL, THUMBNAIL)
    } else {
        picture.clone()
    };
    let upright = match orientation {
        Some(2) => small.fliph(),
        Some(3) => small.rotate180(),
        Some(4) => small.flipv(),
        Some(5) => small.rotate90().fliph(),
        Some(6) => small.rotate90(),
        Some(7) => small.rotate270().fliph(),
        Some(8) => small.rotate270(),
        _ => small,
    };
    let mut out = Vec::new();
    let encoder = JpegEncoder::new_with_quality(&mut out, THUMBNAIL_QUALITY);
    // Encoding an in-memory RGB8 image cannot fail for lack of a format; an error would be a
    // bug in the encoder, and the thumbnail is then simply empty and not attached.
    if upright.to_rgb8().write_with_encoder(encoder).is_err() {
        out.clear();
    }
    out
}

/// Reads EXIF into the object: when, where, camera, orientation. Returns the orientation.
fn read_exif(bytes: &[u8], object: &mut ExtractedObject) -> Option<u32> {
    let exif = exif::Reader::new()
        .read_from_container(&mut Cursor::new(bytes))
        .ok()?;
    let field = |tag: exif::Tag| exif.get_field(tag, exif::In::PRIMARY);
    let text = |tag: exif::Tag| {
        field(tag).map(|f| {
            f.display_value()
                .to_string()
                .trim_matches('"')
                .trim()
                .to_owned()
        })
    };
    let orientation = field(exif::Tag::Orientation).and_then(|f| f.value.get_uint(0));
    if let Some(o) = orientation {
        object.props.insert("orientation".into(), json!(o));
    }
    if let Some(make) = text(exif::Tag::Make).filter(|t| !t.is_empty()) {
        object.props.insert("camera_make".into(), json!(make));
    }
    if let Some(model) = text(exif::Tag::Model).filter(|t| !t.is_empty()) {
        object.props.insert("camera_model".into(), json!(model));
    }
    if let Some(exif::Value::Ascii(parts)) = field(exif::Tag::DateTimeOriginal).map(|f| &f.value)
        && let Some(first) = parts.first()
        && let Ok(dt) = exif::DateTime::from_ascii(first)
    {
        object.captured_at = Some(ms_of(
            i64::from(dt.year),
            i64::from(dt.month),
            i64::from(dt.day),
            i64::from(dt.hour),
            i64::from(dt.minute),
            i64::from(dt.second),
        ));
    }
    let lat = degrees(
        field(exif::Tag::GPSLatitude),
        text(exif::Tag::GPSLatitudeRef),
        "S",
    );
    let lon = degrees(
        field(exif::Tag::GPSLongitude),
        text(exif::Tag::GPSLongitudeRef),
        "W",
    );
    if let (Some(lat), Some(lon)) = (lat, lon)
        && (-90.0..=90.0).contains(&lat)
        && (-180.0..=180.0).contains(&lon)
    {
        object.geo_lat = Some(lat);
        object.geo_lon = Some(lon);
    }
    orientation
}

/// Degrees from a GPS rational triple and its hemisphere.
fn degrees(field: Option<&exif::Field>, reference: Option<String>, negative: &str) -> Option<f64> {
    let exif::Value::Rational(parts) = &field?.value else {
        return None;
    };
    if parts.len() < 3 || parts.iter().take(3).any(|p| p.denom == 0) {
        return None;
    }
    let value = parts[0].to_f64() + parts[1].to_f64() / 60.0 + parts[2].to_f64() / 3600.0;
    let sign = if reference.as_deref() == Some(negative) {
        -1.0
    } else {
        1.0
    };
    Some(value * sign)
}

/// Milliseconds since the epoch for a civil date and time, read as UTC: EXIF carries no zone
/// (Howard Hinnant's days-from-civil).
pub fn ms_of(year: i64, month: i64, day: i64, hour: i64, minute: i64, second: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    ((days * 86_400) + hour * 3_600 + minute * 60 + second) * 1_000
}

#[cfg(test)]
mod tests {
    use super::ms_of;
    use crate::apply::datetime_of;

    #[test]
    fn civil_times_round_trip_through_the_date_formatting() {
        assert_eq!(ms_of(1970, 1, 1, 0, 0, 0), 0);
        assert_eq!(
            datetime_of(ms_of(2025, 11, 3, 10, 20, 30)),
            "2025-11-03T10:20:30Z"
        );
        assert_eq!(
            datetime_of(ms_of(2000, 2, 29, 23, 59, 59)),
            "2000-02-29T23:59:59Z"
        );
        assert_eq!(
            datetime_of(ms_of(1969, 12, 31, 23, 59, 59)),
            "1969-12-31T23:59:59Z"
        );
    }
}
