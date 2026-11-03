// SPDX-License-Identifier: AGPL-3.0-or-later
//! File names, path keys and cloud conflict names of the files folder (M5 Task 2, spec §5 and §9,
//! M5.md appendix "cloud conflict-file names").

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests: a failure here is the finding"
)]

use dbme_core::files::paths::{
    CloudConflict, CloudVendor, Confidence, MAX_NAME_BYTES, MAX_REL_PATH_BYTES, UNTITLED,
    cloud_conflict, dedupe, file_stem, is_dot_path, join, path_key,
};

#[test]
fn every_forbidden_character_becomes_a_dash() {
    for c in [
        '\\', '/', ':', '*', '?', '"', '<', '>', '|', '#', '^', '[', ']',
    ] {
        assert_eq!(file_stem(&format!("a{c}b")), "a-b", "{c:?}");
    }
    for c in ['\u{0}', '\u{1}', '\t', '\n', '\r', '\u{1f}', '\u{7f}'] {
        assert_eq!(file_stem(&format!("a{c}b")), "a-b", "{c:?}");
    }
    assert_eq!(file_stem("Plan: Q4 / 2026?"), "Plan- Q4 - 2026-");
    // A C1 control or a non-ASCII letter is kept.
    assert_eq!(file_stem("Café — été"), "Café — été");
}

#[test]
fn whitespace_collapses_and_edges_are_trimmed() {
    assert_eq!(file_stem("  a   b\u{a0}\u{2003}c  "), "a b c");
    assert_eq!(file_stem(" .hidden. "), "hidden");
    assert_eq!(file_stem("..."), UNTITLED);
    assert_eq!(file_stem(""), UNTITLED);
    assert_eq!(file_stem("   "), UNTITLED);
    assert_eq!(file_stem(". . ."), UNTITLED);
    assert_eq!(file_stem("a.b"), "a.b");
    assert_eq!(file_stem("v1.0."), "v1.0");
}

#[test]
fn titles_are_nfc_normalised() {
    let decomposed = "Cafe\u{301}";
    let composed = "Caf\u{e9}";
    assert_eq!(file_stem(decomposed), composed);
    assert_eq!(file_stem(composed), composed);
}

#[test]
fn windows_reserved_names_get_an_underscore_after_the_head() {
    for name in [
        "CON",
        "con",
        "PRN",
        "AUX",
        "NUL",
        "COM0",
        "COM1",
        "com9",
        "LPT0",
        "LPT9",
        "lpt3",
        "COM\u{b9}",
        "COM\u{b2}",
        "LPT\u{b3}",
    ] {
        assert_eq!(file_stem(name), format!("{name}_"), "{name}");
    }
    assert_eq!(file_stem("CON.md"), "CON_.md");
    assert_eq!(file_stem("com1.txt"), "com1_.txt");
    assert_eq!(file_stem("Nul.backup.txt"), "Nul_.backup.txt");
    assert_eq!(file_stem("CON "), "CON_");
    for fine in [
        "Console", "COM", "COM10", "LPT", "CONTEXT", "Nulls", "a.CON",
    ] {
        assert_eq!(file_stem(fine), fine, "{fine}");
    }
}

#[test]
fn long_stems_are_cut_on_a_character_boundary() {
    let long = "é".repeat(100);
    let stem = file_stem(&long);
    assert_eq!(stem.len(), 120);
    assert_eq!(stem.chars().count(), 60);
    let odd = format!("a{}", "é".repeat(100));
    let stem = file_stem(&odd);
    assert_eq!(stem.len(), 119, "121 would split an é");
    assert!(stem.starts_with('a') && stem.ends_with('é'));
    let ascii = "x".repeat(300);
    assert_eq!(file_stem(&ascii).len(), MAX_NAME_BYTES);
    // Trailing dots or spaces that the cut exposes are trimmed again.
    let dotted = format!("a{}b", ".".repeat(200));
    assert_eq!(file_stem(&dotted), "a");
    let spaced = format!("{} {}", "a".repeat(119), "b".repeat(50));
    assert_eq!(file_stem(&spaced), "a".repeat(119));
}

