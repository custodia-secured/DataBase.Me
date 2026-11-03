// SPDX-License-Identifier: AGPL-3.0-or-later
//! Pictures with EXIF and thumbnails (M6 acceptance): a photo becomes a `Media` with when and
//! where it was taken, the camera and size as fields, an image block, the original and an
//! upright thumbnail attached; a picture without EXIF gets its thumbnail alone; what cannot be
//! decoded is attached with a warning.

#![allow(
    clippy::unwrap_used,
    reason = "test helpers outside #[test] functions may panic"
)]

mod common;

use std::fs;
use std::io::Cursor;
use std::path::Path;
use std::sync::Arc;

use dbme_core::model::AttachmentRole;
use dbme_core::{Clock, ManualClock};
use dbme_ingest::images::ms_of;
use dbme_ingest::importer::BlobDraft;
use dbme_ingest::{ImportOptions, Importers, apply, plan};
use dbme_store::{Vault, VaultOptions};

use common::{fixture_file, photo_jpeg, pixel_png, png_header_only};

fn vault(dir: &Path) -> Vault {
    let clock: Arc<dyn Clock> = Arc::new(ManualClock::new(1_762_128_000_000));
    let options = VaultOptions {
        identity: dbme_store::Identity::generate(clock.as_ref()).unwrap(),
        clock,
    };
    Vault::create(&dir.join("Test.dbme"), options).unwrap()
}

#[test]
fn a_photo_becomes_media_with_its_exif_an_image_block_and_an_upright_thumbnail() {
    let dir = tempfile::tempdir().unwrap();
    let mut vault = vault(dir.path());
    let path = fixture_file("images/photo.jpg", &photo_jpeg());
    let plan = plan(&vault, &[path], ImportOptions::default(), &Importers::v0()).unwrap();
    assert_eq!(plan.entries[0].importer, "image");
    assert!(plan.warnings.is_empty(), "{:?}", plan.warnings);
    let object = &plan.entries[0].extract.objects[0];
    assert_eq!(
        (object.title.as_str(), object.type_name.as_str()),
        ("photo", "Media")
    );
    assert_eq!(object.captured_at, Some(ms_of(2025, 11, 3, 10, 20, 30)));
    assert!((object.geo_lat.unwrap() - 40.446_166_7).abs() < 1e-6);
    assert!(
        (object.geo_lon.unwrap() + 79.982_222_2).abs() < 1e-6,
        "west is negative"
    );
    assert_eq!(object.props["width"], 64);
    assert_eq!(object.props["height"], 32);
    assert_eq!(object.props["orientation"], 6);
    assert_eq!(object.props["camera_make"], "Dbme");
    assert_eq!(object.props["camera_model"], "Cam 1");
    assert_eq!(object.blocks.len(), 1);
    assert_eq!(object.blocks[0].kind, "image");
    assert_eq!(object.blocks[0].content["alt"], "photo.jpg");
    let original = &object.attachments[0];
    assert_eq!(original.role, AttachmentRole::Original);
    assert_eq!(object.blocks[0].content["blob"], original.hash.to_string());
    assert_eq!(original.mime.as_deref(), Some("image/jpeg"));
    let thumb = &object.attachments[1];
    assert_eq!(thumb.role, AttachmentRole::Thumbnail);
    assert_eq!(thumb.filename.as_deref(), Some("photo.thumb.jpg"));
    assert_eq!(thumb.mime.as_deref(), Some("image/jpeg"));
    let BlobDraft::Derived(bytes) = &thumb.blob else {
        panic!("a thumbnail is derived bytes");
    };
    let decoded = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .unwrap()
        .decode()
        .unwrap();
    assert_eq!(
        (decoded.width(), decoded.height()),
        (32, 64),
        "rotated upright"
    );
    assert_eq!(plan.entries[0].extract.types[0].name, "Media");

    let run = apply(&mut vault, &plan).unwrap();
    let read = vault.read();
    let media = read.object(run.created[0]).unwrap();
    assert_eq!(media.captured_at, Some(ms_of(2025, 11, 3, 10, 20, 30)));
    assert!((media.geo_lat.unwrap() - 40.446_166_7).abs() < 1e-6);
    assert_eq!(media.props_json["camera_make"], "Dbme");
    let attachments = read.attachments(media.id).unwrap();
    assert_eq!(attachments.len(), 2);
    assert!(
        attachments
            .iter()
            .any(|a| a.role == AttachmentRole::Thumbnail)
    );
    assert!(vault.blobs().check(&thumb.hash).unwrap());
    assert_eq!(read.blocks(media.id).unwrap()[0].kind, "image");
    assert_eq!(
        read.provenance_of(media.id).unwrap()[0].source_kind,
        "import:image"
    );
    let report = vault.verify(true).unwrap();
    assert!(report.is_ok(), "{:#?}", report.problems);
}

#[test]
fn a_png_without_exif_and_pictures_that_cannot_be_decoded() {
    let dir = tempfile::tempdir().unwrap();
    let mut vault = vault(dir.path());
    let pixel = fixture_file("images/pixel.png", &pixel_png());
    let bomb = dir.path().join("bomb.png");
    fs::write(&bomb, png_header_only(100_000, 100_000)).unwrap();
    let junk = dir.path().join("junk.jpg");
    fs::write(&junk, b"not a picture at all").unwrap();
    let plan = plan(
        &vault,
        &[pixel, bomb, junk],
        ImportOptions::default(),
        &Importers::v0(),
    )
    .unwrap();
    assert_eq!(plan.entries.len(), 3, "{:?}", plan.refused);

    let pixel = &plan.entries[0].extract.objects[0];
    assert_eq!(pixel.captured_at, None);
    assert_eq!(pixel.geo_lat, None);
    assert_eq!(
        (
            pixel.props["width"].as_u64(),
            pixel.props["height"].as_u64()
        ),
        (Some(1), Some(1))
    );
    assert!(!pixel.props.contains_key("camera_make"));
    assert_eq!(pixel.attachments.len(), 2, "an original and a thumbnail");
    assert!((pixel.provenance.confidence - 1.0).abs() < f64::EPSILON);

    let bomb = &plan.entries[1].extract.objects[0];
    assert_eq!(bomb.props["width"], 100_000);
    assert_eq!(bomb.attachments.len(), 1, "no thumbnail");
    assert!(
        plan.warnings
            .iter()
            .any(|w| w.contains("over the 64000000 limit")),
        "{:?}",
        plan.warnings
    );

    let junk = &plan.entries[2].extract.objects[0];
    assert_eq!(junk.type_name, "Media");
    assert!(!junk.props.contains_key("width"));
    assert_eq!(junk.attachments.len(), 1);
    assert_eq!(junk.blocks.len(), 1, "the block names the blob even so");
    assert!((junk.provenance.confidence - 0.7).abs() < f64::EPSILON);
    assert!(
        plan.warnings
            .iter()
            .any(|w| w.contains("not a picture the decoder knows")),
        "{:?}",
        plan.warnings
    );

    let run = apply(&mut vault, &plan).unwrap();
    assert_eq!(run.objects, 3);
    let report = vault.verify(true).unwrap();
    assert!(report.is_ok(), "{:#?}", report.problems);
}
