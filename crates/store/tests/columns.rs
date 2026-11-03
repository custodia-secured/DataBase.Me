// SPDX-License-Identifier: AGPL-3.0-or-later
//! The rules for a Table's columns and rows (ADR-0030): where a column goes, what it may be called,
//! and which types a new row takes. `crates/api/tests/commands.rs` covers them through the commands.

#![allow(
    clippy::unwrap_used,
    reason = "test helpers outside #[test] functions may panic"
)]

mod common;

use dbme_core::Id;
use dbme_core::model::FieldKind;
use dbme_store::{NewColumn, NewObject, NewType, Result, Source, Txn};
use serde_json::json;

use common::{TestVault, new_vault};

fn write<T>(t: &mut TestVault, change: impl FnOnce(&mut Txn<'_>) -> Result<T>) -> Result<T> {
    t.tick();
    t.vault.transact(change).map(|(value, _)| value)
}

fn page(t: &mut TestVault, title: &str, parent: Option<Id>, type_ids: Vec<Id>) -> Id {
    write(t, |txn| {
        txn.create_object(NewObject {
            title: title.into(),
            parent_id: parent,
            type_ids,
            ..NewObject::default()
        })
    })
    .unwrap()
}

fn new_type(t: &mut TestVault, name: &str) -> Id {
    write(t, |txn| {
        txn.create_type(NewType {
            name: name.into(),
            ..NewType::default()
        })
    })
    .unwrap()
}

fn built_in(t: &TestVault, name: &str) -> Id {
    type_named(t, name).unwrap()
}

fn type_named(t: &TestVault, name: &str) -> Option<Id> {
    t.vault
        .read()
        .types()
        .unwrap()
        .into_iter()
        .find(|ty| ty.name == name)
        .map(|ty| ty.id)
}

fn column(label: &str, kind: FieldKind, choices: &[&str]) -> NewColumn {
    NewColumn {
        label: label.into(),
        kind,
        choices: choices.iter().map(|c| (*c).to_owned()).collect(),
    }
}

fn text(label: &str) -> NewColumn {
    column(label, FieldKind::Text, &[])
}

fn add(t: &mut TestVault, source: Source, new: NewColumn) -> Result<Id> {
    write(t, |txn| txn.add_column(source, new))
}

fn children_of(parent: Id) -> Source {
    Source::Children {
        parent: Some(parent),
    }
}

fn labels(t: &TestVault, source: Source) -> Vec<String> {
    t.vault
        .read()
        .fields_in(source)
        .unwrap()
        .into_iter()
        .map(|field| field.label)
        .collect()
}

fn types_of(t: &TestVault, object: Id) -> Vec<Id> {
    t.vault.read().summaries(&[object]).unwrap()[0]
        .type_ids
        .clone()
}

fn shared(t: &TestVault, parent: Id) -> Vec<Id> {
    t.vault
        .read()
        .types_shared_by_children(Some(parent))
        .unwrap()
        .into_iter()
        .map(|ty| ty.id)
        .collect()
}

#[test]
fn the_shared_types_are_those_every_live_child_has() {
    let mut t = new_vault();
    let note = built_in(&t, "Note");
    let task = built_in(&t, "Task");
    let parent = page(&mut t, "Parent", None, vec![]);
    assert!(shared(&t, parent).is_empty());

    page(&mut t, "a", Some(parent), vec![note, task]);
    assert_eq!(shared(&t, parent).len(), 2);

    let b = page(&mut t, "b", Some(parent), vec![note]);
    assert_eq!(shared(&t, parent), [note]);

    let c = page(&mut t, "c", Some(parent), vec![]);
    assert!(shared(&t, parent).is_empty());

    // The Trash does not count, and a type on the row below does not either.
    write(&mut t, |txn| txn.trash_object(c)).unwrap();
    assert_eq!(shared(&t, parent), [note]);
    page(&mut t, "below", Some(b), vec![task]);
    assert_eq!(shared(&t, parent), [note]);
}

#[test]
fn the_target_is_the_listed_type_then_a_shared_type_then_the_pages_name() {
    let mut t = new_vault();
    let note = built_in(&t, "Note");
    let parent = page(&mut t, "Devices", None, vec![]);
    let source = children_of(parent);

    // A Table of a type: that type, built-in or not.
    let target = t
        .vault
        .read()
        .column_target(Source::Type { type_id: note })
        .unwrap();
    assert_eq!(target.type_id, Some(note));
    assert_eq!(target.type_name, "Note");

    // No children, or children with only a built-in type: the page's name, not yet a type.
    let target = t.vault.read().column_target(source).unwrap();
    assert_eq!(
        (target.type_id, target.type_name.as_str()),
        (None, "Devices")
    );
    let a = page(&mut t, "a", Some(parent), vec![note]);
    let target = t.vault.read().column_target(source).unwrap();
    assert_eq!(
        (target.type_id, target.type_name.as_str()),
        (None, "Devices")
    );

    // A type of that name is used, whether or not the children have it yet.
    let devices = new_type(&mut t, "Devices");
    let target = t.vault.read().column_target(source).unwrap();
    assert_eq!(target.type_id, Some(devices));

    // A shared type comes first, the first by name.
    let phones = new_type(&mut t, "Phones");
    let cases = new_type(&mut t, "Cases");
    write(&mut t, |txn| {
        txn.add_type(a, phones)?;
        txn.add_type(a, cases)
    })
    .unwrap();
    let target = t.vault.read().column_target(source).unwrap();
    assert_eq!(target.type_id, Some(cases));
    assert_eq!(target.type_name, "Cases");
}

#[test]
fn the_pages_name_is_made_fit_for_a_type() {
    let mut t = new_vault();
    let name_for = |t: &mut TestVault, title: &str| {
        let parent = page(t, title, None, vec![]);
        t.vault
            .read()
            .column_target(children_of(parent))
            .unwrap()
            .type_name
    };
    assert_eq!(name_for(&mut t, ""), "Untitled");
    assert_eq!(name_for(&mut t, "   "), "Untitled");
    assert_eq!(name_for(&mut t, "  Devices "), "Devices");
    assert_eq!(name_for(&mut t, "Journal"), "Journal pages");
    let long = name_for(&mut t, &"é".repeat(300));
    assert_eq!(long.chars().count(), 190);
}

#[test]
fn the_top_level_and_a_missing_type_are_not_targets() {
    let mut t = new_vault();
    let top = Source::Children { parent: None };
    assert!(t.vault.read().column_target(top).is_err());
    assert!(add(&mut t, top, text("X")).is_err());

    let gone = new_type(&mut t, "Gone");
    write(&mut t, |txn| txn.delete_type(gone)).unwrap();
    assert!(
        t.vault
            .read()
            .column_target(Source::Type { type_id: gone })
            .is_err()
    );
}

#[test]
fn a_column_goes_on_the_target_and_children_get_it() {
    let mut t = new_vault();
    let note = built_in(&t, "Note");
    let parent = page(&mut t, "Devices", None, vec![]);
    let a = page(&mut t, "a", Some(parent), vec![note]);
    let b = page(&mut t, "b", Some(parent), vec![]);
    let source = children_of(parent);

    let field = add(&mut t, source, text("Serial")).unwrap();
    let devices = type_named(&t, "Devices").unwrap();
    assert_eq!(t.vault.read().field(field).unwrap().type_id, devices);
    assert!(types_of(&t, a).contains(&note));
    assert!(types_of(&t, a).contains(&devices));
    assert_eq!(types_of(&t, b), [devices]);
    assert!(
        t.vault
            .read()
            .fields_in(Source::Type { type_id: note })
            .unwrap()
            .is_empty()
    );

    // A second column changes no types: everything already has this one.
    let before = t.vault.read().types().unwrap().len();
    add(&mut t, source, text("Owner")).unwrap();
    assert_eq!(t.vault.read().types().unwrap().len(), before);
    assert_eq!(labels(&t, source), ["Serial", "Owner"]);
}

#[test]
fn a_column_on_a_type_changes_no_pages() {
    let mut t = new_vault();
    let task = built_in(&t, "Task");
    let parent = page(&mut t, "Parent", None, vec![]);
    let child = page(&mut t, "child", Some(parent), vec![]);

    let source = Source::Type { type_id: task };
    add(&mut t, source, text("Owner")).unwrap();
    assert_eq!(labels(&t, source), ["Owner"]);
    assert!(types_of(&t, child).is_empty());
    assert_eq!(t.vault.read().count_of_type(task).unwrap(), 0);
}

#[test]
fn an_empty_table_gets_a_first_row_of_the_type() {
    let mut t = new_vault();
    let parent = page(&mut t, "Empty", None, vec![]);
    let source = children_of(parent);

    add(&mut t, source, text("Serial")).unwrap();
    let rows = t.vault.read().children(Some(parent)).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].title, "");
    assert_eq!(types_of(&t, rows[0].id), [type_named(&t, "Empty").unwrap()]);
    assert_eq!(labels(&t, source), ["Serial"]);
}

