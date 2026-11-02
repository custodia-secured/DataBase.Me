// SPDX-License-Identifier: AGPL-3.0-or-later
//! Gate 1d (M5, Decision 17): what `diffy::merge` does inside one block. Each test asserts
//! the crate's real behaviour, so the file is the record of the finding, not a wish list.
//! Where the plan assumed otherwise, the comment says so.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "gate probes: a failure here is the finding"
)]

use std::time::Instant;

use diffy::{ConflictStyle, IncompleteHunkStyle, MergeOptions, merge, merge_bytes};

const BASE: &str = "one\ntwo\nthree\nfour\nfive\n";
const EDITED: &str = "one\nTWO\nthree\nfour\nFIVE\n";

/// `MergeOptions` with the given style and the default marker length of 7.
fn options(style: ConflictStyle) -> MergeOptions {
    let mut o = MergeOptions::new();
    o.set_conflict_style(style);
    o
}

// 1. The three identities hold for multi-line texts.
#[test]
fn identities_hold() {
    assert_eq!(merge(BASE, BASE, EDITED), Ok(EDITED.to_owned()));
    assert_eq!(merge(BASE, EDITED, BASE), Ok(EDITED.to_owned()));
    assert_eq!(merge(BASE, EDITED, EDITED), Ok(EDITED.to_owned()));
    assert_eq!(merge(BASE, BASE, BASE), Ok(BASE.to_owned()));
}

// 2. Disjoint lines merge clean; the same line conflicts.
#[test]
fn disjoint_lines_merge_and_the_same_line_conflicts() {
    let ours = "ONE\ntwo\nthree\nfour\nfive\n";
    let theirs = "one\ntwo\nthree\nfour\nFIVE\n";
    assert_eq!(
        merge(BASE, ours, theirs),
        Ok("ONE\ntwo\nthree\nfour\nFIVE\n".to_owned())
    );
    let same_line = merge(
        BASE,
        "one\nTWO\nthree\nfour\nfive\n",
        "one\n2\nthree\nfour\nfive\n",
    );
    assert!(same_line.is_err(), "{same_line:?}");
}

// 2. Adjacent lines (2 and 3) CONFLICT: the two changed ranges touch, so diff3 sees one
// region changed three ways. The plan's "disjoint-block edits always clean" holds only across
// blocks; inside a block two neighbouring lines are not disjoint for diffy. Git behaves the same.
#[test]
fn adjacent_line_edits_conflict() {
    let ours = "one\nTWO\nthree\nfour\nfive\n";
    let theirs = "one\ntwo\nTHREE\nfour\nfive\n";
    let result = merge(BASE, ours, theirs);
    assert_eq!(
        result,
        Err("one\n<<<<<<< ours\nTWO\nthree\n||||||| original\ntwo\nthree\n=======\ntwo\nTHREE\n>>>>>>> theirs\nfour\nfive\n".to_owned())
    );
    // One untouched line between the edits is enough to keep them apart.
    let theirs = "one\ntwo\nthree\nFOUR\nfive\n";
    assert_eq!(
        merge(BASE, ours, theirs),
        Ok("one\nTWO\nthree\nFOUR\nfive\n".to_owned())
    );
}

// 3. A dropped trailing newline is an edit of the last line, so it conflicts with any other edit
// of that line. Lines carry their `\n`, so "b" and "b\n" are different lines to diffy.
#[test]
fn a_dropped_trailing_newline_is_an_edit_of_the_last_line() {
    let conflict = merge("a\nb\n", "a\nb", "a\nB\n");
    assert!(conflict.is_err(), "{conflict:?}");
    // Dropping it alone is taken as our change.
    assert_eq!(merge("a\nb\n", "a\nb", "a\nb\n"), Ok("a\nb".to_owned()));
    // Dropping it while the other side edits the line just above: the two changed lines are
    // adjacent, so this is the adjacency conflict again, not a clean merge.
    assert_eq!(
        merge("a\nb\n", "a\nb", "A\nb\n"),
        Err("<<<<<<< ours\na\nb||||||| original\na\nb\n=======\nA\nb\n>>>>>>> theirs\n".to_owned())
    );
    // With an untouched line between them it merges clean.
    assert_eq!(
        merge("a\nb\nc\n", "a\nb\nc", "A\nb\nc\n"),
        Ok("A\nb\nc".to_owned())
    );
    // No side has a trailing newline: an ordinary merge, the same adjacency rule.
    assert_eq!(merge("a\nb", "a\nb", "a\nB"), Ok("a\nB".to_owned()));
    assert!(merge("a\nb", "A\nb", "a\nB").is_err());
    assert_eq!(
        merge("a\nb\nc", "A\nb\nc", "a\nb\nC"),
        Ok("A\nb\nC".to_owned())
    );
}

