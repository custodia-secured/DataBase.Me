// SPDX-License-Identifier: AGPL-3.0-or-later
//! File names and relative paths of the files folder (FORMAT.md §6.1), and the names cloud drives
//! give to conflicting copies (M5.md, appendix "cloud conflict-file names").
//!
//! A file name comes from a title through [`file_stem`]: NFC, the characters that break a file
//! system or a wikilink replaced, whitespace collapsed, edges trimmed, Windows reserved names
//! defused, [`MAX_NAME_BYTES`] enforced. Siblings with the same stem are told apart by [`dedupe`]
//! (`stem (2)`, `stem (3)`, …). Paths compare by [`path_key`], which folds case and normalisation,
//! so `Café.md` and `cafe\u{301}.md` are one file on every file system we meet.

use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;

/// The longest file or folder name, without `.md`, in bytes.
pub const MAX_NAME_BYTES: usize = 120;
/// The longest relative path, `.md` included, in bytes.
pub const MAX_REL_PATH_BYTES: usize = 200;
/// The stem of an object whose title sanitises to nothing.
pub const UNTITLED: &str = "Untitled";
/// The file extension of an object's file.
const EXT: &str = ".md";
/// [`join`] never cuts a stem shorter than this.
const MIN_STEM_BYTES: usize = 8;
/// Characters invalid on some file system or that break a wikilink.
const FORBIDDEN: [char; 13] = [
    '\\', '/', ':', '*', '?', '"', '<', '>', '|', '#', '^', '[', ']',
];

/// The file stem for a title: NFC; each of `\ / : * ? " < > | # ^ [ ]` and each control character
/// becomes `-`; runs of whitespace become one space; spaces and dots are trimmed at both ends;
/// an empty result is [`UNTITLED`]; a Windows reserved name (`CON`, `COM1`, `LPT¹`, …, compared
/// case-insensitively on the part before the first dot) gets `_` after that part, so `CON.md`
/// becomes `CON_.md`; the stem is cut to [`MAX_NAME_BYTES`] at a character boundary and trailing
/// spaces and dots are trimmed again.
pub fn file_stem(title: &str) -> String {
    let mut out = String::with_capacity(title.len());
    let mut pending_space = false;
    for c in title.nfc() {
        let c = if FORBIDDEN.contains(&c) || (c as u32) < 0x20 || c == '\x7f' {
            '-'
        } else {
            c
        };
        if c.is_whitespace() {
            pending_space = true;
            continue;
        }
        if pending_space && !out.is_empty() {
            out.push(' ');
        }
        pending_space = false;
        out.push(c);
    }
    let trimmed = out.trim_matches([' ', '.']);
    if trimmed.is_empty() {
        return UNTITLED.to_owned();
    }
    let mut stem = trimmed.to_owned();
    if let Some(head) = reserved_head(&stem) {
        stem.insert(head, '_');
    }
    if stem.len() > MAX_NAME_BYTES {
        let cut = trim_end(cut_to(&stem, MAX_NAME_BYTES));
        stem = if cut.is_empty() {
            UNTITLED.to_owned()
        } else {
            cut.to_owned()
        };
    }
    stem
}

