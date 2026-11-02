// SPDX-License-Identifier: AGPL-3.0-or-later
//! The three-way merge of the files folder (M5 Task 2, spec §8 and §9, ADR-0025): the identities,
//! disjoint and clashing frontmatter edits, the set merge of `types` and `tags`, relations, links
//! and extras, disjoint block edits, adjacent and far-apart line edits in one paragraph, edit
//! against delete, both delete, insertions on both sides, reorder, reparent, the empty base,
//! determinism, and, as proptest properties over small random documents, the three identities and
//! the no-loss rule: every value a side wrote is in the result or in a `Conflict`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests: a failure here is the finding"
)]

use dbme_core::files::{
    Conflict, ConflictSite, FileBlock, FileDoc, Frontmatter, LinkEntry, Merged, NoResolver,
    RelationRef, Schema, assign_ids, merge, normalize,
};
use dbme_core::{Id, SortKey};
use proptest::prelude::*;
use serde_json::{Value, json};

fn id(n: u128) -> Id {
    Id::from_parts(1_000, n)
}

fn text(s: &str) -> Value {
    json!({ "inline": [{ "text": s }] })
}

fn block(n: u128, kind: &str, content: Value, children: Vec<FileBlock>) -> FileBlock {
    FileBlock {
        id: Some(id(n)),
        kind: kind.to_owned(),
        content,
        children,
    }
}

fn para(n: u128, s: &str) -> FileBlock {
    block(n, "paragraph", text(s), Vec::new())
}

fn bullet(n: u128, s: &str, children: Vec<FileBlock>) -> FileBlock {
    block(n, "bullet", text(s), children)
}

fn code(n: u128, s: &str) -> FileBlock {
    block(n, "code", json!({ "code": s }), Vec::new())
}

fn front() -> Frontmatter {
    Frontmatter {
        id: Some(id(900)),
        types: vec!["Note".to_owned()],
        sort: Some(SortKey::first()),
        ..Frontmatter::default()
    }
}

fn doc(body: Vec<FileBlock>) -> FileDoc {
    FileDoc {
        front: front(),
        body,
    }
}

