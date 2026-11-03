// SPDX-License-Identifier: AGPL-3.0-or-later
//! The importer of last resort: any file becomes one object with the file attached as its
//! original and nothing extracted. `Media` for pictures, sound and video by their media type;
//! `Document` for everything else. Archives are files too (M6 Decision 5): nothing is unpacked.

use crate::importer::{AttachmentDraft, Ctx, Extract, ExtractedObject, Importer, ProvenanceDraft};
use crate::{IngestError, Sniffed, Source};

/// The generic importer.
#[derive(Clone, Copy, Debug, Default)]
pub struct Generic;

/// The type a generic import of `sniffed` gets.
pub fn type_for(sniffed: &Sniffed) -> &'static str {
    if sniffed.mime_starts_with("image/")
        || sniffed.mime_starts_with("audio/")
        || sniffed.mime_starts_with("video/")
    {
        "Media"
    } else {
        "Document"
    }
}

impl Importer for Generic {
    fn id(&self) -> &'static str {
        "file"
    }

    fn accepts(&self, _sniffed: &Sniffed) -> bool {
        true
    }

    fn plan(&self, source: &Source, _ctx: &Ctx<'_>) -> Result<Extract, IngestError> {
        let sniffed = source
            .sniffed
            .as_ref()
            .ok_or_else(|| IngestError::NotASource(source.path.clone()))?;
        let hash = source.hash()?;
        let mut object = ExtractedObject::new(
            &source.stem(),
            type_for(sniffed),
            ProvenanceDraft::from_source(source, Some(hash.clone())),
        );
        object
            .attachments
            .push(AttachmentDraft::original(source, hash));
        Ok(Extract {
            types: Vec::new(),
            folder: None,
            objects: vec![object],
            warnings: Vec::new(),
        })
    }
}
