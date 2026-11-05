// SPDX-License-Identifier: AGPL-3.0-or-later
//! The commands, through a session, as the desktop shell and the e2e server call them.

#![allow(
    clippy::unwrap_used,
    reason = "test helpers outside #[test] functions may panic"
)]

use std::sync::{Arc, Mutex};

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use dbme_api::{self as api, BlockChange, IpcError, IpcErrorKind, LONG_SORT_KEY, Session};
use dbme_core::model::FieldKind;
use dbme_core::{CoreError, Id, IdGenerator, ManualClock};
use dbme_store::{
    Committed, Identity, NewColumn, NewField, NewObject, NewRelation, Position, Sort, Source,
    StoreError, VaultOptions,
};
use serde_json::{Value, json};

struct Fixture {
    session: Session,
    events: Arc<Mutex<Vec<Committed>>>,
    clock: Arc<ManualClock>,
    _dir: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let events: Arc<Mutex<Vec<Committed>>> = Arc::default();
        let sink = Arc::clone(&events);
        let session = Session::new(move |c| sink.lock().unwrap().push(c.clone()));
        let clock = Arc::new(ManualClock::new(1_761_380_471_000));
        let identity =
            Identity::load_or_create(&dir.path().join("config"), clock.as_ref()).unwrap();
        let options = VaultOptions {
            identity,
            clock: clock.clone(),
        };
        let path = dir.path().join("Test.dbme").display().to_string();
        api::vault_create(&session, &path, options).unwrap();
        Self {
            session,
            events,
            clock,
            _dir: dir,
        }
    }

    fn tick(&self) {
        self.clock.advance(1);
    }

    fn take_events(&self) -> Vec<Committed> {
        std::mem::take(&mut *self.events.lock().unwrap())
    }

    fn object(&self, title: &str) -> Id {
        self.tick();
        api::object_create(
            &self.session,
            NewObject {
                title: title.into(),
                ..Default::default()
            },
        )
        .unwrap()
    }

    fn apply(&self, object: Id, changes: Vec<BlockChange>) -> Result<(), IpcError> {
        self.tick();
        api::blocks_apply(&self.session, object, changes)
    }

    /// `(kind, text, depth)` of each block, in tree order.
    fn outline(&self, object: Id) -> Vec<(String, String, usize)> {
        let blocks = api::blocks_list(&self.session, object).unwrap();
        blocks
            .iter()
            .map(|b| {
                let mut depth = 0;
                let mut parent = b.parent_block_id;
                while let Some(p) = parent {
                    depth += 1;
                    parent = blocks.iter().find(|x| x.id == p).unwrap().parent_block_id;
                }
                let text = b.content_json["inline"][0]["text"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned();
                (b.kind.clone(), text, depth)
            })
            .collect()
    }
}

fn ids(n: usize) -> Vec<Id> {
    let generator = IdGenerator::new();
    (0..n)
        .map(|_| generator.next(1_761_380_471_000).unwrap())
        .collect()
}

fn text(text: &str) -> Value {
    json!({ "inline": [{ "text": text }] })
}

fn create(id: Id, parent: Option<Id>, after: Option<Id>, kind: &str, t: &str) -> BlockChange {
    BlockChange::Create {
        id,
        parent,
        after,
        kind: kind.into(),
        content: text(t),
    }
}

fn line(kind: &str, text: &str, depth: usize) -> (String, String, usize) {
    (kind.into(), text.into(), depth)
}

#[test]
fn without_a_vault_every_vault_command_says_so() {
    let session = Session::new(|_| {});
    assert_eq!(api::vault_info(&session).unwrap(), None);
    let error = api::types_list(&session).unwrap_err();
    assert_eq!(error.kind, IpcErrorKind::NoVault);
    assert_eq!(api::undo(&session).unwrap_err().kind, IpcErrorKind::NoVault);
    assert!(format!("{session:?}").contains("None"));
}

#[test]
fn store_errors_map_to_kinds() {
    let cases = [
        (StoreError::Rule("no".into()), IpcErrorKind::Rule),
        (
            StoreError::UndoBlocked("no".into()),
            IpcErrorKind::UndoBlocked,
        ),
        (StoreError::Locked("a.dbme".into()), IpcErrorKind::Locked),
        (StoreError::BadVaultName("a".into()), IpcErrorKind::BadPath),
        (
            StoreError::TooNew {
                found: 9,
                supported: 1,
            },
            IpcErrorKind::TooNew,
        ),
        (
            StoreError::Core(CoreError::invalid("x", "y")),
            IpcErrorKind::Invalid,
        ),
        (
            StoreError::NotFound {
                entity: "object",
                id: "x".into(),
            },
            IpcErrorKind::NotFound,
        ),
        (
            StoreError::MissingBlob(dbme_core::ContentHash::of(b"x")),
            IpcErrorKind::Storage,
        ),
    ];
    for (error, kind) in cases {
        let message = error.to_string();
        assert_eq!(IpcError::from(error), IpcError { kind, message });
    }
}