#[test]
fn a_column_that_is_refused_leaves_nothing_behind() {
    let mut t = new_vault();
    let parent = page(&mut t, "Parent", None, vec![]);
    let child = page(&mut t, "child", Some(parent), vec![]);
    let source = children_of(parent);
    let types = t.vault.read().types().unwrap().len();

    for refused in [
        text(""),
        text("   "),
        text("Title"),
        text(" TITLE "),
        text(&"x".repeat(201)),
        column("Where", FieldKind::Geo, &[]),
        column("Status", FieldKind::Select, &[]),
        column("Status", FieldKind::Select, &["  ", ""]),
    ] {
        let label = refused.label.clone();
        assert!(add(&mut t, source, refused).is_err(), "{label:?}");
        assert_eq!(t.vault.read().types().unwrap().len(), types, "{label:?}");
        assert!(types_of(&t, child).is_empty(), "{label:?}");
    }
}

#[test]
fn a_label_cannot_repeat_one_the_table_shows() {
    let mut t = new_vault();
    let parent = page(&mut t, "Parent", None, vec![]);
    let source = children_of(parent);
    add(&mut t, source, text("Serial")).unwrap();
    assert!(add(&mut t, source, text("  sERIAL ")).is_err());

    // Nor one on the target type that this Table does not show yet.
    let hidden = new_type(&mut t, "Hidden");
    add(&mut t, Source::Type { type_id: hidden }, text("Secret")).unwrap();
    let other = page(&mut t, "Hidden", None, vec![]);
    assert!(add(&mut t, children_of(other), text("secret")).is_err());
    add(&mut t, children_of(other), text("Open")).unwrap();
}

