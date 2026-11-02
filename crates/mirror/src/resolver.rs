// SPDX-License-Identifier: AGPL-3.0-or-later
//! The [`LinkResolver`] over a vault's `mirror_state`: wikilinks name paths, and the rows say which
//! object has which path.

use std::collections::HashMap;

use dbme_core::files::LinkResolver;
use dbme_core::files::paths::path_key;
use dbme_core::{ContentHash, Id};
use dbme_store::{MirrorFileState, MirrorState};

/// Resolves `[[path]]` to objects and objects to paths from the `mirror_state` rows of the
/// objects that have a file (`synced`, `pending` or `held`), and names attachments by their blob's
/// media type.
#[derive(Clone, Debug, Default)]
pub struct VaultResolver {
    /// Object → path without `.md`.
    paths: HashMap<Id, String>,
    /// `path_key` of the path without `.md` → object.
    by_key: HashMap<String, Id>,
    /// Lower-cased basename without `.md` → the objects that have it.
    by_name: HashMap<String, Vec<Id>>,
    /// Blob → file extension, from the attachments' media types.
    extensions: HashMap<ContentHash, String>,
}

impl VaultResolver {
    /// Builds the resolver from every object's file row.
    pub fn new(states: &[MirrorState]) -> Self {
        let mut resolver = Self::default();
        for state in states {
            if matches!(
                state.state,
                MirrorFileState::Synced | MirrorFileState::Pending | MirrorFileState::Held
            ) {
                resolver.add(state.object_id, &state.rel_path);
            }
        }
        resolver
    }

    /// Records that `id` lives at `rel_path`, replacing an earlier path of the same object.
    pub fn add(&mut self, id: Id, rel_path: &str) {
        self.forget(id);
        let bare = rel_path.strip_suffix(".md").unwrap_or(rel_path).to_owned();
        self.by_key.insert(path_key(&bare), id);
        let name = bare.rsplit('/').next().unwrap_or(&bare);
        self.by_name.entry(path_key(name)).or_default().push(id);
        self.paths.insert(id, bare);
    }

    /// Forgets an object's path.
    pub fn forget(&mut self, id: Id) {
        if let Some(bare) = self.paths.remove(&id) {
            self.by_key.remove(&path_key(&bare));
            let name = bare.rsplit('/').next().unwrap_or(&bare);
            if let Some(ids) = self.by_name.get_mut(&path_key(name)) {
                ids.retain(|other| *other != id);
            }
        }
    }

    /// Records the media type of a blob, so its file gets an extension.
    pub fn add_blob(&mut self, hash: ContentHash, mime: Option<&str>, filename: Option<&str>) {
        let ext = extension_for(mime, filename);
        self.extensions.insert(hash, ext);
    }

    /// Whether any object has this path (compared by `path_key`).
    pub fn has_path(&self, rel_path: &str) -> Option<Id> {
        let bare = rel_path.strip_suffix(".md").unwrap_or(rel_path);
        self.by_key.get(&path_key(bare)).copied()
    }
}

/// The file extension for a media type, or the one in the file name, or none.
pub(crate) fn extension_for(mime: Option<&str>, filename: Option<&str>) -> String {
    let from_mime = match mime.map(|m| m.split(';').next().unwrap_or(m).trim()) {
        Some("image/png") => Some(".png"),
        Some("image/jpeg") => Some(".jpg"),
        Some("image/gif") => Some(".gif"),
        Some("image/webp") => Some(".webp"),
        Some("image/svg+xml") => Some(".svg"),
        Some("image/heic") => Some(".heic"),
        Some("application/pdf") => Some(".pdf"),
        Some("text/plain") => Some(".txt"),
        Some("text/markdown") => Some(".md"),
        Some("text/csv") => Some(".csv"),
        Some("application/json") => Some(".json"),
        Some("audio/mpeg") => Some(".mp3"),
        Some("audio/mp4") => Some(".m4a"),
        Some("video/mp4") => Some(".mp4"),
        Some("application/zip") => Some(".zip"),
        _ => None,
    };
    if let Some(ext) = from_mime {
        return ext.to_owned();
    }
    filename
        .and_then(|name| name.rsplit_once('.'))
        .map(|(_, ext)| ext)
        .filter(|ext| {
            !ext.is_empty() && ext.len() <= 8 && ext.chars().all(|c| c.is_ascii_alphanumeric())
        })
        .map(|ext| format!(".{}", ext.to_ascii_lowercase()))
        .unwrap_or_default()
}

impl LinkResolver for VaultResolver {
    fn resolve(&self, target: &str) -> Option<Id> {
        let target = target.trim().trim_start_matches('/');
        let bare = target.strip_suffix(".md").unwrap_or(target);
        if let Some(id) = self.by_key.get(&path_key(bare)) {
            return Some(*id);
        }
        if bare.contains('/') {
            return None;
        }
        match self.by_name.get(&path_key(bare)).map(Vec::as_slice) {
            Some([only]) => Some(*only),
            _ => None,
        }
    }