#[test]
fn every_change_is_announced_and_reads_are_not() {
    let f = Fixture::new();
    let x = f.object("x");
    let created = f.take_events();
    assert_eq!(created.len(), 1);
    assert!(created[0].changed.contains(&x));

    api::object_get(&f.session, x).unwrap();
    api::objects_page(
        &f.session,
        Source::Children { parent: None },
        &Sort::default(),
        0,
        10,
    )
    .unwrap();
    assert!(f.take_events().is_empty());

    f.tick();
    assert!(!api::object_rename(&f.session, x, "x".into()).unwrap());
    assert!(f.take_events().is_empty(), "no change, no event");
    let undone = api::undo(&f.session).unwrap().unwrap();
    assert_eq!(f.take_events(), std::slice::from_ref(&undone));
    assert_eq!(undone.undo_of, Some(created[0].group_id));
    assert!(api::vault_info(&f.session).unwrap().unwrap().can_redo);
}

#[test]
fn an_event_moves_in_one_undo_and_cannot_end_before_it_starts() {
    let f = Fixture::new();
    let x = f.object("x");
    let when = |f: &Fixture| {
        let row = api::object_get(&f.session, x).unwrap();
        (row.event_start, row.event_end)
    };
    f.tick();
    assert!(api::object_set_event(&f.session, x, Some(1_000), Some(2_000)).unwrap());
    assert_eq!(when(&f), (Some(1_000), Some(2_000)));
    assert!(
        !api::object_set_event(&f.session, x, Some(1_000), Some(2_000)).unwrap(),
        "no change"
    );

    f.tick();
    assert!(api::object_set_event(&f.session, x, Some(5_000), Some(2_000)).is_err());
    assert_eq!(
        when(&f),
        (Some(1_000), Some(2_000)),
        "a refusal changes nothing"
    );

    f.tick();
    assert!(api::object_set_event(&f.session, x, Some(3_000), Some(4_000)).unwrap());
    api::undo(&f.session).unwrap().unwrap();
    assert_eq!(
        when(&f),
        (Some(1_000), Some(2_000)),
        "one undo puts both back"
    );

    f.tick();
    assert!(api::object_set_event(&f.session, x, None, None).unwrap());
    assert_eq!(when(&f), (None, None));
}

#[test]
fn an_editor_batch_builds_a_nested_list_in_one_step() {
    let f = Fixture::new();
    let page = f.object("page");
    f.take_events();
    let [title, a, a1, a2, b] = ids(5).try_into().unwrap();
    f.apply(
        page,
        vec![
            BlockChange::Create {
                id: title,
                parent: None,
                after: None,
                kind: "heading".into(),
                content: json!({"level": 1, "inline": [{"text": "Groceries"}]}),
            },
            create(a, None, Some(title), "bullet", "fruit"),
            BlockChange::Create {
                id: a1,
                parent: Some(a),
                after: None,
                kind: "todo".into(),
                content: text("apples, but no checked"),
            },
        ],
    )
    .unwrap_err();
    assert!(
        api::blocks_list(&f.session, page).unwrap().is_empty(),
        "a refused block undoes the whole batch"
    );
    assert!(f.take_events().is_empty());

    f.apply(
        page,
        vec![
            BlockChange::Create {
                id: title,
                parent: None,
                after: None,
                kind: "heading".into(),
                content: json!({"level": 1, "inline": [{"text": "Groceries"}]}),
            },
            create(a, None, Some(title), "bullet", "fruit"),
            create(a1, Some(a), None, "bullet", "apples"),
            create(a2, Some(a), Some(a1), "bullet", "pears"),
            create(b, None, Some(a), "bullet", "bread"),
        ],
    )
    .unwrap();
    assert_eq!(
        f.outline(page),
        [
            line("heading", "Groceries", 0),
            line("bullet", "fruit", 0),
            line("bullet", "apples", 1),
            line("bullet", "pears", 1),
            line("bullet", "bread", 0),
        ]
    );
    let events = f.take_events();
    assert_eq!(events.len(), 1, "one batch, one step");

    // Outdent "pears", turn "fruit" into a paragraph once its last child has left, delete
    // "apples" with it gone first.
    f.apply(
        page,
        vec![
            BlockChange::Move {
                id: a2,
                parent: None,
                after: Some(a),
            },
            BlockChange::Delete { id: a1 },
            BlockChange::Update {
                id: a,
                kind: "paragraph".into(),
                content: text("fruit:"),
            },
        ],
    )
    .unwrap();
    assert_eq!(
        f.outline(page),
        [
            line("heading", "Groceries", 0),
            line("paragraph", "fruit:", 0),
            line("bullet", "pears", 0),
            line("bullet", "bread", 0),
        ]
    );

    f.tick();
    api::undo(&f.session).unwrap();
    assert_eq!(f.outline(page).len(), 5);
    assert_eq!(f.outline(page)[2], line("bullet", "apples", 1));
}