fn with_fields(fields: &[(&str, Value)]) -> FileDoc {
    let mut d = doc(Vec::new());
    d.front.fields = fields
        .iter()
        .map(|(k, v)| ((*k).to_owned(), v.clone()))
        .collect();
    d
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

fn run(base: &FileDoc, ours: &FileDoc, theirs: &FileDoc) -> Merged {
    merge(base, ours, theirs, &Schema::default(), &NoResolver)
}

fn clean(base: &FileDoc, ours: &FileDoc, theirs: &FileDoc) -> FileDoc {
    let merged = run(base, ours, theirs);
    assert!(merged.conflicts.is_empty(), "{:#?}", merged.conflicts);
    merged.doc
}

fn top_ids(doc: &FileDoc) -> Vec<Option<Id>> {
    doc.body.iter().map(|b| b.id).collect()
}

fn sites(merged: &Merged) -> Vec<&ConflictSite> {
    merged.conflicts.iter().map(|c| &c.site).collect()
}

fn find(blocks: &[FileBlock], n: u128) -> Option<&FileBlock> {
    blocks.iter().find_map(|b| {
        if b.id == Some(id(n)) {
            Some(b)
        } else {
            find(&b.children, n)
        }
    })
}

fn relation(n: u128) -> RelationRef {
    RelationRef {
        id: Some(id(n)),
        link: None,
    }
}

fn link(n: u128, rel_type: &str) -> LinkEntry {
    LinkEntry {
        id: Some(id(n)),
        rel_type: rel_type.to_owned(),
        ..LinkEntry::default()
    }
}

/// A base and a document that differs from it everywhere: fields, sets, envelope, blocks edited,
/// deleted, added, reordered and reparented.
fn pair() -> (FileDoc, FileDoc) {
    let mut base = with_fields(&[("status", json!("open")), ("due", json!("2026-11-30"))]);
    base.front.tags = strings(&["a", "b"]);
    base.front.relations = vec![("related".to_owned(), vec![relation(1)])];
    base.front.links = vec![link(5, "related")];
    base.body = vec![
        para(1, "one"),
        bullet(2, "two", vec![para(3, "three")]),
        code(4, "x = 1"),
        para(5, "five"),
    ];
    let mut x = with_fields(&[("due", json!("2026-12-01")), ("count", json!(3))]);
    x.front.types = strings(&["Task", "Note"]);
    x.front.tags = strings(&["b", "c"]);
    x.front.summary = Some("changed".to_owned());
    x.front.relations = vec![("related".to_owned(), vec![relation(1), relation(2)])];
    x.front.links = vec![link(6, "cites"), link(5, "related")];
    x.front.extra = vec![("aliases".to_owned(), json!(["X"]))];
    x.body = vec![
        code(4, "x = 2"),
        bullet(2, "two", vec![para(5, "five"), para(3, "THREE")]),
        para(6, "six"),
    ];
    (base, x)
}

// --- identities ------------------------------------------------------------------------------

#[test]
fn the_three_identities_hold() {
    let (b, x) = pair();
    let mut normal = x.clone();
    normalize(&mut normal);
    assert_eq!(normal, x, "the hand-built document must be in normal form");
    assert_eq!(clean(&b, &b, &x), x);
    assert_eq!(clean(&b, &x, &b), x);
    assert_eq!(clean(&b, &x, &x), x);
    assert_eq!(clean(&b, &b, &b), b);
    assert_eq!(clean(&x, &x, &x), x);
}

// --- frontmatter -----------------------------------------------------------------------------

#[test]
fn disjoint_frontmatter_edits_merge_clean() {
    let base = with_fields(&[("status", json!("open")), ("due", json!("2026-11-30"))]);
    let mut ours = base.clone();
    ours.front.summary = Some("ours wrote this".to_owned());
    ours.front.fields[1].1 = json!("2026-12-24");
    let mut theirs = base.clone();
    theirs.front.fields[0].1 = json!("done");
    theirs.front.fields.push(("count".to_owned(), json!(3)));
    theirs.front.created = Some(1_700_000_000_000);
    let merged = clean(&base, &ours, &theirs);
    assert_eq!(merged.front.summary.as_deref(), Some("ours wrote this"));
    assert_eq!(merged.front.created, Some(1_700_000_000_000));
    assert_eq!(
        merged.front.fields,
        [
            ("status".to_owned(), json!("done")),
            ("due".to_owned(), json!("2026-12-24")),
            ("count".to_owned(), json!(3)),
        ]
    );
}

#[test]
fn the_same_field_changed_both_ways_conflicts_and_ours_wins() {
    let base = with_fields(&[("status", json!("open")), ("due", json!("2026-11-30"))]);
    let mut ours = base.clone();
    ours.front.fields[0].1 = json!("doing");
    ours.front.fields.remove(1);
    ours.front.summary = Some("o".to_owned());
    let mut theirs = base.clone();
    theirs.front.fields[0].1 = json!("done");
    theirs.front.fields[1].1 = json!("2027-01-01");
    theirs.front.summary = Some("t".to_owned());
    let merged = run(&base, &ours, &theirs);
    assert_eq!(
        merged.doc.front.fields,
        [("status".to_owned(), json!("doing"))]
    );
    assert_eq!(merged.doc.front.summary.as_deref(), Some("o"));
    assert_eq!(
        merged.conflicts,
        [
            Conflict {
                site: ConflictSite::Envelope("summary"),
                base: Value::Null,
                ours: json!("o"),
                theirs: json!("t"),
                diff3: None,
            },
            Conflict {
                site: ConflictSite::Field("status".to_owned()),
                base: json!("open"),
                ours: json!("doing"),
                theirs: json!("done"),
                diff3: None,
            },
            // Deleted by ours, edited by theirs: a conflict too, and the deletion stands.
            Conflict {
                site: ConflictSite::Field("due".to_owned()),
                base: json!("2026-11-30"),
                ours: Value::Null,
                theirs: json!("2027-01-01"),
                diff3: None,
            },
        ]
    );
}

#[test]
fn types_and_tags_merge_as_sets_without_conflict() {
    let mut base = doc(Vec::new());
    base.front.types = strings(&["A", "B"]);
    base.front.tags = strings(&["x", "y", "z"]);
    let mut ours = base.clone();
    ours.front.types = strings(&["A", "B", "C"]);
    ours.front.tags = strings(&["z", "x", "new"]);
    let mut theirs = base.clone();
    theirs.front.types = strings(&["B", "D"]);
    theirs.front.tags = strings(&["x", "y", "z", "theirs"]);
    let merged = clean(&base, &ours, &theirs);
    // A deleted by theirs, C added by ours, D added by theirs, in ours' order then theirs'.
    assert_eq!(merged.front.types, strings(&["B", "C", "D"]));
    // y deleted by ours, `new` and `theirs` added.
    assert_eq!(merged.front.tags, strings(&["z", "x", "new", "theirs"]));
    // Both add the same type: once.
    theirs.front.types = strings(&["A", "B", "C"]);
    assert_eq!(
        clean(&base, &ours, &theirs).front.types,
        strings(&["A", "B", "C"])
    );
}

#[test]
fn relations_links_and_extras_are_sites_of_their_own() {
    let mut base = doc(Vec::new());
    base.front.relations = vec![("related".to_owned(), vec![relation(1)])];
    base.front.links = vec![link(5, "related")];
    base.front.extra = vec![("aliases".to_owned(), json!(["A"]))];
    // Ours relabels a link (same set); theirs adds a link: theirs' list is taken whole.
    let mut ours = base.clone();
    ours.front.links[0].label = Some("ours".to_owned());
    ours.front.relations[0].1.push(relation(2));
    let mut theirs = base.clone();
    theirs.front.links.push(link(6, "cites"));
    theirs.front.extra[0].1 = json!(["A", "B"]);
    let merged = clean(&base, &ours, &theirs);
    assert_eq!(merged.front.links, [link(5, "related"), link(6, "cites")]);
    assert_eq!(merged.front.relations[0].1, [relation(1), relation(2)]);
    assert_eq!(merged.front.extra[0].1, json!(["A", "B"]));

    // Three ways everywhere: ours stands, each site is one conflict. (A relabel alone is not a
    // change of the links set; adding a link is.)
    ours.front.links.push(link(8, "cites"));
    theirs.front.relations[0].1 = vec![relation(3)];
    theirs.front.links = vec![link(7, "related")];
    ours.front.extra[0].1 = json!(["C"]);
    let merged = run(&base, &ours, &theirs);
    assert_eq!(
        sites(&merged),
        [
            &ConflictSite::Relations("related".to_owned()),
            &ConflictSite::Links,
            &ConflictSite::Field("aliases".to_owned()),
        ]
    );
    assert_eq!(merged.doc.front.links, ours.front.links);
    assert_eq!(merged.doc.front.relations, ours.front.relations);
    assert_eq!(merged.doc.front.extra, ours.front.extra);
    let links = &merged.conflicts[1];
    assert_eq!(links.base, json!([link(5, "related")]));
    assert_eq!(links.theirs, json!([link(7, "related")]));
    assert_eq!(links.diff3, None);
}

// --- blocks ----------------------------------------------------------------------------------

#[test]
fn disjoint_block_edits_merge_clean() {
    let base = doc(vec![para(1, "a"), para(2, "b"), code(3, "c")]);
    let mut ours = base.clone();
    ours.body[0] = para(1, "A");
    let mut theirs = base.clone();
    theirs.body[2] = code(3, "C");
    assert_eq!(
        clean(&base, &ours, &theirs),
        doc(vec![para(1, "A"), para(2, "b"), code(3, "C")])
    );
}

#[test]
fn adjacent_line_edits_in_one_paragraph_conflict_with_diff3_text() {
    let base = doc(vec![para(1, "one\ntwo\nthree\nfour"), para(2, "other")]);
    let ours = doc(vec![para(1, "one\nTWO\nthree\nfour"), para(2, "other")]);
    let theirs = doc(vec![para(1, "one\ntwo\nTHREE\nfour"), para(2, "other")]);
    let merged = run(&base, &ours, &theirs);
    assert_eq!(merged.doc, ours);
    assert_eq!(merged.conflicts.len(), 1, "{:#?}", merged.conflicts);
    let conflict = &merged.conflicts[0];
    assert_eq!(conflict.site, ConflictSite::Block(id(1)));
    assert_eq!(conflict.base, text("one\ntwo\nthree\nfour"));
    assert_eq!(conflict.ours, text("one\nTWO\nthree\nfour"));
    assert_eq!(conflict.theirs, text("one\ntwo\nTHREE\nfour"));
    assert_eq!(
        conflict.diff3.as_deref(),
        Some(
            "one\n<<<<<<< ours\nTWO\nthree\n||||||| original\ntwo\nthree\n=======\ntwo\nTHREE\n>>>>>>> theirs\nfour"
        )
    );
    // A one-line paragraph edited twice is the same thing: all or nothing (ADR-0025 rule 3).
    let merged = run(
        &doc(vec![para(1, "hello world")]),
        &doc(vec![para(1, "hello brave world")]),
        &doc(vec![para(1, "hello world!")]),
    );
    assert_eq!(sites(&merged), [&ConflictSite::Block(id(1))]);
    assert!(
        merged.conflicts[0]
            .diff3
            .as_deref()
            .unwrap()
            .ends_with(">>>>>>> theirs\n")
    );
}

#[test]
fn far_apart_line_edits_in_one_paragraph_merge() {
    let lines: Vec<String> = (1..=10).map(|i| format!("line {i}")).collect();
    let mut ours = lines.clone();
    ours[1] = "LINE 2 (ours)".to_owned();
    let mut theirs = lines.clone();
    theirs[8] = "LINE 9 (theirs)".to_owned();
    let mut both = lines.clone();
    both[1] = ours[1].clone();
    both[8] = theirs[8].clone();
    let merged = clean(
        &doc(vec![para(1, &lines.join("\n"))]),
        &doc(vec![para(1, &ours.join("\n"))]),
        &doc(vec![para(1, &theirs.join("\n"))]),
    );
    assert_eq!(merged, doc(vec![para(1, &both.join("\n"))]));
}

#[test]
fn code_and_todo_blocks_merge_as_text_too() {
    let base = doc(vec![code(1, "a\nb\nc\nd\ne")]);
    let ours = doc(vec![code(1, "A\nb\nc\nd\ne")]);
    let theirs = doc(vec![code(1, "a\nb\nc\nd\nE")]);
    assert_eq!(
        clean(&base, &ours, &theirs),
        doc(vec![code(1, "A\nb\nc\nd\nE")])
    );
    let todo = |checked: bool, s: &str| {
        block(
            2,
            "todo",
            json!({ "inline": [{ "text": s }], "checked": checked }),
            Vec::new(),
        )
    };
    // Ours ticks the box (line 1), theirs edits line 3: clean, both taken.
    let merged = clean(
        &doc(vec![todo(false, "one\ntwo\nthree")]),
        &doc(vec![todo(true, "one\ntwo\nthree")]),
        &doc(vec![todo(false, "one\ntwo\nTHREE")]),
    );
    assert_eq!(merged, doc(vec![todo(true, "one\ntwo\nTHREE")]));
    // Ticking the box and editing the first line touch the same line: a conflict.
    let merged = run(
        &doc(vec![todo(false, "one")]),
        &doc(vec![todo(true, "one")]),
        &doc(vec![todo(false, "ONE")]),
    );
    assert_eq!(sites(&merged), [&ConflictSite::Block(id(2))]);
    assert!(merged.conflicts[0].diff3.is_some());
    // A kind without text: both changed is a conflict without diff3 text.
    let image = |alt: &str| {
        block(
            3,
            "image",
            json!({ "blob": format!("blake3:{}", "0".repeat(64)), "alt": alt }),
            Vec::new(),
        )
    };
    let merged = run(
        &doc(vec![image("a")]),
        &doc(vec![image("b")]),
        &doc(vec![image("c")]),
    );
    assert_eq!(merged.doc, doc(vec![image("b")]));
    assert_eq!(sites(&merged), [&ConflictSite::Block(id(3))]);
    assert_eq!(merged.conflicts[0].diff3, None);
}

#[test]
fn edit_against_delete_keeps_the_edited_block_and_conflicts() {
    let base = doc(vec![para(1, "a"), para(2, "b"), para(3, "c")]);
    // Ours deletes 2, theirs edits it: kept at theirs' position, with theirs' text.
    let ours = doc(vec![para(1, "a"), para(3, "c")]);
    let theirs = doc(vec![para(1, "a"), para(2, "B"), para(3, "c")]);
    let merged = run(&base, &ours, &theirs);
    assert_eq!(merged.doc, theirs);
    assert_eq!(merged.conflicts.len(), 1);
    let conflict = &merged.conflicts[0];
    assert_eq!(conflict.site, ConflictSite::Block(id(2)));
    assert_eq!(conflict.ours, Value::Null);
    assert_eq!(conflict.theirs["content"], text("B"));
    assert_eq!(conflict.base["content"], text("b"));
    assert_eq!(conflict.diff3, None);
    // The other way round: ours edits, theirs deletes.
    let merged = run(&base, &theirs, &ours);
    assert_eq!(merged.doc, theirs);
    assert_eq!(merged.conflicts[0].theirs, Value::Null);
    assert_eq!(merged.conflicts[0].ours["content"], text("B"));
    // Moving a block among its siblings is not an edit of the block: the deletion stands.
    let moved = doc(vec![para(2, "b"), para(1, "a"), para(3, "c")]);
    assert_eq!(clean(&base, &moved, &ours), ours);
    // Moving it under another parent is: the block is kept there.
    let base = doc(vec![bullet(1, "l", Vec::new()), para(2, "b")]);
    let nested = doc(vec![bullet(1, "l", vec![para(2, "b")])]);
    let deleted = doc(vec![bullet(1, "l", Vec::new())]);
    let merged = run(&base, &nested, &deleted);
    assert_eq!(merged.doc, nested);
    assert_eq!(sites(&merged), [&ConflictSite::Block(id(2))]);
}

#[test]
fn deletions_are_clean_when_the_other_side_left_the_block_alone() {
    let base = doc(vec![para(1, "a"), para(2, "b"), para(3, "c")]);
    let without = doc(vec![para(1, "a"), para(3, "c")]);
    // Both delete.
    assert_eq!(clean(&base, &without, &without), without);
    // One deletes, the other leaves it.
    assert_eq!(clean(&base, &without, &base), without);
    assert_eq!(clean(&base, &base, &without), without);
    // Neighbouring deletions on either side: no spurious order conflict.
    let ours = doc(vec![para(1, "a"), para(3, "c")]);
    let theirs = doc(vec![para(1, "a"), para(2, "b")]);
    assert_eq!(clean(&base, &ours, &theirs), doc(vec![para(1, "a")]));
    // Deleting everything on one side while the other edits a field.
    let mut edited = base.clone();
    edited.front.summary = Some("s".to_owned());
    let mut empty = doc(Vec::new());
    empty.front.summary = Some("s".to_owned());
    assert_eq!(clean(&base, &edited, &doc(Vec::new())), empty);
}

#[test]
fn insertions_on_both_sides_are_both_kept() {
    let base = doc(vec![para(1, "a"), para(2, "b"), para(3, "c")]);
    // Different positions: clean.
    let ours = doc(vec![
        para(1, "a"),
        para(10, "x"),
        para(2, "b"),
        para(3, "c"),
    ]);
    let theirs = doc(vec![
        para(1, "a"),
        para(2, "b"),
        para(20, "y"),
        para(3, "c"),
    ]);
    let merged = clean(&base, &ours, &theirs);
    assert_eq!(top_ids(&merged), [1, 10, 2, 20, 3].map(|n| Some(id(n))));
    // The same position: the order conflicts, ours' order stands, theirs' block follows its
    // predecessor in theirs, and nothing is lost.
    let ours = doc(vec![para(1, "a"), para(10, "x"), para(2, "b")]);
    let theirs = doc(vec![para(1, "a"), para(20, "y"), para(2, "b")]);
    let merged = run(&doc(vec![para(1, "a"), para(2, "b")]), &ours, &theirs);
    assert_eq!(top_ids(&merged.doc), [1, 20, 10, 2].map(|n| Some(id(n))));
    assert_eq!(sites(&merged), [&ConflictSite::Children(None)]);
    let conflict = &merged.conflicts[0];
    assert_eq!(conflict.base, json!([id(1).to_string(), id(2).to_string()]));
    assert_eq!(
        conflict.ours,
        json!([id(1).to_string(), id(10).to_string(), id(2).to_string()])
    );
    assert_eq!(
        conflict.theirs,
        json!([id(1).to_string(), id(20).to_string(), id(2).to_string()])
    );
    let diff3 = conflict.diff3.as_deref().unwrap();
    assert!(diff3.contains("<<<<<<< ours\n") && diff3.contains(&id(20).to_string()));
    // Both insert the same new block at the same place: clean.
    let same = doc(vec![para(1, "a"), para(10, "x"), para(2, "b")]);
    assert_eq!(
        clean(&doc(vec![para(1, "a"), para(2, "b")]), &same, &same),
        same
    );
}

#[test]
fn a_reorder_on_one_side_is_taken() {
    let base = doc(vec![para(1, "a"), para(2, "b"), para(3, "c")]);
    let reordered = doc(vec![para(3, "c"), para(1, "a"), para(2, "b")]);
    assert_eq!(clean(&base, &base, &reordered), reordered);
    assert_eq!(clean(&base, &reordered, &base), reordered);
    // Reorder on one side, an edit on the other: both.
    let edited = doc(vec![para(1, "a"), para(2, "B"), para(3, "c")]);
    assert_eq!(
        clean(&base, &reordered, &edited),
        doc(vec![para(3, "c"), para(1, "a"), para(2, "B")])
    );
    // Inside a list item, the same.
    let base = doc(vec![bullet(1, "l", vec![para(2, "a"), para(3, "b")])]);
    let swapped = doc(vec![bullet(1, "l", vec![para(3, "b"), para(2, "a")])]);
    assert_eq!(clean(&base, &base, &swapped), swapped);
    // Two different reorders of the same list: a `Children(parent)` conflict, ours' order.
    let other = doc(vec![bullet(
        1,
        "l",
        vec![para(3, "b"), para(4, "c"), para(2, "a")],
    )]);
    let ours = doc(vec![bullet(
        1,
        "l",
        vec![para(4, "c"), para(2, "a"), para(3, "b")],
    )]);
    let merged = run(&base, &ours, &other);
    assert_eq!(merged.doc, ours);
    assert_eq!(sites(&merged), [&ConflictSite::Children(Some(id(1)))]);
}

#[test]
fn a_reparent_on_one_side_is_taken() {
    let base = doc(vec![bullet(1, "list", Vec::new()), para(2, "p")]);
    let nested = doc(vec![bullet(1, "list", vec![para(2, "p")])]);
    assert_eq!(clean(&base, &base, &nested), nested);
    assert_eq!(clean(&base, &nested, &base), nested);
    // Reparent on one side, edit the moved block on the other: both.
    let edited = doc(vec![bullet(1, "list", Vec::new()), para(2, "P")]);
    assert_eq!(
        clean(&base, &edited, &nested),
        doc(vec![bullet(1, "list", vec![para(2, "P")])])
    );
    // Out of a list: the same.
    assert_eq!(clean(&nested, &nested, &base), base);
    // Two different new parents: a `Block` conflict, ours' parent.
    let base = doc(vec![
        bullet(1, "a", Vec::new()),
        bullet(3, "b", Vec::new()),
        para(2, "p"),
    ]);
    let ours = doc(vec![
        bullet(1, "a", vec![para(2, "p")]),
        bullet(3, "b", Vec::new()),
    ]);
    let theirs = doc(vec![
        bullet(1, "a", Vec::new()),
        bullet(3, "b", vec![para(2, "p")]),
    ]);
    let merged = run(&base, &ours, &theirs);
    assert_eq!(merged.doc, ours);
    assert_eq!(sites(&merged), [&ConflictSite::Block(id(2))]);
    assert_eq!(merged.conflicts[0].base, Value::Null);
    assert_eq!(merged.conflicts[0].ours, json!(id(1)));
    assert_eq!(merged.conflicts[0].theirs, json!(id(3)));
    // A block whose new parent the other side deleted is not lost: it joins the top level.
    let base = doc(vec![bullet(1, "a", Vec::new()), para(2, "p")]);
    let ours = doc(vec![bullet(1, "a", vec![para(2, "p")])]);
    let theirs = doc(vec![para(2, "p")]);
    let merged = clean(&base, &ours, &theirs);
    assert_eq!(merged, theirs);
}

#[test]
fn an_empty_base_is_clean_only_when_both_sides_agree() {
    let base = FileDoc::default();
    let ours = doc(vec![para(1, "same"), para(2, "ours only")]);
    let mut theirs = ours.clone();
    theirs.front.tags = strings(&["t"]);
    let merged = clean(&base, &ours, &theirs);
    assert_eq!(top_ids(&merged), [1, 2].map(|n| Some(id(n))));
    assert_eq!(merged.front.tags, strings(&["t"]));
    // Different blocks on each side: all kept, but with no base the order of two insertions at
    // the same place is a `Children` conflict (diffy, gate 6): a shared prefix does not help.
    let theirs = doc(vec![para(1, "same"), para(3, "theirs only")]);
    let merged = run(&base, &ours, &theirs);
    // Ours' order, then theirs' block right after its predecessor in theirs (block 1).
    assert_eq!(top_ids(&merged.doc), [1, 3, 2].map(|n| Some(id(n))));
    assert_eq!(sites(&merged), [&ConflictSite::Children(None)]);
    // Different texts in the same block: a conflict with an empty original.
    let ours = doc(vec![para(1, "same")]);
    let theirs = doc(vec![para(1, "different")]);
    let merged = run(&base, &ours, &theirs);
    assert_eq!(merged.doc, ours);
    assert_eq!(sites(&merged), [&ConflictSite::Block(id(1))]);
    assert_eq!(merged.conflicts[0].base, Value::Null);
    assert_eq!(
        merged.conflicts[0].diff3.as_deref(),
        Some("<<<<<<< ours\nsame\n||||||| original\n=======\ndifferent\n>>>>>>> theirs\n")
    );
    // Different kinds in the same block: a conflict, ours' kind.
    let theirs = doc(vec![block(
        1,
        "heading",
        json!({ "inline": [{ "text": "same" }], "level": 2 }),
        Vec::new(),
    )]);
    let merged = run(&base, &ours, &theirs);
    assert_eq!(merged.doc, ours);
    // The kind and the content (the heading's level) each differed three ways.
    assert_eq!(
        sites(&merged),
        [&ConflictSite::Block(id(1)), &ConflictSite::Block(id(1))]
    );
    assert_eq!(merged.conflicts[0].ours, json!("paragraph"));
    assert_eq!(merged.conflicts[0].theirs, json!("heading"));
    assert_eq!(merged.conflicts[1].ours, text("same"));
    assert_eq!(merged.conflicts[1].diff3, None);
}

#[test]
fn a_kind_change_on_one_side_merges_with_an_edit_on_the_other() {
    let base = doc(vec![para(1, "one\ntwo\nthree")]);
    let ours = doc(vec![block(1, "quote", text("one\ntwo\nthree"), Vec::new())]);
    let theirs = doc(vec![para(1, "one\ntwo\nTHREE")]);
    assert_eq!(
        clean(&base, &ours, &theirs),
        doc(vec![block(1, "quote", text("one\ntwo\nTHREE"), Vec::new())])
    );
}

#[test]
fn blocks_without_ids_are_kept_as_additions_of_their_side() {
    let anon = |s: &str| FileBlock {
        id: None,
        kind: "paragraph".to_owned(),
        content: text(s),
        children: Vec::new(),
    };
    let base = doc(vec![para(1, "a")]);
    let ours = doc(vec![para(1, "a"), anon("ours typed this")]);
    let theirs = doc(vec![anon("theirs typed this"), para(1, "a")]);
    let merged = clean(&base, &ours, &theirs);
    assert_eq!(
        merged.body,
        [
            anon("theirs typed this"),
            para(1, "a"),
            anon("ours typed this")
        ]
    );
    // With ids assigned first, the same documents merge the same way, by id.
    let mut n = 100;
    let mut next = || {
        n += 1;
        id(n)
    };
    let (mut ours, mut theirs) = (ours, theirs);
    assign_ids(&mut ours, &mut next);
    assign_ids(&mut theirs, &mut next);
    let merged = clean(&base, &ours, &theirs);
    assert_eq!(top_ids(&merged), [102, 1, 101].map(|n| Some(id(n))));
}

#[test]
fn merging_is_deterministic() {
    let (base, x) = pair();
    let mut y = x.clone();
    y.front.summary = Some("y".to_owned());
    y.front.fields[0].1 = json!("2026-12-31");
    y.body = vec![
        bullet(2, "TWO", vec![para(3, "three!"), para(5, "FIVE")]),
        para(7, "seven"),
        code(4, "x = 3"),
        para(1, "one!"),
    ];
    let first = run(&base, &x, &y);
    assert!(!first.conflicts.is_empty());
    for _ in 0..100 {
        assert_eq!(run(&base, &x, &y), first);
    }
}

// --- properties over small random documents --------------------------------------------------

const WORDS: [&str; 5] = ["alpha", "bravo", "charlie", "delta", "echo"];
const TYPES: [&str; 3] = ["Note", "Task", "Person"];
const TAGS: [&str; 3] = ["a", "b", "c"];
const KEYS: [&str; 3] = ["status", "due", "count"];
/// How many block ids a random body draws from; ids are `1..=BLOCKS`.
const BLOCKS: u8 = 6;

/// One to three lines from the vocabulary: normal-form text.
fn arb_text() -> impl Strategy<Value = String> {
    prop::collection::vec(0usize..WORDS.len(), 1..=3)
        .prop_map(|ws| ws.iter().map(|w| WORDS[*w]).collect::<Vec<_>>().join("\n"))
}

/// A block's draw: kind (0 paragraph, 1 bullet, 2 code), text, wanted parent (a smaller index),
/// and a sort byte for its place among siblings.
type Draw = (u8, String, Option<u8>, u8);

fn arb_draw(index: u8) -> BoxedStrategy<Option<Draw>> {
    let parent = if index == 0 {
        Just(None).boxed()
    } else {
        prop::option::of(0..index).boxed()
    };
    prop::option::of((0u8..3, arb_text(), parent, any::<u8>())).boxed()
}

/// A tree out of the draws: a block's parent is the one it wanted when that block exists and is
/// a bullet (a smaller index, so there is never a cycle), else the top level; siblings by sort
/// byte then index.
fn build_body(draws: &[Option<Draw>]) -> Vec<FileBlock> {
    let parent_of = |i: usize| -> Option<usize> {
        let (_, _, wanted, _) = draws[i].as_ref()?;
        let p = usize::from((*wanted)?);
        matches!(draws.get(p), Some(Some((1, ..)))).then_some(p)
    };
    fn children(
        parent: Option<usize>,
        draws: &[Option<Draw>],
        parent_of: &dyn Fn(usize) -> Option<usize>,
    ) -> Vec<FileBlock> {
        let mut members: Vec<(u8, usize)> = (0..draws.len())
            .filter(|i| draws[*i].is_some() && parent_of(*i) == parent)
            .map(|i| (draws[i].as_ref().map_or(0, |d| d.3), i))
            .collect();
        members.sort_unstable();
        members
            .into_iter()
            .map(|(_, i)| {
                let (kind, text_, _, _) = draws[i].as_ref().unwrap();
                let n = i as u128 + 1;
                match kind {
                    0 => para(n, text_),
                    1 => bullet(n, text_, children(Some(i), draws, parent_of)),
                    _ => code(n, text_),
                }
            })
            .collect()
    }
    children(None, draws, &parent_of)
}

fn arb_body() -> impl Strategy<Value = Vec<FileBlock>> {
    (0..BLOCKS)
        .map(arb_draw)
        .collect::<Vec<_>>()
        .prop_map(|draws| build_body(&draws))
}

fn arb_front() -> impl Strategy<Value = Frontmatter> {
    let picks = |pool: &'static [&'static str], min: usize| {
        prop::collection::vec(0..pool.len(), min..=pool.len()).prop_map(move |is| {
            let mut out: Vec<String> = Vec::new();
            for i in is {
                if !out.contains(&pool[i].to_owned()) {
                    out.push(pool[i].to_owned());
                }
            }
            out
        })
    };
    let field_value = |k: usize| -> Value {
        match k {
            0 => json!("open"),
            1 => json!("2026-11-30"),
            _ => json!(3),
        }
    };
    (
        picks(&TYPES, 1),
        picks(&TAGS, 0),
        prop::option::of(0usize..WORDS.len()),
        prop::collection::vec(prop::option::of(0usize..3), KEYS.len()),
        prop::option::of(prop::collection::vec(1u128..5, 0..=2)),
        prop::collection::vec((1u128..4, 0usize..2), 0..=2),
        prop::option::of(0usize..WORDS.len()),
    )
        .prop_map(
            move |(types, tags, summary, fields, related, links, alias)| Frontmatter {
                id: Some(id(900)),
                types,
                tags,
                summary: summary.map(|w| WORDS[w].to_owned()),
                sort: Some(SortKey::first()),
                fields: fields
                    .iter()
                    .enumerate()
                    .filter_map(|(k, v)| {
                        v.map(|v| (KEYS[k].to_owned(), json!([field_value(k), v])))
                    })
                    .collect(),
                relations: related
                    .map(|ids| {
                        vec![(
                            "related".to_owned(),
                            ids.into_iter().map(relation).collect(),
                        )]
                    })
                    .unwrap_or_default(),
                links: links
                    .into_iter()
                    .map(|(n, t)| link(n, ["related", "cites"][t]))
                    .collect(),
                extra: alias
                    .map(|w| vec![("aliases".to_owned(), json!([WORDS[w]]))])
                    .unwrap_or_default(),
                ..Frontmatter::default()
            },
        )
}