#[test]
fn dedupe_counts_from_two() {
    let taken = |s: &str| ["Alpha", "Alpha (2)", "Alpha (3)"].contains(&s);
    assert_eq!(dedupe("Alpha", &taken), "Alpha (4)");
    assert_eq!(dedupe("Beta", &taken), "Beta");
    let taken_once = |s: &str| s == "Alpha";
    assert_eq!(dedupe("Alpha", &taken_once), "Alpha (2)");
    // The suffix fits within the name limit, and the cut never ends in a space.
    let long = "x".repeat(MAX_NAME_BYTES);
    let deduped = dedupe(&long, &|s: &str| s == long);
    assert_eq!(deduped.len(), MAX_NAME_BYTES);
    assert!(deduped.ends_with("x (2)"));
    let spaced = format!("{} {}", "y".repeat(115), "z".repeat(10));
    let deduped = dedupe(&spaced, &|s: &str| s == spaced);
    assert_eq!(deduped, format!("{} (2)", "y".repeat(115)));
    // Many collisions: the count keeps growing.
    let many = |s: &str| s == "N" || (s.starts_with("N (") && s.len() <= "N (99)".len());
    assert_eq!(dedupe("N", &many), "N (100)");
}

#[test]
fn path_keys_fold_case_normalisation_and_separators() {
    assert_eq!(path_key("Projects/Caf\u{e9}.md"), "projects/caf\u{e9}.md");
    assert_eq!(
        path_key("Projects/Cafe\u{301}.md"),
        path_key("projects/CAF\u{c9}.md")
    );
    assert_eq!(path_key("A\\B\\c.md"), "a/b/c.md");
    assert_eq!(path_key("ÄÖÜ/Straße"), "äöü/straße");
    assert_eq!(path_key(""), "");
}

#[test]
fn dot_paths_are_any_component_starting_with_a_dot() {
    assert!(is_dot_path(".obsidian/app.json"));
    assert!(is_dot_path(".dbme/schema.json"));
    assert!(is_dot_path("Notes/.trash/x.md"));
    assert!(is_dot_path("Notes\\.trash\\x.md"));
    assert!(is_dot_path("Notes/.hidden"));
    assert!(!is_dot_path("Notes/v1.0/x.md"));
    assert!(!is_dot_path("Notes/a.b.md"));
    assert!(!is_dot_path("Notes/x.md"));
    assert!(!is_dot_path(""));
}

#[test]
fn join_builds_the_path_and_cuts_the_stem_to_fit() {
    assert_eq!(join(&[], "Alpha"), "Alpha.md");
    assert_eq!(join(&["Work", "Alpha"], "Beta"), "Work/Alpha/Beta.md");
    assert_eq!(join(&["Work", "", "Alpha"], "Beta"), "Work/Alpha/Beta.md");
    let prefix = "p".repeat(100);
    let stem = "s".repeat(150);
    let path = join(&[&prefix], &stem);
    assert_eq!(path.len(), MAX_REL_PATH_BYTES);
    assert!(path.starts_with(&format!("{prefix}/sss")) && path.ends_with(".md"));
    // The cut respects character boundaries.
    let stem = "é".repeat(100);
    let path = join(&[&prefix], &stem);
    assert!(path.len() <= MAX_REL_PATH_BYTES);
    assert!(path.ends_with("é.md"));
    // The stem is never cut below eight bytes, even when the folders alone are too long.
    let deep = "d".repeat(195);
    let path = join(&[&deep], &"s".repeat(50));
    assert_eq!(path, format!("{deep}/{}.md", "s".repeat(8)));
    assert!(path.len() > MAX_REL_PATH_BYTES);
    // A short stem is never touched.
    assert_eq!(join(&[&deep], "ab"), format!("{deep}/ab.md"));
    // A cut that exposes a trailing space or dot trims it.
    let path = join(
        &[&prefix],
        &format!("{} {}", "a".repeat(96), "b".repeat(50)),
    );
    assert_eq!(path, format!("{prefix}/{}.md", "a".repeat(96)));
}