#[test]
fn a_batch_touches_only_its_own_objects_blocks() {
    let f = Fixture::new();
    let (page, other) = (f.object("page"), f.object("other"));
    let [mine] = ids(1).try_into().unwrap();
    f.apply(other, vec![create(mine, None, None, "paragraph", "theirs")])
        .unwrap();
    for change in [
        BlockChange::Update {
            id: mine,
            kind: "paragraph".into(),
            content: text("hijacked"),
        },
        BlockChange::Move {
            id: mine,
            parent: None,
            after: None,
        },
        BlockChange::Delete { id: mine },
    ] {
        let error = f.apply(page, vec![change]).unwrap_err();
        assert_eq!(error.kind, IpcErrorKind::Rule);
    }
    assert_eq!(f.outline(other), [line("paragraph", "theirs", 0)]);
}

#[test]
fn long_sort_keys_are_rebalanced_in_the_same_step() {
    let f = Fixture::new();
    let page = f.object("page");
    let blocks = ids(120);
    let first = blocks[0];
    f.apply(page, vec![create(first, None, None, "paragraph", "first")])
        .unwrap();
    // Each new block goes straight after the first, the worst case for key length.
    let mut longest = 0;
    for &id in &blocks[1..] {
        f.apply(page, vec![create(id, None, Some(first), "paragraph", "x")])
            .unwrap();
        let keys = api::blocks_list(&f.session, page).unwrap();
        longest = keys
            .iter()
            .map(|b| b.sort_key.as_str().len())
            .max()
            .unwrap();
        assert!(longest <= LONG_SORT_KEY, "a key of {longest} characters");
    }
    assert!(longest > 0);
    let order: Vec<Id> = api::blocks_list(&f.session, page)
        .unwrap()
        .iter()
        .map(|b| b.id)
        .collect();
    let mut expected = vec![first];
    expected.extend(blocks[1..].iter().rev());
    assert_eq!(order, expected, "rebalancing keeps the order");

    // Objects too.
    let parent = f.object("parent");
    let anchor = {
        f.tick();
        api::object_create(
            &f.session,
            NewObject {
                title: "anchor".into(),
                parent_id: Some(parent),
                ..Default::default()
            },
        )
        .unwrap()
    };
    for n in 0..100 {
        f.tick();
        api::object_create(
            &f.session,
            NewObject {
                title: format!("{n}"),
                parent_id: Some(parent),
                position: Position::After(anchor),
                ..Default::default()
            },
        )
        .unwrap();
    }
    let children = api::object_children(&f.session, Some(parent)).unwrap();
    assert!(
        children
            .iter()
            .all(|o| o.sort_key.as_str().len() <= LONG_SORT_KEY)
    );
    assert_eq!(children[0].title, "anchor");
    assert_eq!(children[1].title, "99");
    f.tick();
    assert!(api::object_move(&f.session, anchor, None, Position::Last).unwrap());
}