#[test]
fn keys_come_from_labels_and_stay_distinct() {
    let mut t = new_vault();
    let parent = page(&mut t, "Parent", None, vec![]);
    let source = children_of(parent);
    add(&mut t, source, text("Due date")).unwrap();
    add(&mut t, source, text("Due-date!")).unwrap();
    add(&mut t, source, text("Due date?")).unwrap();
    add(&mut t, source, text("名前")).unwrap();
    let keys: Vec<String> = t
        .vault
        .read()
        .fields_in(source)
        .unwrap()
        .into_iter()
        .map(|field| field.key)
        .collect();
    assert_eq!(keys, ["due_date", "due_date_2", "due_date_3", "column_4"]);
}

#[test]
fn a_select_lists_its_distinct_choices() {
    let mut t = new_vault();
    let parent = page(&mut t, "Parent", None, vec![]);
    let field = add(
        &mut t,
        children_of(parent),
        column(
            "Status",
            FieldKind::Select,
            &["To do", " Doing ", "", "To do", "To-do", "完了"],
        ),
    )
    .unwrap();
    assert_eq!(
        t.vault.read().field(field).unwrap().options_json,
        json!({ "options": [
            { "id": "to_do", "label": "To do" },
            { "id": "doing", "label": "Doing" },
            { "id": "to_do_2", "label": "To-do" },
            { "id": "choice_4", "label": "完了" },
        ] })
    );

    // Choices mean nothing to any other kind.
    let plain = add(
        &mut t,
        children_of(parent),
        column("Note", FieldKind::Text, &["ignored"]),
    )
    .unwrap();
    assert_eq!(t.vault.read().field(plain).unwrap().options_json, json!({}));
}