fn none(_: &str) -> bool {
    false
}

fn hit(vendor: CloudVendor, confidence: Confidence, base_name: &str) -> Option<CloudConflict> {
    Some(CloudConflict {
        vendor,
        confidence,
        base_name: base_name.to_owned(),
    })
}

#[test]
fn syncthing_names_are_certain() {
    assert_eq!(
        cloud_conflict("notes.sync-conflict-20261102-143015-ABCDEFG.md", &none),
        hit(CloudVendor::Syncthing, Confidence::Certain, "notes.md")
    );
    assert_eq!(
        cloud_conflict("notes.sync-conflict-20261102-143015-.md", &none),
        hit(CloudVendor::Syncthing, Confidence::Certain, "notes.md"),
        "an empty device id"
    );
    assert_eq!(
        cloud_conflict("notes.sync-conflict-20261102-143015-ABCDEFG", &none),
        hit(CloudVendor::Syncthing, Confidence::Certain, "notes"),
        "no extension"
    );
    assert_eq!(
        cloud_conflict("a.b.sync-conflict-20261102-143015-7777777.tar.gz", &none),
        hit(CloudVendor::Syncthing, Confidence::Certain, "a.b.tar.gz")
    );
    for miss in [
        "notes.sync-conflict-2026112-143015-ABCDEFG.md",
        "notes.sync-conflict-20261102-143015-abcdefg.md",
        "notes.sync-conflict-20261102-143015-ABCDEFGH.md",
        "notes.sync-conflict-20261102-143015-ABC1EFG.md",
        "notes.sync-conflict-20261102-143015ABCDEFG.md",
        ".sync-conflict-20261102-143015-ABCDEFG.md",
        "notes.sync-conflict.md",
    ] {
        assert_eq!(cloud_conflict(miss, &none), None, "{miss}");
    }
}

#[test]
fn nextcloud_names_are_certain() {
    assert_eq!(
        cloud_conflict("mydata (conflicted copy 2018-04-10 093612).txt", &none),
        hit(CloudVendor::Nextcloud, Confidence::Certain, "mydata.txt")
    );
    assert_eq!(
        cloud_conflict("mydata (conflicted copy alex 2018-04-10 093612).txt", &none),
        hit(CloudVendor::Nextcloud, Confidence::Certain, "mydata.txt"),
        "with the user"
    );
    assert_eq!(
        cloud_conflict("mydata (conflicted copy Alex Doe 2018-04-10 093612)", &none),
        hit(CloudVendor::Nextcloud, Confidence::Certain, "mydata"),
        "a user with a space, no extension"
    );
    assert_eq!(
        cloud_conflict("mydata_conflict-20180410-093612.txt", &none),
        hit(CloudVendor::Nextcloud, Confidence::Certain, "mydata.txt"),
        "the legacy form"
    );
    assert_eq!(
        cloud_conflict("mydata (case clash from alex).txt", &none),
        hit(CloudVendor::Nextcloud, Confidence::Certain, "mydata.txt")
    );
    for miss in [
        "mydata (conflicted copy 2018-04-10 09361).txt",
        "mydata (conflicted copy 2018-04-10093612).txt",
        "mydata_conflict-2018041-093612.txt",
        "mydata_conflict-20180410-093612x.txt",
        "mydata (case clash from alex",
    ] {
        assert_eq!(cloud_conflict(miss, &none), None, "{miss}");
    }
}