#[test]
fn field_values_are_set_checked_and_cleared() {
    let f = Fixture::new();
    f.tick();
    let task = api::types_list(&f.session)
        .unwrap()
        .into_iter()
        .find(|t| t.name == "Task")
        .unwrap()
        .id;
    api::field_create(
        &f.session,
        NewField {
            type_id: task,
            key: "due".into(),
            label: "Due".into(),
            kind: FieldKind::Date,
            options: json!({}),
            required: false,
        },
    )
    .unwrap();
    let x = f.object("x");
    f.tick();
    assert!(api::object_add_type(&f.session, x, task).unwrap());
    assert!(!api::object_add_type(&f.session, x, task).unwrap());
    assert_eq!(
        api::fields_list(&f.session, Source::Type { type_id: task })
            .unwrap()
            .len(),
        1
    );

    f.tick();
    assert!(api::object_set_prop(&f.session, x, "due".into(), Some(json!("2026-10-25"))).unwrap());
    let bad = api::object_set_prop(&f.session, x, "due".into(), Some(json!("tomorrow")));
    assert_eq!(bad.unwrap_err().kind, IpcErrorKind::Invalid);
    let null = api::object_set_prop(&f.session, x, "due".into(), Some(Value::Null));
    assert_eq!(null.unwrap_err().kind, IpcErrorKind::Invalid);
    assert_eq!(
        api::object_get(&f.session, x).unwrap().props_json["due"],
        "2026-10-25"
    );
    f.tick();
    assert!(api::object_set_prop(&f.session, x, "due".into(), None).unwrap());
    assert!(
        api::object_get(&f.session, x)
            .unwrap()
            .props_json
            .is_empty()
    );
    f.tick();
    assert!(api::object_remove_type(&f.session, x, task).unwrap());
}

/// Tables and their columns (ADR-0030).
mod columns {
    use super::*;

    fn type_named(f: &Fixture, name: &str) -> Option<Id> {
        api::types_list(&f.session)
            .unwrap()
            .into_iter()
            .find(|t| t.name == name)
            .map(|t| t.id)
    }

    fn child(f: &Fixture, parent: Id, title: &str, type_ids: Vec<Id>) -> Id {
        f.tick();
        api::object_create(
            &f.session,
            NewObject {
                title: title.into(),
                parent_id: Some(parent),
                type_ids,
                ..Default::default()
            },
        )
        .unwrap()
    }

    fn column(label: &str, kind: FieldKind) -> NewColumn {
        NewColumn {
            label: label.into(),
            kind,
            choices: Vec::new(),
        }
    }

    fn add(f: &Fixture, source: Source, label: &str, kind: FieldKind) -> Result<Id, IpcError> {
        f.tick();
        api::column_add(&f.session, source, column(label, kind))
    }

    fn children_of(parent: Id) -> Source {
        Source::Children {
            parent: Some(parent),
        }
    }

    fn labels(f: &Fixture, source: Source) -> Vec<String> {
        api::fields_list(&f.session, source)
            .unwrap()
            .into_iter()
            .map(|field| field.label)
            .collect()
    }

    fn types_of(f: &Fixture, object: Id) -> Vec<Id> {
        api::object_summaries(&f.session, &[object]).unwrap()[0]
            .type_ids
            .clone()
    }