/// The length of the part before the first dot when it is a Windows reserved device name.
fn reserved_head(stem: &str) -> Option<usize> {
    let head = stem.split('.').next().unwrap_or("");
    let upper = head.to_uppercase();
    let reserved = match upper.as_str() {
        "CON" | "PRN" | "AUX" | "NUL" => true,
        _ => upper
            .strip_prefix("COM")
            .or_else(|| upper.strip_prefix("LPT"))
            .is_some_and(|rest| {
                matches!(
                    rest,
                    "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                )
            }),
    };
    reserved.then_some(head.len())
}

/// The longest prefix of `text` that fits `max` bytes and ends on a character boundary.
fn cut_to(text: &str, max: usize) -> &str {
    let mut end = max.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

fn trim_end(text: &str) -> &str {
    text.trim_end_matches([' ', '.'])
}

/// The key two paths are compared by: NFC, then each character lower-cased (`char::to_lowercase`,
/// an approximation of Unicode case folding that is exact for every script a file name is likely
/// to carry), then `\` normalised to `/`.
pub fn path_key(rel_path: &str) -> String {
    rel_path
        .nfc()
        .flat_map(char::to_lowercase)
        .map(|c| if c == '\\' { '/' } else { c })
        .collect()
}

/// `stem` if `taken(stem)` is false, else the first of `stem (2)`, `stem (3)`, … that is not
/// taken. The stem is cut first so that the suffix fits in [`MAX_NAME_BYTES`].
pub fn dedupe(stem: &str, taken: &dyn Fn(&str) -> bool) -> String {
    if !taken(stem) {
        return stem.to_owned();
    }
    let mut n: u64 = 2;
    loop {
        let suffix = format!(" ({n})");
        let room = MAX_NAME_BYTES.saturating_sub(suffix.len());
        let base = trim_end(cut_to(stem, room));
        let candidate = format!("{base}{suffix}");
        if !taken(&candidate) {
            return candidate;
        }
        n += 1;
    }
}

/// Whether any component of the path starts with `.`: Obsidian's and our own hidden folders.
pub fn is_dot_path(rel_path: &str) -> bool {
    rel_path
        .split(['/', '\\'])
        .any(|component| component.starts_with('.'))
}

/// `A/B/stem.md` from the ancestor folder names and the stem. When the whole path would exceed
/// [`MAX_REL_PATH_BYTES`], the stem is cut to fit, but never below eight bytes, and the caller
/// dedupes the result; a path that is still too long because its folders alone are is returned as
/// it is, for the caller to refuse.
pub fn join(ancestors: &[&str], stem: &str) -> String {
    let mut prefix = String::new();
    for ancestor in ancestors.iter().filter(|a| !a.is_empty()) {
        prefix.push_str(ancestor);
        prefix.push('/');
    }
    let room = MAX_REL_PATH_BYTES
        .saturating_sub(prefix.len() + EXT.len())
        .max(MIN_STEM_BYTES);
    let stem = if stem.len() > room {
        let cut = trim_end(cut_to(stem, room));
        if cut.is_empty() { UNTITLED } else { cut }
    } else {
        stem
    };
    format!("{prefix}{stem}{EXT}")
}

/// A file name a cloud drive gave to a conflicting copy, and the name the file would have had.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloudConflict {
    /// Which drive's naming matched.
    pub vendor: CloudVendor,
    /// How sure the match is.
    pub confidence: Confidence,
    /// The sibling's name: the file as it was before the drive renamed its copy.
    pub base_name: String,
}

/// The cloud drives whose conflict names are recognised. Google Drive names its copies after the
/// original and is not recognised (appendix rule 8).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudVendor {
    /// ` (conflicted copy)`, ` (<editor>'s conflicted copy <date>)`.
    Dropbox,
    /// `<stem>-<DEVICENAME>.<ext>`.
    OneDrive,
    /// `<stem>.sync-conflict-<date>-<time>-<id>.<ext>`.
    Syncthing,
    /// ` (conflicted copy [<user> ]<date> <time>)`, `_conflict-<date>-<time>`, ` (case clash from`.
    Nextcloud,
    /// `.Conflict`, a name clash rather than an edit conflict.
    Resilio,
    /// `<stem> <n>.<ext>` from iCloud Drive on macOS.
    #[serde(rename = "icloud_mac")]
    ICloudMac,
    /// `<stem> (n).<ext>` or `<stem>(0n).<ext>` from iCloud Drive on Windows.
    #[serde(rename = "icloud_windows")]
    ICloudWindows,
}

/// How sure a cloud-conflict match is: the inbox shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    /// A fixed-width timestamp no title would carry by accident.
    Certain,
    /// The vendor's documented shape, dated.
    Likely,
    /// A shape a title could have on its own; a sibling check or a person decides.
    Possible,
}

/// Whether `name` (a file name, extension included) is a cloud drive's conflicting copy, by the
/// eight rules of the M5 appendix in order of confidence: Syncthing and Nextcloud (certain),
/// Dropbox (likely when dated, possible when not), OneDrive, Resilio, iCloud macOS and iCloud
/// Windows (possible). `sibling_exists` is asked whether the original is present where the rule
/// requires it (OneDrive, iCloud). Only `str` methods are used; no regular expressions.
///
/// Two departures from the appendix, both towards caution: OneDrive's rule compares the suffix to
/// the local device name, which this pure function does not know, so a match ranks "possible"
/// rather than "likely" and the caller may raise it; Dropbox's localised phrases are not
/// recognised until one is seen on disk.
pub fn cloud_conflict(name: &str, sibling_exists: &dyn Fn(&str) -> bool) -> Option<CloudConflict> {
    syncthing(name)
        .or_else(|| nextcloud(name))
        .or_else(|| dropbox(name))
        .or_else(|| onedrive(name, sibling_exists))
        .or_else(|| resilio(name))
        .or_else(|| icloud_mac(name, sibling_exists))
        .or_else(|| icloud_windows(name, sibling_exists))
}

/// The match, unless stripping the marker left no name or a dot file.
fn found(vendor: CloudVendor, confidence: Confidence, base_name: String) -> Option<CloudConflict> {
    (!base_name.is_empty() && !base_name.starts_with('.')).then_some(CloudConflict {
        vendor,
        confidence,
        base_name,
    })
}