#[test]
fn dropbox_names_are_likely_when_dated_and_possible_when_not() {
    assert_eq!(
        cloud_conflict("report (Thor's conflicted copy 2014-09-14).pdf", &none),
        hit(CloudVendor::Dropbox, Confidence::Likely, "report.pdf")
    );
    assert_eq!(
        cloud_conflict("report (conflicted copy 2014-09-14).pdf", &none),
        hit(CloudVendor::Dropbox, Confidence::Likely, "report.pdf")
    );
    assert_eq!(
        cloud_conflict("report (conflicted copy).pdf", &none),
        hit(CloudVendor::Dropbox, Confidence::Possible, "report.pdf")
    );
    assert_eq!(
        cloud_conflict("report (Thor's conflicted copy).pdf", &none),
        hit(CloudVendor::Dropbox, Confidence::Possible, "report.pdf")
    );
    assert_eq!(
        cloud_conflict("report (Thor\u{2019}s conflicted copy)", &none),
        hit(CloudVendor::Dropbox, Confidence::Possible, "report"),
        "a typographic apostrophe, no extension"
    );
    assert_eq!(
        cloud_conflict("my (draft) report (conflicted copy).md", &none),
        hit(
            CloudVendor::Dropbox,
            Confidence::Possible,
            "my (draft) report.md"
        ),
        "the last ` (` before the phrase"
    );
    // The listed false positive: a note titled "Draft (conflicted copy)" trips the undated branch.
    assert_eq!(
        cloud_conflict("Draft (conflicted copy).md", &none),
        hit(CloudVendor::Dropbox, Confidence::Possible, "Draft.md")
    );
    for miss in [
        "conflicted copy.md",
        "(conflicted copy).md",
        "report (my conflicted copy).md",
        "report (conflicted copy 2014-9-14).md",
        "report (conflicted copy 2014-09-14.md",
        "report (conflicted copy) final.md",
        "report conflicted copy.md",
    ] {
        assert_eq!(cloud_conflict(miss, &none), None, "{miss}");
    }
}

#[test]
fn onedrive_needs_the_sibling() {
    let siblings = |s: &str| ["Report.txt", "book1.xlsx", "todo.md"].contains(&s);
    assert_eq!(
        cloud_conflict("Report-JOHNS-SURFACE.txt", &siblings),
        hit(CloudVendor::OneDrive, Confidence::Possible, "Report.txt")
    );
    assert_eq!(
        cloud_conflict("book1-desktop-computer.xlsx", &siblings),
        hit(CloudVendor::OneDrive, Confidence::Possible, "book1.xlsx")
    );
    assert_eq!(cloud_conflict("Report-JOHNS-SURFACE.txt", &none), None);
    assert_eq!(cloud_conflict("book1-desktop-computer.xlsx", &none), None);
    // The listed false positive: `todo-laptop.md` next to `todo.md`.
    assert_eq!(
        cloud_conflict("todo-laptop.md", &siblings),
        hit(CloudVendor::OneDrive, Confidence::Possible, "todo.md")
    );
    assert_eq!(cloud_conflict("todo-laptop.md", &none), None);
    assert_eq!(cloud_conflict("-todo.md", &|s: &str| s == ".md"), None);
    assert_eq!(cloud_conflict("todo-.md", &|s: &str| s == "todo.md"), None);
}

#[test]
fn resilio_names_are_possible_and_strip_repeatedly() {
    assert_eq!(
        cloud_conflict("photo.Conflict", &none),
        hit(CloudVendor::Resilio, Confidence::Possible, "photo")
    );
    assert_eq!(
        cloud_conflict("photo.Conflict.jpg", &none),
        hit(CloudVendor::Resilio, Confidence::Possible, "photo.jpg")
    );
    assert_eq!(
        cloud_conflict("photo.Conflict.Conflict.jpg", &none),
        hit(CloudVendor::Resilio, Confidence::Possible, "photo.jpg")
    );
    assert_eq!(
        cloud_conflict("photo.jpg.Conflict.Conflict", &none),
        hit(CloudVendor::Resilio, Confidence::Possible, "photo.jpg")
    );
    // The listed false positive.
    assert_eq!(
        cloud_conflict("merge.Conflict.md", &none),
        hit(CloudVendor::Resilio, Confidence::Possible, "merge.md")
    );
    for miss in [
        "photo.conflict.jpg",
        "Conflict.md",
        ".Conflict",
        ".Conflict.md",
        "photoConflict.md",
    ] {
        assert_eq!(cloud_conflict(miss, &none), None, "{miss}");
    }
}