    #[test]
    fn a_column_of_a_type_goes_on_that_type() {
        let f = Fixture::new();
        let task = type_named(&f, "Task").unwrap();
        let source = Source::Type { type_id: task };
        let target = api::column_target(&f.session, source).unwrap();
        assert_eq!(target.type_id, Some(task));
        assert_eq!(target.type_name, "Task");

        add(&f, source, "Serial number", FieldKind::Number).unwrap();
        let fields = api::fields_list(&f.session, source).unwrap();
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].key, "serial_number");
        assert_eq!(fields[0].type_id, task);
    }

    #[test]
    fn a_column_of_untyped_children_makes_a_type_named_after_the_page() {
        let f = Fixture::new();
        let parent = f.object("Infrastructure");
        let a = child(&f, parent, "a", vec![]);
        let b = child(&f, parent, "b", vec![]);
        let source = children_of(parent);
        let types_before = api::types_list(&f.session).unwrap().len();

        let target = api::column_target(&f.session, source).unwrap();
        assert_eq!(target.type_id, None);
        assert_eq!(target.type_name, "Infrastructure");

        add(&f, source, "Serial", FieldKind::Text).unwrap();
        let made = type_named(&f, "Infrastructure").unwrap();
        assert_eq!(types_of(&f, a), [made]);
        assert_eq!(types_of(&f, b), [made]);
        assert_eq!(labels(&f, source), ["Serial"]);
        assert_eq!(
            api::column_target(&f.session, source).unwrap().type_id,
            Some(made)
        );

        // The next column goes on the same type, and values are checked against the fields.
        add(&f, source, "RAM", FieldKind::Number).unwrap();
        assert_eq!(api::types_list(&f.session).unwrap().len(), types_before + 1);
        assert_eq!(labels(&f, source), ["Serial", "RAM"]);
        f.tick();
        assert!(api::object_set_prop(&f.session, a, "ram".into(), Some(json!(32))).unwrap());
        f.tick();
        let bad = api::object_set_prop(&f.session, a, "ram".into(), Some(json!("lots")));
        assert_eq!(bad.unwrap_err().kind, IpcErrorKind::Invalid);

        // Adding a column is one undo step, whatever it made.
        f.tick();
        api::undo(&f.session).unwrap();
        f.tick();
        api::undo(&f.session).unwrap();
        assert_eq!(labels(&f, source), ["Serial"]);
        f.tick();
        api::undo(&f.session).unwrap();
        assert_eq!(labels(&f, source), Vec::<String>::new());
        assert_eq!(type_named(&f, "Infrastructure"), None);
        assert!(types_of(&f, a).is_empty());
        assert!(types_of(&f, b).is_empty());
    }

    #[test]
    fn a_column_of_children_sharing_a_type_goes_on_it() {
        let f = Fixture::new();
        let first = f.object("Devices");
        child(&f, first, "a", vec![]);
        add(&f, children_of(first), "Serial", FieldKind::Text).unwrap();
        let devices = type_named(&f, "Devices").unwrap();

        let second = f.object("Phones");
        let c = child(&f, second, "c", vec![devices]);
        let d = child(&f, second, "d", vec![devices]);
        let source = children_of(second);
        assert_eq!(
            api::column_target(&f.session, source).unwrap().type_id,
            Some(devices)
        );
        add(&f, source, "Carrier", FieldKind::Text).unwrap();
        assert_eq!(type_named(&f, "Phones"), None);
        assert_eq!(types_of(&f, c), [devices]);
        assert_eq!(types_of(&f, d), [devices]);
        assert_eq!(labels(&f, children_of(first)), ["Serial", "Carrier"]);
    }

    #[test]
    fn a_column_leaves_a_built_in_type_alone_unless_it_is_the_tables_type() {
        let f = Fixture::new();
        let note = type_named(&f, "Note").unwrap();
        let parent = f.object("Reading");
        let a = child(&f, parent, "a", vec![note]);
        add(&f, children_of(parent), "Rating", FieldKind::Number).unwrap();
        let made = type_named(&f, "Reading").unwrap();
        assert_eq!(
            api::fields_list(&f.session, Source::Type { type_id: note })
                .unwrap()
                .len(),
            0
        );
        assert!(types_of(&f, a).contains(&note));
        assert!(types_of(&f, a).contains(&made));

        // A page named like a built-in type does not take that type's name.
        let journal = f.object("Journal");
        child(&f, journal, "day", vec![]);
        let target = api::column_target(&f.session, children_of(journal)).unwrap();
        assert_eq!(target.type_id, None);
        assert_eq!(target.type_name, "Journal pages");
        add(&f, children_of(journal), "Mood", FieldKind::Text).unwrap();
        let built_in = type_named(&f, "Journal").unwrap();
        let fields = api::fields_list(&f.session, Source::Type { type_id: built_in });
        assert!(fields.unwrap().is_empty());
        assert!(type_named(&f, "Journal pages").is_some());
    }

    #[test]
    fn untyped_children_reuse_a_type_with_the_pages_name_and_a_blank_page_is_untitled() {
        let f = Fixture::new();
        let parent = f.object("");
        let a = child(&f, parent, "a", vec![]);
        add(&f, children_of(parent), "One", FieldKind::Text).unwrap();
        let untitled = type_named(&f, "Untitled").unwrap();

        // A child with no type means no type is shared, so the page's own name is used again.
        let b = child(&f, parent, "b", vec![]);
        add(&f, children_of(parent), "Two", FieldKind::Text).unwrap();
        assert_eq!(types_of(&f, a), [untitled]);
        assert_eq!(types_of(&f, b), [untitled]);
    }

    #[test]
    fn a_column_of_an_empty_table_makes_its_first_row() {
        let f = Fixture::new();
        let parent = f.object("Empty");
        let source = children_of(parent);
        assert_eq!(api::objects_count(&f.session, source).unwrap(), 0);

        add(&f, source, "Serial", FieldKind::Text).unwrap();
        assert_eq!(api::objects_count(&f.session, source).unwrap(), 1);
        assert_eq!(labels(&f, source), ["Serial"]);

        // The next row is of the same type, so the column keeps showing.
        f.tick();
        let row = api::row_add(&f.session, source, parent).unwrap();
        assert_eq!(types_of(&f, row), [type_named(&f, "Empty").unwrap()]);
        assert_eq!(
            api::object_get(&f.session, row).unwrap().parent_id,
            Some(parent)
        );
    }

    #[test]
    fn a_new_row_takes_the_types_a_row_there_has() {
        let f = Fixture::new();
        let note = type_named(&f, "Note").unwrap();
        let task = type_named(&f, "Task").unwrap();

        let parent = f.object("Parent");
        f.tick();
        let of_type = api::row_add(&f.session, Source::Type { type_id: task }, parent).unwrap();
        assert_eq!(types_of(&f, of_type), [task]);
        assert_eq!(
            api::object_get(&f.session, of_type).unwrap().parent_id,
            Some(parent)
        );

        let mixed = f.object("Mixed");
        child(&f, mixed, "a", vec![note, task]);
        child(&f, mixed, "b", vec![note]);
        f.tick();
        let row = api::row_add(&f.session, children_of(mixed), mixed).unwrap();
        assert_eq!(types_of(&f, row), [note]);

        let plain = f.object("Plain");
        child(&f, plain, "a", vec![note]);
        child(&f, plain, "b", vec![]);
        f.tick();
        let row = api::row_add(&f.session, children_of(plain), plain).unwrap();
        assert!(types_of(&f, row).is_empty());

        let empty = f.object("Empty");
        f.tick();
        let row = api::row_add(&f.session, children_of(empty), empty).unwrap();
        assert!(types_of(&f, row).is_empty());
    }

    #[test]
    fn a_trashed_child_does_not_stop_a_type_being_shared() {
        let f = Fixture::new();
        let note = type_named(&f, "Note").unwrap();
        let parent = f.object("Parent");
        child(&f, parent, "a", vec![note]);
        let untyped = child(&f, parent, "b", vec![]);
        f.tick();
        api::object_trash(&f.session, untyped).unwrap();
        f.tick();
        let row = api::row_add(&f.session, children_of(parent), parent).unwrap();
        assert_eq!(types_of(&f, row), [note]);
    }

    #[test]
    fn a_column_is_refused_and_nothing_is_kept() {
        let f = Fixture::new();
        let parent = f.object("Parent");
        let a = child(&f, parent, "a", vec![]);
        let source = children_of(parent);
        let types_before = api::types_list(&f.session).unwrap().len();

        for (label, kind) in [
            ("", FieldKind::Text),
            ("   ", FieldKind::Text),
            ("Title", FieldKind::Text),
            (" title ", FieldKind::Text),
            ("Where", FieldKind::Geo),
            ("Status", FieldKind::Select),
        ] {
            assert!(add(&f, source, label, kind).is_err(), "{label:?}");
            assert_eq!(api::types_list(&f.session).unwrap().len(), types_before);
            assert!(types_of(&f, a).is_empty());
        }
        assert!(add(&f, Source::Children { parent: None }, "X", FieldKind::Text).is_err());

        add(&f, source, "Serial", FieldKind::Text).unwrap();
        assert!(add(&f, source, "serial", FieldKind::Number).is_err());
        assert_eq!(labels(&f, source), ["Serial"]);
    }

    #[test]
    fn a_column_gets_a_key_and_a_select_gets_its_choices() {
        let f = Fixture::new();
        let parent = f.object("Parent");
        let a = child(&f, parent, "a", vec![]);
        let source = children_of(parent);

        add(&f, source, "Due date", FieldKind::Date).unwrap();
        add(&f, source, "Due-date!", FieldKind::Date).unwrap();
        add(&f, source, "名前", FieldKind::Text).unwrap();
        f.tick();
        api::column_add(
            &f.session,
            source,
            NewColumn {
                label: "Status".into(),
                kind: FieldKind::Select,
                choices: vec![
                    "To do".into(),
                    " Doing ".into(),
                    "".into(),
                    "To do".into(),
                    "To-do".into(),
                    "完了".into(),
                ],
            },
        )
        .unwrap();

        let fields = api::fields_list(&f.session, source).unwrap();
        let keys: Vec<&str> = fields.iter().map(|field| field.key.as_str()).collect();
        assert_eq!(keys, ["due_date", "due_date_2", "column_3", "status"]);
        assert_eq!(fields[2].label, "名前");
        assert_eq!(
            fields[3].options_json,
            json!({ "options": [
                { "id": "to_do", "label": "To do" },
                { "id": "doing", "label": "Doing" },
                { "id": "to_do_2", "label": "To-do" },
                { "id": "choice_4", "label": "完了" },
            ] })
        );
        f.tick();
        assert!(
            api::object_set_prop(&f.session, a, "status".into(), Some(json!("doing"))).unwrap()
        );
        f.tick();
        let bad = api::object_set_prop(&f.session, a, "status".into(), Some(json!("nope")));
        assert_eq!(bad.unwrap_err().kind, IpcErrorKind::Invalid);
    }

    #[test]
    fn a_field_is_renamed_and_deleted_and_its_values_stay() {
        let f = Fixture::new();
        let parent = f.object("Parent");
        let a = child(&f, parent, "a", vec![]);
        let source = children_of(parent);
        let serial = add(&f, source, "Serial", FieldKind::Text).unwrap();
        add(&f, source, "Owner", FieldKind::Text).unwrap();
        f.tick();
        api::object_set_prop(&f.session, a, "serial".into(), Some(json!("X1"))).unwrap();

        f.tick();
        assert!(api::field_rename(&f.session, serial, " Serial no. ").unwrap());
        assert_eq!(labels(&f, source), ["Serial no.", "Owner"]);
        let renamed = api::fields_list(&f.session, source).unwrap();
        assert_eq!(renamed[0].key, "serial");
        assert_eq!(
            api::object_get(&f.session, a).unwrap().props_json["serial"],
            "X1"
        );

        f.tick();
        assert!(!api::field_rename(&f.session, serial, "Serial no.").unwrap());
        f.tick();
        assert!(api::field_rename(&f.session, serial, "SERIAL NO.").unwrap());
        for refused in ["owner", "Title", "  "] {
            f.tick();
            let renamed = api::field_rename(&f.session, serial, refused);
            assert!(renamed.is_err(), "{refused:?}");
        }
        assert_eq!(labels(&f, source), ["SERIAL NO.", "Owner"]);

        f.tick();
        api::field_delete(&f.session, serial).unwrap();
        assert_eq!(labels(&f, source), ["Owner"]);
        assert_eq!(
            api::object_get(&f.session, a).unwrap().props_json["serial"],
            "X1"
        );

        f.tick();
        api::undo(&f.session).unwrap();
        assert_eq!(labels(&f, source), ["SERIAL NO.", "Owner"]);
    }
}