// 4. CRLF is not normalised. The plan marked this "not verified"; it is now: a line-ending
// switch is a change of every line, taken when the other side is untouched and a whole-block
// conflict when the other side edited anything.
#[test]
fn crlf_is_a_change_of_every_line() {
    let crlf = BASE.replace('\n', "\r\n");
    // Theirs only converts: taken as theirs, so the result is CRLF.
    assert_eq!(merge(BASE, BASE, &crlf), Ok(crlf.clone()));
    // Ours edits one line, theirs converts: every line differs three ways.
    let result = merge(BASE, "one\nTWO\nthree\nfour\nfive\n", &crlf);
    assert_eq!(
        result,
        Err(format!(
            "<<<<<<< ours\none\nTWO\nthree\nfour\nfive\n||||||| original\n{BASE}=======\n{crlf}>>>>>>> theirs\n"
        ))
    );
    // Mixed endings in one input are kept as they are, line by line.
    let mixed = "one\ntwo\nthree\nfour\r\nfive\n";
    assert_eq!(
        merge(BASE, "ONE\ntwo\nthree\nfour\nfive\n", mixed),
        Ok("ONE\ntwo\nthree\nfour\r\nfive\n".to_owned())
    );
    // The same text, both sides CRLF, is an identity.
    assert_eq!(merge(BASE, &crlf, &crlf), Ok(crlf));
}

// 5. The exact `Err` shape, both styles, default and shortened markers. The side labels are
// fixed: `ours`, `original`, `theirs`.
#[test]
fn conflict_markers_have_a_fixed_shape() {
    let (base, ours, theirs) = ("a\nx\nz\n", "a\ny\nz\n", "a\nw\nz\n");
    // Default `MergeOptions::new()` is Diff3, length 7.
    assert_eq!(
        merge(base, ours, theirs),
        Err("a\n<<<<<<< ours\ny\n||||||| original\nx\n=======\nw\n>>>>>>> theirs\nz\n".to_owned())
    );
    assert_eq!(
        options(ConflictStyle::Diff3).merge(base, ours, theirs),
        merge(base, ours, theirs)
    );
    assert_eq!(
        options(ConflictStyle::Merge).merge(base, ours, theirs),
        Err("a\n<<<<<<< ours\ny\n=======\nw\n>>>>>>> theirs\nz\n".to_owned())
    );
    let mut short = options(ConflictStyle::Diff3);
    short.set_conflict_marker_length(3);
    assert_eq!(
        short.merge(base, ours, theirs),
        Err("a\n<<< ours\ny\n||| original\nx\n===\nw\n>>> theirs\nz\n".to_owned())
    );
    // Parsing the Err back: each marker line is marker * length, then an optional label.
    let text = merge(base, ours, theirs).unwrap_err();
    let markers: Vec<&str> = text
        .lines()
        .filter(|l| {
            l.starts_with("<<<<<<<")
                || l.starts_with("|||||||")
                || l.starts_with("=======")
                || l.starts_with(">>>>>>>")
        })
        .collect();
    assert_eq!(
        markers,
        [
            "<<<<<<< ours",
            "||||||| original",
            "=======",
            ">>>>>>> theirs"
        ]
    );
}

// 6. An empty base (first enable, Decision 20b): two different texts conflict as one region
// with an empty original; the same text on both sides is clean.
#[test]
fn an_empty_base_conflicts_unless_both_sides_agree() {
    assert_eq!(
        merge("", "x\n", "y\n"),
        Err("<<<<<<< ours\nx\n||||||| original\n=======\ny\n>>>>>>> theirs\n".to_owned())
    );
    assert_eq!(merge("", "x\n", "x\n"), Ok("x\n".to_owned()));
    assert_eq!(merge("", "x\n", ""), Ok("x\n".to_owned()));
    assert_eq!(merge("", "", ""), Ok(String::new()));
    // A shared prefix does not help: inserts are not matched line by line against each other.
    let shared = merge("", "same\nx\n", "same\ny\n");
    assert!(shared.is_err(), "{shared:?}");
}

// 7. The same insertion, the same deletion, and edit versus delete.
#[test]
fn same_changes_merge_and_edit_against_delete_conflicts() {
    // Both insert the same line in the same place.
    let inserted = "one\ntwo\nNEW\nthree\nfour\nfive\n";
    assert_eq!(merge(BASE, inserted, inserted), Ok(inserted.to_owned()));
    // Both delete the same lines.
    let deleted = "one\nfour\nfive\n";
    assert_eq!(merge(BASE, deleted, deleted), Ok(deleted.to_owned()));
    // One deletes a line the other edits: a conflict with an empty side.
    let result = merge(
        BASE,
        "one\nthree\nfour\nfive\n",
        "one\nTWO\nthree\nfour\nfive\n",
    );
    assert_eq!(
        result,
        Err("one\n<<<<<<< ours\n||||||| original\ntwo\n=======\nTWO\n>>>>>>> theirs\nthree\nfour\nfive\n".to_owned())
    );
    // Both insert different lines at the same place: a conflict with an empty original.
    let result = merge(
        BASE,
        "one\ntwo\nA\nthree\nfour\nfive\n",
        "one\ntwo\nB\nthree\nfour\nfive\n",
    );
    assert!(result.is_err(), "{result:?}");
}