fn arb_doc() -> impl Strategy<Value = FileDoc> {
    (arb_front(), arb_body()).prop_map(|(front, body)| FileDoc { front, body })
}

/// Every block of a tree by id, with its parent.
fn flat(blocks: &[FileBlock]) -> Vec<(Id, Option<Id>, &FileBlock)> {
    fn walk<'a>(
        blocks: &'a [FileBlock],
        parent: Option<Id>,
        out: &mut Vec<(Id, Option<Id>, &'a FileBlock)>,
    ) {
        for b in blocks {
            out.push((b.id.unwrap(), parent, b));
            walk(&b.children, b.id, out);
        }
    }
    let mut out = Vec::new();
    walk(blocks, None, &mut out);
    out
}

/// The lines of a generated block's text (one text run, or `code`).
fn lines_of(block: &FileBlock) -> Vec<String> {
    let text_ = block
        .content
        .get("code")
        .or_else(|| block.content["inline"][0].get("text"))
        .and_then(Value::as_str)
        .unwrap_or("");
    text_.lines().map(str::to_owned).collect()
}

/// The no-loss rule for one side: every value `side` wrote (one that differs from the base) is in
/// the merged document or in a conflict at its site.
fn check_no_loss(
    base: &FileDoc,
    side: &FileDoc,
    merged: &Merged,
    which: &str,
) -> Result<(), TestCaseError> {
    let has_conflict = |site: &ConflictSite| merged.conflicts.iter().any(|c| &c.site == site);
    let doc = &merged.doc;
    let out = format!("{which} lost a value:\nbase {base:#?}\nside {side:#?}\nmerged {merged:#?}");

    // Blocks: a block the side changed (or added) survives with its content, or its lines are in
    // the merged text, or its site is a conflict.
    let base_blocks = flat(&base.body);
    let merged_blocks = flat(&doc.body);
    for (bid, _, block) in flat(&side.body) {
        let in_base = base_blocks.iter().find(|(i, ..)| *i == bid);
        if in_base.is_some_and(|(_, _, b)| b.kind == block.kind && b.content == block.content) {
            continue;
        }
        if has_conflict(&ConflictSite::Block(bid)) {
            continue;
        }
        let Some((_, _, result)) = merged_blocks.iter().find(|(i, ..)| *i == bid) else {
            return Err(TestCaseError::fail(format!("block {bid} vanished: {out}")));
        };
        if result.content == block.content {
            continue;
        }
        let base_lines = in_base.map(|(_, _, b)| lines_of(b)).unwrap_or_default();
        let result_lines = lines_of(result);
        for line in lines_of(block) {
            prop_assert!(
                base_lines.contains(&line) || result_lines.contains(&line),
                "line {line:?} of block {bid}: {out}"
            );
        }
    }
    // Frontmatter.
    let keyed = |side_list: &[(String, Value)],
                 base_list: &[(String, Value)],
                 result: &[(String, Value)],
                 site: &dyn Fn(&str) -> ConflictSite|
     -> Result<(), TestCaseError> {
        for (k, v) in side_list {
            let unchanged = base_list.iter().any(|(bk, bv)| bk == k && bv == v);
            let kept = result.iter().any(|(rk, rv)| rk == k && rv == v);
            prop_assert!(
                unchanged || kept || has_conflict(&site(k)),
                "field {k}: {out}"
            );
        }
        Ok(())
    };
    keyed(
        &side.front.fields,
        &base.front.fields,
        &doc.front.fields,
        &|k| ConflictSite::Field(k.to_owned()),
    )?;
    keyed(
        &side.front.extra,
        &base.front.extra,
        &doc.front.extra,
        &|k| ConflictSite::Field(k.to_owned()),
    )?;
    for (k, v) in &side.front.relations {
        let unchanged = base
            .front
            .relations
            .iter()
            .any(|(bk, bv)| bk == k && bv == v);
        let kept = doc
            .front
            .relations
            .iter()
            .any(|(rk, rv)| rk == k && rv == v);
        prop_assert!(
            unchanged || kept || has_conflict(&ConflictSite::Relations(k.clone())),
            "relations {k}: {out}"
        );
    }
    for t in &side.front.types {
        prop_assert!(
            base.front.types.contains(t) || doc.front.types.contains(t),
            "type {t}: {out}"
        );
    }
    for t in &side.front.tags {
        prop_assert!(
            base.front.tags.contains(t) || doc.front.tags.contains(t),
            "tag {t}: {out}"
        );
    }
    let key = |l: &LinkEntry| (l.id, l.rel_type.clone(), l.block);
    for l in &side.front.links {
        let unchanged = base.front.links.iter().any(|b| key(b) == key(l));
        let kept = doc.front.links.iter().any(|r| key(r) == key(l));
        prop_assert!(
            unchanged || kept || has_conflict(&ConflictSite::Links),
            "link {l:?}: {out}"
        );
    }
    if side.front.summary != base.front.summary {
        prop_assert!(
            doc.front.summary == side.front.summary
                || has_conflict(&ConflictSite::Envelope("summary")),
            "summary: {out}"
        );
    }
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn generated_documents_are_in_normal_form(x in arb_doc()) {
        let mut normal = x.clone();
        normalize(&mut normal);
        prop_assert_eq!(normal, x);
    }

    #[test]
    fn identities_hold_for_random_documents(b in arb_doc(), x in arb_doc()) {
        for (ours, theirs) in [(&b, &x), (&x, &b), (&x, &x)] {
            let merged = run(&b, ours, theirs);
            prop_assert!(merged.conflicts.is_empty(), "{:#?}", merged.conflicts);
            prop_assert_eq!(&merged.doc, &x);
        }
        let same = run(&b, &b, &b);
        prop_assert!(same.conflicts.is_empty());
        prop_assert_eq!(same.doc, b);
    }

    #[test]
    fn nothing_either_side_wrote_is_lost(b in arb_doc(), o in arb_doc(), t in arb_doc()) {
        let merged = run(&b, &o, &t);
        check_no_loss(&b, &o, &merged, "ours")?;
        check_no_loss(&b, &t, &merged, "theirs")?;
        // The result is in normal form, every block has an id once, and the merge is a function.
        let mut normal = merged.doc.clone();
        normalize(&mut normal);
        prop_assert_eq!(&normal, &merged.doc);
        let mut ids: Vec<Id> = flat(&merged.doc.body).into_iter().map(|(i, ..)| i).collect();
        let n = ids.len();
        ids.sort_unstable();
        ids.dedup();
        prop_assert_eq!(ids.len(), n);
        prop_assert_eq!(run(&b, &o, &t), merged);
    }

    #[test]
    fn a_side_that_only_edits_other_blocks_than_the_other_merges_clean(
        b in arb_doc(),
        edits in prop::collection::vec((0..BLOCKS, 0usize..WORDS.len()), 0..=2),
    ) {
        // Ours edits even-indexed blocks, theirs odd-indexed ones: disjoint, so always clean.
        let mut ours = b.clone();
        let mut theirs = b.clone();
        for (i, w) in edits {
            let target = if i % 2 == 0 { &mut ours } else { &mut theirs };
            fn edit(blocks: &mut [FileBlock], n: u128, word: &str) {
                for block in blocks {
                    if block.id == Some(id(n)) {
                        block.content = if block.kind == "code" {
                            json!({ "code": word })
                        } else {
                            text(word)
                        };
                    }
                    edit(&mut block.children, n, word);
                }
            }
            edit(&mut target.body, u128::from(i) + 1, WORDS[w]);
        }
        let merged = run(&b, &ours, &theirs);
        prop_assert!(merged.conflicts.is_empty(), "{:#?}", merged.conflicts);
        for n in 1..=u128::from(BLOCKS) {
            for side in [&ours, &theirs] {
                if let (Some(base_block), Some(block)) = (find(&b.body, n), find(&side.body, n))
                    && base_block.content != block.content
                {
                    prop_assert_eq!(
                        find(&merged.doc.body, n).map(|r| &r.content),
                        Some(&block.content)
                    );
                }
            }
        }
    }
}