/// The stem and the extension (with its dot); a leading dot is part of the stem.
fn split_ext(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    }
}

/// Whether the bytes of `text` from `at` match `pattern`, where `D` stands for an ASCII digit and
/// every other byte for itself. A match is all ASCII, so `at + pattern.len()` is a char boundary.
fn ascii_pattern(text: &str, at: usize, pattern: &[u8]) -> bool {
    text.as_bytes()
        .get(at..at + pattern.len())
        .is_some_and(|bytes| {
            bytes.iter().zip(pattern).all(|(b, p)| {
                if *p == b'D' {
                    b.is_ascii_digit()
                } else {
                    b == p
                }
            })
        })
}

/// What may follow a conflict marker: nothing, or the extension.
fn ext_or_empty(rest: &str) -> bool {
    rest.is_empty() || rest.starts_with('.')
}

/// Rule 3: `.sync-conflict-DDDDDDDD-DDDDDD-` then seven of `A-Z2-7` or none, then the extension.
fn syncthing(name: &str) -> Option<CloudConflict> {
    const MARKER: &str = ".sync-conflict-";
    for (i, _) in name.match_indices(MARKER) {
        let stamp = i + MARKER.len();
        if !ascii_pattern(name, stamp, b"DDDDDDDD-DDDDDD-") {
            continue;
        }
        let after = stamp + 16;
        let id_ok = name
            .as_bytes()
            .get(after..after + 7)
            .is_some_and(|id| id.iter().all(|b| matches!(b, b'A'..=b'Z' | b'2'..=b'7')));
        let rest = if id_ok {
            &name[after + 7..]
        } else {
            &name[after..]
        };
        if ext_or_empty(rest) {
            return found(
                CloudVendor::Syncthing,
                Confidence::Certain,
                format!("{}{rest}", &name[..i]),
            );
        }
    }
    None
}

/// Rule 4: ` (conflicted copy [<user> ]DDDD-DD-DD DDDDDD)`, legacy `_conflict-DDDDDDDD-DDDDDD`,
/// and ` (case clash from …)`.
fn nextcloud(name: &str) -> Option<CloudConflict> {
    const COPY: &str = " (conflicted copy";
    const LEGACY: &str = "_conflict-";
    const CLASH: &str = " (case clash from";
    for (i, _) in name.match_indices(COPY) {
        let inner_start = i + COPY.len();
        let Some(close) = name[inner_start..].find(')') else {
            continue;
        };
        let close = inner_start + close;
        let inner = &name[inner_start..close];
        let rest = &name[close + 1..];
        // `inner` is ` [<user> ]DDDD-DD-DD DDDDDD`: a space, then the user (optional) and the
        // stamp, the byte before the stamp being a space either way.
        let dated = inner.len().checked_sub(17).is_some_and(|at| {
            at >= 1
                && inner.as_bytes()[at - 1] == b' '
                && ascii_pattern(inner, at, b"DDDD-DD-DD DDDDDD")
        });
        if dated && ext_or_empty(rest) {
            return found(
                CloudVendor::Nextcloud,
                Confidence::Certain,
                format!("{}{rest}", &name[..i]),
            );
        }
    }
    for (i, _) in name.match_indices(LEGACY) {
        let stamp = i + LEGACY.len();
        if ascii_pattern(name, stamp, b"DDDDDDDD-DDDDDD") && ext_or_empty(&name[stamp + 15..]) {
            return found(
                CloudVendor::Nextcloud,
                Confidence::Certain,
                format!("{}{}", &name[..i], &name[stamp + 15..]),
            );
        }
    }
    if let Some(i) = name.find(CLASH) {
        let close = name[i..].find(')')? + i;
        let rest = &name[close + 1..];
        if ext_or_empty(rest) {
            return found(
                CloudVendor::Nextcloud,
                Confidence::Certain,
                format!("{}{rest}", &name[..i]),
            );
        }
    }
    None
}

/// Rule 1: the last ` (` before `conflicted copy`; `(conflicted copy)` or `(<editor>'s conflicted
/// copy)` undated rank possible; a `DDDD-DD-DD` after the phrase ranks likely.
fn dropbox(name: &str) -> Option<CloudConflict> {
    const PHRASE: &str = "conflicted copy";
    let p = name.rfind(PHRASE)?;
    let after = p + PHRASE.len();
    let (confidence, rest) = if name[after..].starts_with(')') {
        (Confidence::Possible, &name[after + 1..])
    } else if ascii_pattern(name, after, b" DDDD-DD-DD)") {
        (Confidence::Likely, &name[after + 12..])
    } else {
        return None;
    };
    if !ext_or_empty(rest) {
        return None;
    }
    let open = name[..p].rfind(" (")?;
    let between = &name[open + 2..p];
    if !(between.is_empty() || between.ends_with("'s ") || between.ends_with("\u{2019}s ")) {
        return None;
    }
    found(
        CloudVendor::Dropbox,
        confidence,
        format!("{}{rest}", &name[..open]),
    )
}