#[test]
fn attachments_round_trip_as_base64() {
    let f = Fixture::new();
    let x = f.object("x");
    f.take_events();
    let png = b"\x89PNG\r\n\x1a\n pixels";
    let data = BASE64.encode(png);
    f.tick();
    let hash = api::attachment_add(
        &f.session,
        x,
        &data,
        Some("dot.png".into()),
        Some("image/png".into()),
    )
    .unwrap();
    assert_eq!(f.take_events().len(), 1);
    assert_eq!(api::blob_read(&f.session, &hash).unwrap(), data);
    let summary = &api::object_summaries(&f.session, &[x]).unwrap()[0];
    assert_eq!(summary.cover.as_ref(), Some(&hash));

    f.tick();
    let again = api::attachment_add(&f.session, x, &data, None, Some("image/png".into()));
    assert_eq!(again.unwrap(), hash);
    assert!(
        f.take_events().is_empty(),
        "attaching the same bytes changes nothing"
    );
    assert_eq!(api::attachments_list(&f.session, x).unwrap().len(), 1);

    let bad = api::attachment_add(&f.session, x, "not base64!", None, None);
    assert_eq!(bad.unwrap_err().kind, IpcErrorKind::Invalid);
    let missing = api::blob_read(&f.session, &dbme_core::ContentHash::of(b"nothing"));
    assert_eq!(missing.unwrap_err().kind, IpcErrorKind::Storage);
}