/// A flat note of many anchored paragraphs with one insertion on each side merges cleanly in
/// time linear in the block count (review 2026-11-02: `Vec::contains` per child made it
/// quadratic, tens of seconds at 200k blocks).
#[test]
fn a_flat_note_of_many_blocks_merges_quickly() {
    const N: u128 = 40_000;
    let base_blocks: Vec<FileBlock> = (1..=N).map(|n| para(n, "x")).collect();
    let base = doc(base_blocks);
    let mut ours = base.clone();
    ours.body.insert(0, para(N + 1, "ours"));
    let mut theirs = base.clone();
    theirs.body.push(para(N + 2, "theirs"));
    let start = std::time::Instant::now();
    let merged = clean(&base, &ours, &theirs);
    let elapsed = start.elapsed();
    assert_eq!(merged.body.len(), usize::try_from(N).unwrap() + 2);
    assert_eq!(merged.body[0].id, Some(id(N + 1)));
    assert_eq!(merged.body.last().unwrap().id, Some(id(N + 2)));
    assert!(elapsed.as_secs() < 30, "{elapsed:?}");
}

/// The deepest block of a tree, counting the top level as 1.
fn depth_of(blocks: &[FileBlock]) -> usize {
    let mut deepest = 0;
    let mut stack: Vec<(&FileBlock, usize)> = blocks.iter().map(|b| (b, 1)).collect();
    while let Some((b, depth)) = stack.pop() {
        deepest = deepest.max(depth);
        stack.extend(b.children.iter().map(|c| (c, depth + 1)));
    }
    deepest
}