#[test]
fn icloud_mac_counts_from_two_with_the_sibling_present() {
    let siblings = |s: &str| ["Notes.md", "Chapter.md"].contains(&s);
    assert_eq!(
        cloud_conflict("Notes 2.md", &siblings),
        hit(CloudVendor::ICloudMac, Confidence::Possible, "Notes.md")
    );
    assert_eq!(
        cloud_conflict("Notes 17.md", &siblings),
        hit(CloudVendor::ICloudMac, Confidence::Possible, "Notes.md")
    );
    assert_eq!(cloud_conflict("Notes 2.md", &none), None);
    // The listed false positive: "Chapter 2" trips only when "Chapter.md" exists.
    assert_eq!(
        cloud_conflict("Chapter 2.md", &siblings),
        hit(CloudVendor::ICloudMac, Confidence::Possible, "Chapter.md")
    );
    assert_eq!(cloud_conflict("Chapter 2.md", &none), None);
    for miss in [
        "Notes 1.md",
        "Notes 02.md",
        "Notes2.md",
        "Notes 2a.md",
        " 2.md",
        "Notes 99999999999999999999.md",
    ] {
        assert_eq!(cloud_conflict(miss, &siblings), None, "{miss}");
    }
}

#[test]
fn icloud_windows_numbers_in_parentheses_with_the_sibling_present() {
    let siblings = |s: &str| ["Notes.md", "noteOne.md", "Meeting.md"].contains(&s);
    assert_eq!(
        cloud_conflict("Notes (1).md", &siblings),
        hit(CloudVendor::ICloudWindows, Confidence::Possible, "Notes.md")
    );
    assert_eq!(
        cloud_conflict("noteOne(01).md", &siblings),
        hit(
            CloudVendor::ICloudWindows,
            Confidence::Possible,
            "noteOne.md"
        )
    );
    assert_eq!(cloud_conflict("Notes (1).md", &none), None);
    assert_eq!(cloud_conflict("noteOne(01).md", &none), None);
    // The listed false positive: "Meeting (1)" trips only when "Meeting.md" exists.
    assert_eq!(
        cloud_conflict("Meeting (1).md", &siblings),
        hit(
            CloudVendor::ICloudWindows,
            Confidence::Possible,
            "Meeting.md"
        )
    );
    assert_eq!(cloud_conflict("Meeting (1).md", &none), None);
    for miss in [
        "Notes(1).md",
        "Notes (a).md",
        "Notes ().md",
        "(01).md",
        " (1).md",
    ] {
        assert_eq!(cloud_conflict(miss, &siblings), None, "{miss}");
    }
}

#[test]
fn ordinary_names_and_google_drive_copies_are_not_conflicts() {
    let everything = |_: &str| true;
    for name in [
        "Alpha.md",
        "Alpha (1 of 2).md",
        "2026-11-02 Meeting.md",
        "Notes - draft.md",
        "a-b",
        "Copy of Alpha.md",
        "",
    ] {
        assert_eq!(cloud_conflict(name, &none), None, "{name}");
    }
    assert_eq!(cloud_conflict("Alpha.md", &everything), None);
    assert_eq!(cloud_conflict("Copy of Alpha.md", &everything), None);
}

#[test]
fn confidence_orders_from_certain_to_possible() {
    assert!(Confidence::Certain < Confidence::Likely);
    assert!(Confidence::Likely < Confidence::Possible);
    let json = serde_json::to_value(CloudConflict {
        vendor: CloudVendor::ICloudMac,
        confidence: Confidence::Possible,
        base_name: "Notes.md".into(),
    })
    .unwrap();
    assert_eq!(json["vendor"], "icloud_mac");
    assert_eq!(json["confidence"], "possible");
}
