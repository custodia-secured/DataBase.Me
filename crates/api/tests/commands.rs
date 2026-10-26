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
    Committed, Identity, NewField, NewObject, Position, Sort, Source, StoreError, VaultOptions,
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