/// Parent links that chain deeper together than on either side alone never overflow the
/// stack: the merge places what would sit past the depth cap at the top level.
#[test]
fn a_merge_whose_parents_chain_past_the_cap_flattens() {
    // Base: 300 top-level paragraphs. Ours nests the odd ones under their predecessors, theirs
    // the even ones, so the merged parent links form one chain of 300.
    let n = 300u128;
    let base = doc((1..=n).map(|i| para(i, "x")).collect());
    fn deepest(b: &mut FileBlock) -> &mut FileBlock {
        if b.children.is_empty() {
            b
        } else {
            deepest(&mut b.children[0])
        }
    }
    let nest = |pick: fn(u128) -> bool| -> FileDoc {
        let mut blocks: Vec<FileBlock> = Vec::new();
        for i in 1..=n {
            let block = para(i, "x");
            if i > 1 && pick(i) {
                deepest(blocks.last_mut().unwrap()).children.push(block);
            } else {
                blocks.push(block);
            }
        }
        doc(blocks)
    };
    let ours = nest(|i| i % 2 == 1);
    let theirs = nest(|i| i % 2 == 0);
    assert!(depth_of(&ours.body) <= 2 && depth_of(&theirs.body) <= 2);
    let merged = std::thread::Builder::new()
        .stack_size(1 << 20)
        .spawn(move || run(&base, &ours, &theirs))
        .unwrap()
        .join()
        .unwrap();
    assert!(merged.conflicts.is_empty(), "{:#?}", merged.conflicts);
    assert!(depth_of(&merged.doc.body) <= 64);
    let mut count = 0;
    let mut stack: Vec<&FileBlock> = merged.doc.body.iter().collect();
    while let Some(b) = stack.pop() {
        count += 1;
        stack.extend(&b.children);
    }
    assert_eq!(count, 300);
}