#[test]
fn search_backlinks_and_ancestors_are_served() {
    let f = Fixture::new();
    let (a, b) = (f.object("Alpha"), f.object("Beta"));
    let [block] = ids(1).try_into().unwrap();
    f.apply(
        b,
        vec![BlockChange::Create {
            id: block,
            parent: None,
            after: None,
            kind: "paragraph".into(),
            content: json!({"inline": [{"mention": a.to_string()}]}),
        }],
    )
    .unwrap();
    let links = api::backlinks(&f.session, a).unwrap();
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].source.id, b);
    let found = api::object_search(&f.session, "alp", 10).unwrap();
    assert_eq!(found[0].object.id, a);
    assert_eq!(
        api::objects_count(&f.session, Source::Children { parent: None }).unwrap(),
        2
    );
    f.tick();
    api::object_move(&f.session, b, Some(a), Position::Last).unwrap();
    assert_eq!(api::object_ancestors(&f.session, b).unwrap()[0].id, a);
    f.tick();
    api::object_trash(&f.session, a).unwrap();
    assert!(
        api::object_search(&f.session, "alp", 10)
            .unwrap()
            .is_empty()
    );
    f.tick();
    api::object_restore(&f.session, a).unwrap();
    api::vault_close(&f.session).unwrap();
    assert_eq!(api::vault_info(&f.session).unwrap(), None);
}