// 8. Line granularity, nothing finer. A one-line paragraph with two different edits always
// conflicts, so inside a paragraph block the merge is all or nothing (Decision 17). With the
// default `IncompleteHunkStyle::Diff3` the markers are glued onto the incomplete line, which
// breaks a line-by-line parse of the Err; `IncompleteHunkStyle::Git` puts each marker on its own
// line. The inbox must set `Git` or append a newline before merging.
#[test]
fn a_single_line_without_newline_merges_at_line_granularity() {
    let (base, ours, theirs) = ("hello world", "hello brave world", "hello world!");
    assert_eq!(
        merge(base, ours, theirs),
        Err("<<<<<<< ours\nhello brave world||||||| original\nhello world=======\nhello world!>>>>>>> theirs\n".to_owned())
    );
    let mut git = MergeOptions::new();
    git.set_incomplete_hunk_style(IncompleteHunkStyle::Git);
    assert_eq!(
        git.merge(base, ours, theirs),
        Err("<<<<<<< ours\nhello brave world\n||||||| original\nhello world\n=======\nhello world!\n>>>>>>> theirs\n".to_owned())
    );
    // One side unchanged: the other's edit is taken, as for any line.
    assert_eq!(merge(base, base, theirs), Ok(theirs.to_owned()));
}

// 9. `merge_bytes` merges non-UTF-8 input and keeps the bytes as they are.
#[test]
fn merge_bytes_takes_non_utf8_input() {
    let base: &[u8] = b"a\n\xff\xfe\nc\n";
    let ours: &[u8] = b"A\n\xff\xfe\nc\n";
    let theirs: &[u8] = b"a\n\xff\xfe\nC\n";
    assert_eq!(
        merge_bytes(base, ours, theirs),
        Ok(b"A\n\xff\xfe\nC\n".to_vec())
    );
    // A conflict renders the same markers as bytes.
    let theirs: &[u8] = b"a\n\xfe\xff\nc\n";
    let conflict = merge_bytes(base, b"a\n\x80\nc\n", theirs);
    assert_eq!(
        conflict,
        Err(b"a\n<<<<<<< ours\n\x80\n||||||| original\n\xff\xfe\n=======\n\xfe\xff\n>>>>>>> theirs\nc\n".to_vec())
    );
    // `MergeOptions` applies to bytes too: the Merge style drops the original section.
    let styled = options(ConflictStyle::Merge).merge_bytes(base, b"a\n\x80\nc\n", theirs);
    assert_eq!(
        styled,
        Err(b"a\n<<<<<<< ours\n\x80\n=======\n\xfe\xff\n>>>>>>> theirs\nc\n".to_vec())
    );
}

// 10. 10,000 lines: edits far apart and identical content, each well under the 2 s budget.
#[test]
fn ten_thousand_lines_merge_quickly() {
    let lines: Vec<String> = (0..10_000).map(|i| format!("line {i}\n")).collect();
    let base = lines.concat();
    let mut ours = lines.clone();
    ours[10] = "line 10 ours\n".into();
    let ours = ours.concat();
    let mut theirs = lines.clone();
    theirs[9_990] = "line 9990 theirs\n".into();
    let theirs = theirs.concat();

    let start = Instant::now();
    let merged = merge(&base, &ours, &theirs).unwrap();
    let far_apart = start.elapsed();
    assert!(merged.contains("line 10 ours\n") && merged.contains("line 9990 theirs\n"));
    assert_eq!(merged.lines().count(), 10_000);

    let start = Instant::now();
    assert_eq!(merge(&base, &base, &base), Ok(base.clone()));
    let identical = start.elapsed();

    println!(
        "10,000 lines: edits far apart {} ms, identical {} ms",
        far_apart.as_millis(),
        identical.as_millis()
    );
    assert!(far_apart.as_secs_f64() < 2.0, "{far_apart:?}");
    assert!(identical.as_secs_f64() < 2.0, "{identical:?}");
}

// 11. The same conflict merge gives the same text every time: the line classifier assigns IDs in
// order of first sight, so hashing does not leak into the output.
#[test]
fn a_conflict_merge_is_deterministic() {
    let ours = "ONE\n2\nthree\nFOUR\nfive\n";
    let theirs = "one\nTWO\nthree\n4\nFIVE\n";
    let first = merge(BASE, ours, theirs);
    assert!(first.is_err(), "{first:?}");
    // Lines 1 and 2 coalesce into one region and lines 4 and 5 into another: two conflicts.
    assert_eq!(first.as_ref().unwrap_err().matches("<<<<<<<").count(), 2);
    for _ in 0..100 {
        assert_eq!(merge(BASE, ours, theirs), first);
        assert_eq!(
            options(ConflictStyle::Merge).merge(BASE, ours, theirs),
            options(ConflictStyle::Merge).merge(BASE, ours, theirs)
        );
    }
}