#[test]
fn every_kind_a_cell_edits_can_be_a_column() {
    let mut t = new_vault();
    let parent = page(&mut t, "Parent", None, vec![]);
    let source = children_of(parent);
    for (label, kind) in [
        ("A", FieldKind::Text),
        ("B", FieldKind::Url),
        ("C", FieldKind::Email),
        ("D", FieldKind::Phone),
        ("E", FieldKind::Number),
        ("F", FieldKind::Boolean),
        ("G", FieldKind::Date),
    ] {
        add(&mut t, source, column(label, kind, &[])).unwrap();
    }
    assert_eq!(labels(&t, source).len(), 7);
}

#[test]
fn a_new_row_takes_the_types_of_its_table() {
    let mut t = new_vault();
    let note = built_in(&t, "Note");
    let task = built_in(&t, "Task");
    let parent = page(&mut t, "Parent", None, vec![]);

    // A Table of a type: that type, under the page the Table is on.
    let source = Source::Type { type_id: task };
    let row = write(&mut t, |txn| txn.add_row(source, parent)).unwrap();
    assert_eq!(types_of(&t, row), [task]);
    assert_eq!(t.vault.read().object(row).unwrap().parent_id, Some(parent));

    // A Table of children: what they all share.
    let mixed = page(&mut t, "Mixed", None, vec![]);
    page(&mut t, "a", Some(mixed), vec![note, task]);
    page(&mut t, "b", Some(mixed), vec![note]);
    let row = write(&mut t, |txn| txn.add_row(children_of(mixed), mixed)).unwrap();
    assert_eq!(types_of(&t, row), [note]);
    assert_eq!(t.vault.read().object(row).unwrap().title, "");

    // Nothing shared, nothing given.
    let empty = page(&mut t, "Empty", None, vec![]);
    let row = write(&mut t, |txn| txn.add_row(children_of(empty), empty)).unwrap();
    assert!(types_of(&t, row).is_empty());
}

#[test]
fn a_field_is_renamed_within_the_rules_and_keeps_its_key() {
    let mut t = new_vault();
    let parent = page(&mut t, "Parent", None, vec![]);
    let source = children_of(parent);
    let serial = add(&mut t, source, text("Serial")).unwrap();
    add(&mut t, source, text("Owner")).unwrap();

    assert!(write(&mut t, |txn| txn.rename_field(serial, " Serial no. ")).unwrap());
    let field = t.vault.read().field(serial).unwrap();
    assert_eq!(field.label, "Serial no.");
    assert_eq!(field.key, "serial");

    assert!(!write(&mut t, |txn| txn.rename_field(serial, "Serial no.")).unwrap());
    assert!(write(&mut t, |txn| txn.rename_field(serial, "SERIAL NO.")).unwrap());
    for refused in ["owner", "Title", "  "] {
        assert!(
            write(&mut t, |txn| txn.rename_field(serial, refused)).is_err(),
            "{refused:?}"
        );
    }
    assert_eq!(labels(&t, source), ["SERIAL NO.", "Owner"]);

    let missing = write(&mut t, |txn| {
        txn.delete_field(serial)?;
        txn.rename_field(serial, "Again")
    });
    assert!(missing.is_err());
}