#[test]
fn relations_drawn_on_the_map_are_made_once_trashed_and_undone() {
    let f = Fixture::new();
    let root = f.object("root");
    let child = |title: &str| {
        f.tick();
        api::object_create(
            &f.session,
            NewObject {
                title: title.into(),
                parent_id: Some(root),
                ..Default::default()
            },
        )
        .unwrap()
    };
    let (a, b) = (child("a"), child("b"));
    f.take_events();
    let drawn = || {
        api::object_tree(&f.session, root, 10)
            .unwrap()
            .relations
            .into_iter()
            .map(|r| (r.id, r.src_id, r.dst_id, r.rel_type))
            .collect::<Vec<_>>()
    };

    f.tick();
    let relation = api::relation_create(&f.session, a, b).unwrap();
    assert_eq!(f.take_events().len(), 1);
    assert_eq!(drawn(), [(relation, a, b, api::RELATED.to_owned())]);

    f.tick();
    assert_eq!(api::relation_create(&f.session, a, b).unwrap(), relation);
    assert!(
        f.take_events().is_empty(),
        "drawing it again changes nothing"
    );

    let myself = api::relation_create(&f.session, a, a).unwrap_err();
    assert_eq!(myself.kind, IpcErrorKind::Rule);

    f.tick();
    assert!(api::relation_trash(&f.session, relation).unwrap());
    assert_eq!(f.take_events().len(), 1);
    assert!(drawn().is_empty());
    f.tick();
    assert!(!api::relation_trash(&f.session, relation).unwrap());
    assert!(f.take_events().is_empty(), "already there, no event");

    f.tick();
    api::undo(&f.session).unwrap().unwrap();
    assert_eq!(drawn().len(), 1, "undo brings it back");
    f.tick();
    api::redo(&f.session).unwrap().unwrap();
    assert!(drawn().is_empty(), "redo sends it away again");
}

#[test]
fn relations_a_block_or_a_field_holds_cannot_be_trashed_from_the_map() {
    let f = Fixture::new();
    let (a, b) = (f.object("a"), f.object("b"));
    let block = ids(1)[0];
    f.apply(
        a,
        vec![BlockChange::Create {
            id: block,
            parent: None,
            after: None,
            kind: "paragraph".into(),
            content: json!({"inline": [{"mention": b.to_string()}]}),
        }],
    )
    .unwrap();
    let mention = api::backlinks(&f.session, b).unwrap()[0].relation.id;
    let refused = api::relation_trash(&f.session, mention).unwrap_err();
    assert_eq!(refused.kind, IpcErrorKind::Rule);

    f.tick();
    let task = api::types_list(&f.session)
        .unwrap()
        .into_iter()
        .find(|t| t.name == "Task")
        .unwrap()
        .id;
    let field = api::field_create(
        &f.session,
        NewField {
            type_id: task,
            key: "linked".into(),
            label: "Linked".into(),
            kind: FieldKind::Relation,
            options: json!({}),
            required: false,
        },
    )
    .unwrap();
    f.tick();
    let held = f
        .session
        .with(|vault| {
            vault
                .transact(|txn| {
                    txn.create_relation(NewRelation {
                        src_id: a,
                        dst_id: b,
                        rel_type: "linked".into(),
                        field_id: Some(field),
                        src_block_id: None,
                        label: None,
                        props: Default::default(),
                    })
                })
                .map(|(id, _)| id)
        })
        .unwrap();
    let refused = api::relation_trash(&f.session, held).unwrap_err();
    assert_eq!(refused.kind, IpcErrorKind::Rule);
    f.tick();
    let free = api::relation_create(&f.session, a, b).unwrap();
    assert_ne!(
        free, held,
        "what a field holds is not a relation drawn on the map"
    );
}