    fn path_of(&self, id: Id) -> Option<String> {
        self.paths.get(&id).cloned()
    }

    fn attachment_name(&self, blob: &ContentHash) -> String {
        let ext = self.extensions.get(blob).map(String::as_str).unwrap_or("");
        format!("{}{ext}", blob.hex())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(n: u128, rel_path: &str, state: MirrorFileState) -> MirrorState {
        MirrorState {
            object_id: Id::from_parts(1, n),
            rel_path: rel_path.to_owned(),
            path_key: path_key(rel_path),
            last_hash: None,
            base_text: None,
            synced_seq: None,
            synced_at: None,
            file_size: None,
            file_mtime_ms: None,
            state,
        }
    }

    #[test]
    fn paths_resolve_exactly_then_by_unique_basename() {
        let resolver = VaultResolver::new(&[
            state(1, "Projects/Alpha.md", MirrorFileState::Synced),
            state(2, "Archive/Alpha.md", MirrorFileState::Pending),
            state(3, "People/Bravo Two.md", MirrorFileState::Held),
            state(4, ".dbme/trash/x.md", MirrorFileState::Trashed),
            state(5, ".dbme/excluded/y.md", MirrorFileState::Excluded),
        ]);
        let id = |n: u128| Some(Id::from_parts(1, n));
        assert_eq!(resolver.resolve("Projects/Alpha"), id(1));
        assert_eq!(resolver.resolve("projects/alpha.md"), id(1));
        assert_eq!(resolver.resolve("/Archive/Alpha "), id(2));
        assert_eq!(
            resolver.resolve("Alpha"),
            None,
            "two files share the basename"
        );
        assert_eq!(resolver.resolve("bravo two"), id(3));
        assert_eq!(resolver.resolve("Nowhere/Bravo Two"), None);
        assert_eq!(resolver.resolve("x"), None, "trashed files have no path");
        assert_eq!(
            resolver.path_of(Id::from_parts(1, 3)).as_deref(),
            Some("People/Bravo Two")
        );
        assert_eq!(resolver.path_of(Id::from_parts(1, 4)), None);
        assert_eq!(resolver.has_path("people/BRAVO TWO.md"), id(3));
        assert_eq!(resolver.has_path("People/Nobody.md"), None);
    }

    #[test]
    fn add_replaces_and_forget_frees() {
        let mut resolver = VaultResolver::default();
        let a = Id::from_parts(1, 1);
        resolver.add(a, "Old/A.md");
        resolver.add(a, "Moved/A.md");
        assert_eq!(resolver.resolve("A"), Some(a), "still unique by basename");
        assert_eq!(
            resolver.resolve("Old/A"),
            None,
            "the old exact path is gone"
        );
        assert_eq!(resolver.path_of(a).as_deref(), Some("Moved/A"));
        resolver.forget(a);
        resolver.forget(a);
        assert_eq!(resolver.resolve("A"), None);
        assert_eq!(resolver.path_of(a), None);
    }

    #[test]
    fn attachments_are_named_by_hash_and_media_type() {
        let mut resolver = VaultResolver::default();
        let png = ContentHash::of(b"png");
        let unknown = ContentHash::of(b"?");
        let named = ContentHash::of(b"named");
        resolver.add_blob(png.clone(), Some("image/png; charset=binary"), None);
        resolver.add_blob(unknown.clone(), Some("application/x-thing"), None);
        resolver.add_blob(named.clone(), None, Some("Report.Final.DOCX"));
        assert_eq!(resolver.attachment_name(&png), format!("{}.png", png.hex()));
        assert_eq!(resolver.attachment_name(&unknown), unknown.hex());
        assert_eq!(
            resolver.attachment_name(&named),
            format!("{}.docx", named.hex())
        );
        assert_eq!(
            resolver.attachment_name(&ContentHash::of(b"never")),
            ContentHash::of(b"never").hex()
        );

        for (mime, ext) in [
            ("image/jpeg", ".jpg"),
            ("image/gif", ".gif"),
            ("image/webp", ".webp"),
            ("image/svg+xml", ".svg"),
            ("image/heic", ".heic"),
            ("application/pdf", ".pdf"),
            ("text/plain", ".txt"),
            ("text/markdown", ".md"),
            ("text/csv", ".csv"),
            ("application/json", ".json"),
            ("audio/mpeg", ".mp3"),
            ("audio/mp4", ".m4a"),
            ("video/mp4", ".mp4"),
            ("application/zip", ".zip"),
        ] {
            assert_eq!(extension_for(Some(mime), None), ext, "{mime}");
        }
        assert_eq!(extension_for(None, Some("noext")), "");
        assert_eq!(extension_for(None, Some("trailing.")), "");
        assert_eq!(extension_for(None, Some("odd.t@r")), "");
        assert_eq!(extension_for(None, Some("long.abcdefghij")), "");
        assert_eq!(extension_for(None, None), "");
    }
}