/// Rule 2: `<stem>-<DEVICENAME>.<ext>` whose sibling `<stem>.<ext>` exists. The device name is
/// not known here, so every `-` is tried and a match ranks possible.
fn onedrive(name: &str, sibling_exists: &dyn Fn(&str) -> bool) -> Option<CloudConflict> {
    let (stem, ext) = split_ext(name);
    for (i, _) in stem.match_indices('-') {
        if i == 0 || i + 1 == stem.len() {
            continue;
        }
        let base = format!("{}{ext}", &stem[..i]);
        if sibling_exists(&base) {
            return found(CloudVendor::OneDrive, Confidence::Possible, base);
        }
    }
    None
}

/// Rule 5: `.Conflict` as the last component or before the extension, stripped repeatedly.
fn resilio(name: &str) -> Option<CloudConflict> {
    const MARKER: &str = ".Conflict";
    let mut current = name.to_owned();
    let mut stripped = false;
    loop {
        if let Some(shorter) = current.strip_suffix(MARKER) {
            current = shorter.to_owned();
            stripped = true;
            continue;
        }
        let (stem, ext) = split_ext(&current);
        let replaced = stem.strip_suffix(MARKER).map(|s| format!("{s}{ext}"));
        match replaced {
            Some(next) => {
                current = next;
                stripped = true;
            }
            None => break,
        }
    }
    if stripped {
        found(CloudVendor::Resilio, Confidence::Possible, current)
    } else {
        None
    }
}

/// Rule 6: `<base> <n>.<ext>`, n ≥ 2 without a leading zero, sibling `<base>.<ext>` present.
fn icloud_mac(name: &str, sibling_exists: &dyn Fn(&str) -> bool) -> Option<CloudConflict> {
    let (stem, ext) = split_ext(name);
    let (base, n) = stem.rsplit_once(' ')?;
    let counted = !n.starts_with('0')
        && n.len() <= 18
        && n.bytes().all(|b| b.is_ascii_digit())
        && n.parse::<u64>().is_ok_and(|n| n >= 2);
    if !counted || base.is_empty() {
        return None;
    }
    let sibling = format!("{base}{ext}");
    if sibling_exists(&sibling) {
        found(CloudVendor::ICloudMac, Confidence::Possible, sibling)
    } else {
        None
    }
}

/// Rule 7: `<base> (n).<ext>` or `<base>(0n).<ext>`, sibling `<base>.<ext>` present.
fn icloud_windows(name: &str, sibling_exists: &dyn Fn(&str) -> bool) -> Option<CloudConflict> {
    let (stem, ext) = split_ext(name);
    let inner = stem.strip_suffix(')')?;
    let open = inner.rfind('(')?;
    let digits = &inner[open + 1..];
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let before = &inner[..open];
    let base = match before.strip_suffix(' ') {
        Some(base) => base,
        None if digits.len() >= 2 && digits.starts_with('0') => before,
        None => return None,
    };
    if base.is_empty() {
        return None;
    }
    let sibling = format!("{base}{ext}");
    if sibling_exists(&sibling) {
        found(CloudVendor::ICloudWindows, Confidence::Possible, sibling)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stems_are_sanitised() {
        assert_eq!(file_stem("Plan: Q4 / 2026?"), "Plan- Q4 - 2026-");
        assert_eq!(file_stem("  a \t b  "), "a - b");
        assert_eq!(file_stem("..."), UNTITLED);
        assert_eq!(file_stem("nul"), "nul_");
        assert_eq!(file_stem("CON.md"), "CON_.md");
        assert_eq!(file_stem("Console"), "Console");
    }

    #[test]
    fn cut_respects_char_boundaries() {
        let text = "aé";
        assert_eq!(cut_to(text, 2), "a");
        assert_eq!(cut_to(text, 3), "aé");
        assert_eq!(cut_to(text, 10), "aé");
    }

    #[test]
    fn helpers() {
        assert_eq!(split_ext("a.b.md"), ("a.b", ".md"));
        assert_eq!(split_ext(".hidden"), (".hidden", ""));
        assert_eq!(split_ext("plain"), ("plain", ""));
        assert!(ascii_pattern("x 2026-11-02)", 1, b" DDDD-DD-DD)"));
        assert!(!ascii_pattern("x 2026-11-0", 1, b" DDDD-DD-DD)"));
        assert!(!ascii_pattern("é", 0, b"DD"));
        assert_eq!(
            found(CloudVendor::Resilio, Confidence::Possible, ".md".into()),
            None
        );
    }
}
