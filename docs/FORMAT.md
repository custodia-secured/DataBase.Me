<!-- SPDX-License-Identifier: Apache-2.0 -->

# DataBase.Me vault format

**Status: draft 4, 2026-11-02. Format version 0.** Nothing here is stable until FORMAT.md v0.1
ships with the MVP (M7). Each section says whether it is **Decided** (fixed by an accepted ADR),
**Draft** (proposed, to be confirmed in the milestone named) or **Open** (not designed yet).

This specification is licensed under the Apache License 2.0 (see `packages/sdk/LICENSE` in the
DataBase.Me repository), unlike the application, so that anyone can write a reader, a converter or
a competing app for DataBase.Me vaults. Its goal is that a programmer with this document, SQLite
and a Markdown parser can recover every object, field, relation, block and file in a vault without
the app.

The key words MUST, MUST NOT, SHOULD and MAY are to be read as described in RFC 2119 and RFC 8174.

## 1. Vault layout (Decided, ADR-0002)

A vault is a folder whose name ends in `.dbme`:

```
MyVault.dbme/
  vault.sqlite          canonical store (optionally encrypted)
  vault.sqlite-wal      SQLite write-ahead log, present while the app runs; part of the database
  vault.sqlite-shm      SQLite's shared-memory index of the WAL; never needed to read the vault
  vault.lock            held locked by the app that has the vault open (see below)
  blobs/ab/cd/<hash>    content-addressed files (optionally encrypted)
  files/                two-way Markdown replica (optional; location configurable)
  export/               JSON and CSV exports (export-only)
  plugins/              installed plugins (M10)
  themes/               user themes (M7)
  logs/ai-audit.jsonl   record of every AI call (M11)
  FORMAT.md             a copy of this specification, written by the app
```

`vault.sqlite` and `blobs/` together are the vault. `files/` and `export/` are derived from them
and can be regenerated. A reader that copies a vault while the app is running MUST copy
`vault.sqlite` together with its `-wal` file, or use SQLite's backup API.

- **One writer.** An app that opens a vault takes an exclusive operating-system lock on
  `vault.lock` (creating the file if needed) and holds it until it closes the vault. Another app
  that cannot take the lock MUST NOT write to the vault. The file's content means nothing; the
  lock disappears with the process, so a crash never leaves a vault locked.
- **Creation is atomic.** The app builds a new vault in a hidden sibling folder named
  `.<name>.dbme.creating-<ULID>` and renames it into place when it is complete. A folder with that
  name pattern is an abandoned creation and MAY be deleted.
- **Temporary blob files.** A blob is written to `blobs/.incoming-<random>` and renamed into place
  when complete (§4). Such files are never part of the vault and MAY be deleted when no app has the
  vault open.

## 2. Encodings (Decided, ADR-0003, ADR-0013)

- **Text** is UTF-8 without a byte-order mark. Files written by the app use LF line endings.
- **IDs** are ULIDs: 26 characters of Crockford base32, upper case. Readers SHOULD accept lower
  case.
- **Timestamps** are integer milliseconds since the Unix epoch (UTC) inside the database, and
  ISO 8601 UTC strings with millisecond precision (`2026-10-25T08:21:11.000Z`) in files and exports.
- **Hashes** are `<algorithm>:<lowercase hex>`, for example `blake3:9f86...`. The only algorithm
  today is BLAKE3 with a 256-bit output. A reader MUST NOT guess an unknown algorithm.
- **Sort keys** order siblings: compare keys byte by byte, then IDs when keys are equal. See §2.1.
- **JSON columns** (`*_json`) hold UTF-8 JSON text.

### 2.1 Sort keys

A sort key is a string over the 62 ASCII digits `0-9A-Za-z`, in that order (so plain byte order
compares them). It is an **integer part** optionally followed by a **fraction**:

- The integer part's first character gives its length. `a` to `z` start integers of 2 to 27
  characters (`a` plus one digit, `b` plus two, …); `A` to `Z` start integers of 27 down to 2
  characters, which sort before them. The smallest integer, `A` and 26 `0`s, is never a key
  without a fraction.
- The fraction, if present, MUST NOT end in `0`.

The first key of an empty list is `a0`. Appending increments the integer part (`a0`, `a1`, …,
`az`, `b00`); prepending decrements it. A key between two neighbours takes a midpoint in the
fraction: between `a0` and `a1` comes `a0V`.

This is the algorithm of David Greenspan's "Implementing Fractional Indexing", as published in the
`fractional-indexing` library (CC0), and gives the same keys. Readers only ever compare keys, so a
reader does not need the algorithm, only the ordering rule above.

## 3. The database, `vault.sqlite` (Decided for migration 1, ADR-0015)

The file is an ordinary SQLite 3 database.

- `PRAGMA application_id` is `0x44424D45` (the ASCII bytes `DBME`). A database with any other
  non-zero application ID is not a vault.
- `PRAGMA user_version` is the number of the last schema migration applied (§9). An app MUST NOT
  write to a vault whose `user_version` is newer than it understands.
- The journal mode is WAL. An app that writes to a vault MUST turn on foreign key enforcement
  (`PRAGMA foreign_keys = ON`).
- Every table is `STRICT`. IDs and hashes are `TEXT`, timestamps are `INTEGER` milliseconds,
  booleans are `INTEGER` 0 or 1, `geo_lat` and `geo_lon` are `REAL`, and JSON columns are `TEXT`.
- Indexes are not part of the format. Readers MUST NOT depend on them; any migration may add or
  drop them.

### 3.1 Tables

Migration 1 (M1) creates the tables below. Every column is `NOT NULL` unless marked `?`; `→ t`
marks a foreign key to table `t`. Every table except `vault_meta` and `ops` holds exactly what you
get by applying the op log (§5) in `seq` order to an empty database.

| Table          | Purpose                                                    | Columns                                                                                                                                                                                                                                                                                                                            |
| -------------- | ---------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `vault_meta`   | Facts about the vault itself; never changed by ops         | `key` (primary key), `value`. Keys: `vault_id` (a ULID), `created_at`, `created_by` (product and version of the app that made it), `format_version`                                                                                                                                                                                |
| `types`        | Object types (user-editable)                               | `id`, `name`, `plural?`, `icon?`, `color?`, `extends?` → `types` (single inheritance), `description?`, `is_system`, `created_at`, `updated_at`                                                                                                                                                                                     |
| `fields`       | Field definitions of a type                                | `id`, `type_id` → `types`, `key` (unique within the type), `label`, `kind` (§3.3), `options_json`, `required`, `sort_order`                                                                                                                                                                                                        |
| `objects`      | Every object                                               | `id`, `title`, `summary?`, `parent_id?` → `objects` (its one home in the hierarchy), `sort_key`, `props_json` (field values by field key, §3.3), `created_at`, `updated_at`, `captured_at?`, `event_start?`, `event_end?`, `geo_lat?`, `geo_lon?`, `deleted_at?` (set while in the Trash), `sensitivity` (`normal` or `sensitive`) |
| `object_types` | Which types an object has (an object can have several)     | `object_id` → `objects`, `type_id` → `types`; the pair is the primary key                                                                                                                                                                                                                                                          |
| `blocks`       | The content tree inside an object                          | `id`, `object_id` → `objects`, `parent_block_id?` → `blocks` (in the same object), `sort_key`, `kind`, `content_json`, `created_at`, `updated_at`                                                                                                                                                                                  |
| `relations`    | Typed, directed links; backlinks are the reverse direction | `id`, `src_id` → `objects`, `dst_id` → `objects`, `rel_type`, `field_id?` → `fields`, `src_block_id?` → `blocks`, `label?`, `props_json`, `created_at`, `deleted_at?`                                                                                                                                                              |
| `blobs`        | Files in the blob store that attachments refer to          | `hash` (primary key), `size` (bytes), `mime?`, `encrypted`, `created_at`. A row is added when an attachment first refers to the blob, and is not removed by ops                                                                                                                                                                    |
| `attachments`  | Which object uses which blob, and how                      | `object_id` → `objects`, `blob_hash` → `blobs`, `role` (`original`, `thumbnail`, `transcript`, `ocr_text` or `snapshot`), `filename?`, `mime?`; `(object_id, blob_hash, role)` is the primary key                                                                                                                                  |
| `ops`          | The append-only operation log (§5)                         | `seq` (`INTEGER PRIMARY KEY`), `op_id` (unique), `group_id`, `undo_of?`, `actor_id`, `device_id`, `ts`, `kind`, `target_id`, `payload_json`, `prev_hash?`, `hash`, `signature?`                                                                                                                                                    |

Migration 2 (M4, ADR-0020) adds one more table the op log produces, and three it does not:

| Table          | Purpose                                                                           | Columns                                                                                                                                                                                     |
| -------------- | --------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `views`        | Saved views: a query, a renderer and its settings                                 | `id`, `name`, `renderer` (an identifier), `query_json` (a query in the DSL, ADR-0020), `config_json` (an object), `owner_object_id?` → `objects`, `created_at`, `updated_at`                |
| `search_dirty` | Derived: objects whose text changed since the index last looked                   | `object_id` (primary key). Emptied by the next search                                                                                                                                       |
| `search_text`  | Derived: the searchable text of every live object                                 | `doc` (`INTEGER PRIMARY KEY`), `object_id` (unique), `title`, `body` (the summary, then the blocks' plain text in tree order, one line each), `props_text` (string and number field values) |
| `fts`          | Derived: an FTS5 external-content index over `search_text`, `content_rowid` `doc` | `title`, `body`, `props_text`; tokenizer `unicode61 remove_diacritics 2`                                                                                                                    |

The derived tables are not part of the op log's promise: a reader MAY ignore them, and an app MAY
rebuild them from `objects` and `blocks` at any time.

Migration 3 (M5, ADR-0021; Proposed) adds the files folder's bookkeeping (§6): four tables the op
log does not produce and `verify` does not replay. They describe files on disk, so a rescan of the
folder can rebuild them; they carry no foreign keys, so a permanent delete replayed from the log
never fails on them.

| Table          | Purpose                                             | Columns                                                                                                                                                                                                                                                                                                                                                                            |
| -------------- | --------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `mirror_state` | One row per object the folder holds or held         | `object_id` (primary key), `rel_path` (unique; forward slashes, relative to the folder), `path_key` (unique; `rel_path` in NFC, case-folded), `last_hash?` (of the bytes last written or read), `base_text?` (the Markdown last synced: the merge base), `synced_seq?`, `synced_at?`, `file_size?`, `file_mtime_ms?`, `state` (`synced`, `pending`, `held`, `excluded`, `trashed`) |
| `mirror_dirty` | Objects whose file must be written again            | `object_id` (primary key). Every app-side transaction adds the objects whose files it changed; a type or field change sets `full_rewrite_needed` in `mirror_meta` instead                                                                                                                                                                                                          |
| `mirror_meta`  | Facts about the folder as a whole                   | `key` (primary key), `value`. Keys: `folder_enabled`, `full_rewrite_needed`, `last_rescan_at`                                                                                                                                                                                                                                                                                      |
| `conflicts`    | The Conflicts inbox: things the person must resolve | `id`, `object_id?`, `rel_path?`, `kind` (`edit_edit`, `edit_delete`, `parse_error`, `invalid_value`, `duplicate_id`, `dropped_file`, `bulk_change_hold`, `missing_id`, `cloud_conflict_file`), `details_json`, `conflict_copy_id?` (the Note holding the file's version), `resolution?`, `created_at`, `resolved_at?`                                                              |

Rules the schema does not express, which an app MUST keep:

- An object's parents never loop, and a block's parents never loop or leave its object. A type's
  `extends` chain never loops.
- Trashing an object sets `deleted_at` on it and on every live object below it; restoring clears
  them. An object is deleted for good only from the Trash, together with its blocks, its type
  assignments, its attachments and the relations that start or end at it.
- A system type (`is_system` = 1) is never deleted. A type is deleted only when no object has it,
  no type extends it and no relation uses one of its fields; its fields go with it.
- A relation drawn by hand has `rel_type` `related`, and no `field_id` or `src_block_id`. It never
  links an object to itself, and there is at most one live one in each direction.
- Every update of a row with an `updated_at` column sets it to the op's `ts`.

### 3.2 Built-in types

Every new vault contains these editable system types: Note, Page, Task, Person, Organization,
Place, Event, Project, Area, Resource, Media, WebClip, Document, Image, Audio, Video, Email,
Message, Contact, Dataset, Tag, Collection, Template, Highlight, Journal.

### 3.3 Field kinds and value encoding (Decided, M1)

Field values live in `objects.props_json` under the field's `key`. A field with no value has no
key: `null` is never stored. Keys that match no field of the object's types are kept as they are.
An app checks the shape of a value, not its meaning: a `url` value is a string, not necessarily a
reachable URL. The options of a `select` or `multi_select` field are listed in `options_json` as
`{"options": [{"id": "<unique string>", ...}, ...]}`; a `rating` is an integer from 0 to
`options_json.max` (1 to 100, default 5).

| Kind                            | JSON value                                                      |
| ------------------------------- | --------------------------------------------------------------- |
| `text`, `url`, `email`, `phone` | string                                                          |
| `rich_text`                     | array of block objects (same shape as `blocks.content_json`)    |
| `number`                        | number                                                          |
| `currency`                      | `{"amount": "<decimal string>", "currency": "<ISO 4217 code>"}` |
| `boolean`                       | `true` or `false`                                               |
| `date`                          | `"YYYY-MM-DD"`                                                  |
| `datetime`                      | ISO 8601 UTC string                                             |
| `duration`                      | integer milliseconds                                            |
| `select`                        | string (the option's ID from `options_json`)                    |
| `multi_select`                  | array of option IDs                                             |
| `relation`                      | not stored here: rows in `relations` with this `field_id`       |
| `geo`                           | `{"lat": number, "lon": number}`                                |
| `file`                          | array of blob hashes (also recorded in `attachments`)           |
| `rating`                        | integer                                                         |
| `formula`, `rollup`             | not stored; computed from `options_json`                        |
| `json`                          | any JSON value                                                  |

### 3.4 Tables of later milestones (Draft)

These tables arrive with the migrations of the milestones that use them. Their columns are the
handoff brief's draft.

| Table          | Purpose                                                               | Columns                                                                                                                                                                                                                                                  |
| -------------- | --------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `provenance`   | Where an object came from                                             | `id`, `object_id`, `source_kind` (`manual`, `import:<importer>`, `clip`, `feed`, `file:external`, `ai:<model>`, `plugin:<id>`), `source_uri`, `import_run_id`, `original_blob`, `extractor`, `extractor_version`, `confidence`, `accepted`, `created_at` |
| `canvas_items` | Positions on a canvas view                                            | `view_id`, `item_id`, `object_id`, `x`, `y`, `w`, `h`, `z`, `style_json`                                                                                                                                                                                 |
| `spaces`       | Top-level spaces and their AI policy                                  | `id`, `name`, `ai_policy` (`none`, `local_only`, `any`), `settings_json`                                                                                                                                                                                 |
| `ai_audit`     | Every AI call                                                         | `id`, `ts`, `provider`, `model`, `purpose`, `object_ids_json`, `bytes_sent`, `user_initiated`                                                                                                                                                            |
| `vec_chunks`   | sqlite-vec embeddings (derived; only with an embedding provider, M11) | `chunk_id`, `embedding`                                                                                                                                                                                                                                  |

`mirror_state` and `conflicts` arrived with migration 3 (§3.1).

### 3.5 Block content (Decided, ADR-0016)

A block's `kind` is a lower-case identifier of at most 64 characters, and its `content_json` is a
JSON object. These are the kinds of format version 0:

| Kind        | `content_json`                            | May have children |
| ----------- | ----------------------------------------- | ----------------- |
| `paragraph` | `inline`                                  | no                |
| `heading`   | `level` (integer, 1 to 6), `inline`       | no                |
| `bullet`    | `inline`                                  | yes               |
| `numbered`  | `inline`                                  | yes               |
| `todo`      | `checked` (`true` or `false`), `inline`   | yes               |
| `quote`     | `inline`                                  | no                |
| `code`      | `code` (string), `language?` (string)     | no                |
| `divider`   | nothing                                   | no                |
| `image`     | `blob` (a blob hash, §2), `alt?` (string) | no                |

- **Text.** `inline` is an array of runs, in order. A text run is `{"text": "<not empty>"}`, with
  any of the marks `bold`, `italic`, `underline`, `strike` and `code` set to `true`. A mark that is
  not set is left out. A text run may also carry `"link": "<URL>"`. A line break is a `\n` inside
  a text run.
- **Mentions.** A mention run is `{"mention": "<object ID>"}`. It is shown with the mentioned
  object's current title. For each object a block mentions, the vault keeps one `relations` row
  with `rel_type` `mentions`, `src_id` the block's object, `dst_id` the mentioned object and
  `src_block_id` the block. It is written in the same transaction as the block. A mention of an
  object that is missing or in the Trash has no row. Backlinks are these rows read from `dst_id`.
- **Children.** Only list items (`bullet`, `numbered`, `todo`) have child blocks: the blocks
  indented under them. Siblings sort by `sort_key`, then by `id` (§2.1).
- **Kinds and keys a reader does not know.** A reader MUST keep a block whose kind it does not
  know, with its content and its children, and MAY show it as a placeholder. Such a block may have
  children. A writer MUST keep `content_json` keys it does not use when it rewrites a block.

## 4. Blob store (Decided layout, ADR-0002; encryption Open, M9)

A blob with hash `blake3:<hex>` is stored at `blobs/<hex[0..2]>/<hex[2..4]>/<hex>`, for example
`blobs/9f/86/9f86d0...`. Blobs never change. When vault encryption is on, each blob is encrypted
with XChaCha20-Poly1305; the envelope format is specified in M9.

## 5. The op log (Decided, ADR-0012)

Every change to the tables of §3.1 is an **op**: one row of `ops`, written in the same SQLite
transaction as the change it records. `vault_meta` is the only table ops do not change. Applying
every op in `seq` order to an empty database reproduces the other tables exactly; an app's
verification does this (§5.5). Rows of `ops` are never updated or deleted.

### 5.1 Columns

| Column         | Meaning                                                                                       |
| -------------- | --------------------------------------------------------------------------------------------- |
| `seq`          | Position in this vault's log, from 1. Local: not hashed, and may differ between synced copies |
| `op_id`        | The op's ULID                                                                                 |
| `group_id`     | The undo step: every op committed in one transaction shares one ULID                          |
| `undo_of`      | For the ops of an undo or redo, the `group_id` of the step they reverse; otherwise `NULL`     |
| `actor_id`     | Who made the change (a ULID made on first launch, ADR-0003)                                   |
| `device_id`    | On which device (likewise)                                                                    |
| `ts`           | When, in milliseconds since the Unix epoch                                                    |
| `kind`         | `<entity>.<action>` (§5.2)                                                                    |
| `target_id`    | The changed row's `id`; for `object_type` and `attachment` rows, the row's `object_id`        |
| `payload_json` | The payload (§5.3)                                                                            |
| `prev_hash`    | The previous op's `hash`; `NULL` for the first op of a vault                                  |
| `hash`         | The op's hash (§5.4)                                                                          |
| `signature`    | Reserved for shared spaces (M15); `NULL`                                                      |

### 5.2 Kinds

An op changes exactly one row. Its entity names the table: `type`, `field`, `object`,
`object_type`, `block`, `relation`, `attachment` or, from migration 2, `view`. The actions are:

| Action    | Entities             | Payload | Columns it may change                                                                                            |
| --------- | -------------------- | ------- | ---------------------------------------------------------------------------------------------------------------- |
| `create`  | all                  | row     | (inserts the row)                                                                                                |
| `delete`  | all                  | row     | (removes the row)                                                                                                |
| `update`  | `type`               | update  | `name`, `plural`, `icon`, `color`, `extends`, `description`                                                      |
| `update`  | `field`              | update  | `label`, `options_json`, `required`, `sort_order`                                                                |
| `update`  | `object`             | update  | `title`, `summary`, `captured_at`, `event_start`, `event_end`, `geo_lat`, `geo_lon`, `sensitivity`; field values |
| `update`  | `block`              | update  | `kind`, `content_json`                                                                                           |
| `update`  | `relation`           | update  | `label`, `props_json`                                                                                            |
| `update`  | `view`               | update  | `name`, `renderer`, `query_json`, `config_json`, `owner_object_id`                                               |
| `move`    | `object`, `block`    | update  | `parent_id` and `sort_key` (objects); `parent_block_id` and `sort_key` (blocks)                                  |
| `trash`   | `object`, `relation` | update  | `deleted_at`                                                                                                     |
| `restore` | `object`, `relation` | update  | `deleted_at`                                                                                                     |

Every update-shaped op (`update`, `move`, `trash`, `restore`) of a row that has an `updated_at`
column also sets it to the op's `ts`; `updated_at` never appears in a payload. No other
entity–action pairs exist.

### 5.3 Payloads

- A **row** payload (`create`, `delete`) is the whole row as a JSON object keyed by column name.
  JSON columns hold nested JSON, not strings; booleans are `true` or `false`; `NULL` is `null`. An
  `attachment` row also carries `blob_size`, the size of its blob, so that replaying it can write
  the `blobs` row (§3.1). A `delete` payload is the row exactly as it was just before it was
  removed.
- An **update** payload is `{"id": "<row id>", "set": {...}, "props": {...}}`. `set` maps each
  changed column to `{"from": <value before>, "to": <value after>}`. `props` (object updates only)
  maps each changed field key to `{"from": ..., "to": ...}`, where a missing `from` means the key
  was absent and a missing `to` means it is removed. Empty `set` or `props` members are omitted.

An op applies only if the row is as the op expects: a `create` needs the row absent, a `delete`
needs the row equal to its payload, and an update needs every `from` value to match. An app MUST
NOT apply an op whose expectation fails.

### 5.4 The hash chain

`hash` is `blake3:` followed by the lowercase hex BLAKE3-256 hash of these bytes, in order:

1. the ASCII text `dbme-op-1` followed by one zero byte;
2. `prev_hash`, `op_id`, `group_id`, `undo_of`, `actor_id` and `device_id`, each as a text value
   (below);
3. `ts` as a signed 64-bit big-endian integer;
4. `kind`, `target_id` and `payload_json`, each as a text value.

A text value is the single byte `0x00` when it is `NULL`, and otherwise the byte `0x01`, the length
of its UTF-8 encoding as an unsigned 64-bit big-endian integer, then those bytes. Columns are hashed
exactly as stored, so a reader never re-encodes JSON. `seq` and `signature` are not hashed.

### 5.5 Undo, redo and verification

- One committed transaction is one undo step. Undoing it appends, as a new step, the inverse of
  each of its ops in reverse order, all with `undo_of` set to the step's `group_id`. The inverse of
  `create` is `delete` (and back), of `trash` is `restore` (and back), and of an update is the same
  kind with every `from` and `to` swapped. Redo is the undo of an undo.
- Before undoing, the app checks that every row is still as the step left it (ignoring
  `updated_at`). If anything has changed since, the undo is refused and nothing is written: an undo
  never overwrites a later change.
- The undo history is kept in memory by the app that made the changes; it is not part of the vault.
- A vault **verifies** when SQLite's `integrity_check` and `foreign_key_check` pass; each op's
  `prev_hash` is the previous op's `hash` and its `hash` recomputes; applying the log in `seq` order
  to an empty database gives exactly the tables of §3.1; and every blob in `blobs` exists in the
  blob store with the recorded size (and, in a deep verification, the recorded hash).

## 6. The files folder (Proposed, M5; rules in ADR-0006 and ADR-0021)

The files folder is a replica of the database that any Markdown tool can edit. It is designed to
open as an Obsidian vault. The reader and writer live in `dbme-core` (`files`), whose module
documentation is the reference for every rule below; the folder's engine commits outside edits
through the store like a second device (ADR-0021).

### 6.1 Paths

- An object lives at `files/<Space>/<ancestors>/<Title>.md`; until spaces exist, a top-level object
  stands where `<Space>` will. Its children live in a folder with the same name, next to it.
- File names are derived from titles: `\ / : * ? " < > | # ^ [ ]` and control characters are
  replaced, leading and trailing dots and spaces are removed, a Windows reserved name (`CON`,
  `NUL`, `COM1`, …) gets `_` before its extension, an empty name becomes `Untitled`, and a name is
  cut to 120 bytes at a character boundary. A sibling whose name is already taken gets ` (2)`,
  ` (3)`, …, compared by `path_key`: NFC, case-folded, one slash form. A relative path longer than
  200 bytes is cut from its deepest names, never below eight bytes each.
- Dot-prefixed names are not the app's, except `.dbme/`, which holds `schema.json` (§6.5) and
  `trash/<id>.md`, where a trashed object's file goes until it is restored.
- The path is a convenience. The object's identity is the `id` in its frontmatter.

### 6.2 Frontmatter

```yaml
---
id: 01JB3R7Q8Y5K3M2N4P6Q8R0S2T
types: [Task]
tags: [travel, admin]
created: 2026-10-25T08:21:11.000Z
updated: 2026-10-25T09:02:40.000Z
due: 2026-11-30
status: Waiting
relations:
  assigned_to:
    - id: 01JB3R9C0D4E5F6G7H8J9K0M1N
      link: "[[People/Alex Doe]]"
links:
  - id: 01JB3RB6QH1WJ2XK3YL4ZM5NA6
    link: "[[Projects/Alpha]]"
    type: related
---
```

- The envelope keys, in this order: `id`, `types`, `tags`, `created`, `updated`, `summary`,
  `captured_at`, `event_start`, `event_end`, `geo_lat`, `geo_lon`, `sensitivity`, `sort` (the
  object's sort key; a change is a move). `updated` is informational on the way in.
- **User fields sit at the top level**, as Obsidian properties, in the schema's field order, each
  encoded by its field kind (§3.3; `schema.json` gives the kind on a rebuild). The envelope keys,
  `relations`, `links`, `fields`, and Obsidian's `aliases` and `cssclasses` are reserved: a user
  field whose key is reserved is written under `fields:`, and a reader accepts both shapes.
- `relations:` lists relations that belong to a field, by field key, each `{id, link}`; `links:`
  lists relations with no field, each `{id, link, type, label?, props?, block?}`. The ID is
  authoritative; the wikilink is for Obsidian and for people. If the two disagree, the ID wins.
- `tags:` and `#tag` in the body are `tagged` relations to `Tag` objects by title. `mentions`
  relations are never written; they are derived from the body.
- Any other key round-trips verbatim through `props_json._frontmatter`.
- `id` is REQUIRED for a file the app wrote. A file without one is matched by path, then by content
  hash, and is a new object otherwise (ADR-0006); nothing is ever trashed on an inference.
- Limits: a file over 16 MiB, a frontmatter over 256 KiB or nested deeper than 64 levels, and a
  block tree deeper than 64 levels are refused as a parse error, bytes untouched.

### 6.3 Body

- Blocks are written as Markdown in order. Every block ends with an Obsidian block anchor
  ` ^<block ID>`: on the line for paragraphs, list items and images; on its own line, with a blank
  line before and after, for headings, code fences, dividers (`***`), quotes and tables. A lone
  `^id` line attaches to the block before it. An empty paragraph is its anchor alone.
- List items nest by the parent's marker width (two spaces under `- `, three under `1. `), and are
  reparented by indentation on the way in.
- Links to other objects are wikilinks naming the path: `[[Projects/Alpha]]`,
  `[[Projects/Alpha|shown text]]`, `[[Projects/Alpha#^block]]`; embeds are `![[…]]`. The alias,
  block reference and embed survive as the `shown`, `block` and `embed` keys of the mention run
  (§3.5). A wikilink that resolves to no object is text.
- An attachment is `![[attachments/<hash><ext>]]`. A block kind the file format does not spell is a
  ` ```dbme ` fence holding its JSON.
- The round trip is normalising: `parse(serialize(x))` is a fixed point of the model, and a file
  the app wrote is written back byte for byte.

### 6.4 Not in the files folder

Canvas layouts, view configurations, the op log and the AI audit log stay in the database. They are
exported as read-only JSON.

### 6.5 Merge, identity and `schema.json`

- `.dbme/schema.json` holds every type and field (the content of the export's `types.json`), so
  a folder can be read without the database.
- An outside edit is merged on the model, not the bytes: frontmatter field by field and the body
  block by block against the last synced text (`mirror_state.base_text`), one side's change taken
  as is, both sides' changes to one block merged line-wise, and a conflict only when that fails. On
  a conflict the app's version stays, the file's becomes a conflict copy in the inbox, and the file
  is rewritten only after the copy is saved.
- An inbound apply never writes the file back; only an app-side change rewrites it. Outside edits
  are committed under the device the files folder derives from the app's (`mirror_device_id`) and
  are not steps of the app's undo stack.

## 7. Export folder (Proposed, M5)

- `export/schema/types.json`: every type with its fields.
- `export/views/<view-id>.json`: one file per saved view.
- `export/data/<Type>.csv`: one CSV per type, one row per live (not trashed) object of the type:
  `id`, `title`, `parent_id`, `created_at`, `updated_at`, then the type's fields in `sort_order`,
  inherited fields included. Multi-valued fields are JSON arrays in a single cell. RFC 4180
  quoting, LF line ends.

These files are export-only: a reader for other tools, not a copy of the vault. The **full export**
adds what makes it one:

- `export/vault_meta.json`: the `vault_meta` table as an object (`vault_id`, `created_at`,
  `created_by`, `format_version`).
- `export/ops.jsonl`: the op log, one op per line in `seq` order, every column of §5.1 included.
- `export/blobs/`: the blob store, same layout as §4.

An import creates an empty vault under the exported `vault_id` and `vault_meta`, **without** the
built-in types, copies the blobs, and appends every op in order, applying each; the hash chain it
builds MUST equal the one exported, hash for hash, or the import fails and leaves nothing. A full
export followed by an import therefore reproduces every table and the chain (M5 tests this).

## 8. Logs (Draft, M11)

`logs/ai-audit.jsonl` holds one JSON object per line, with the same fields as the `ai_audit` table.

## 9. Versioning

- `FORMAT_VERSION` is an integer that increases with every incompatible change. This draft is
  version 0.
- Database changes happen only through numbered migrations, and every migration updates this
  document. Each migration runs in its own transaction and sets `user_version` to its number, so a
  vault is never left half-migrated.
- An app MUST refuse to modify a vault with a newer format than it supports, and SHOULD open it read
  only.

## 10. Changes to this document

| Version | Date       | Change                                                                                                                                          |
| ------- | ---------- | ----------------------------------------------------------------------------------------------------------------------------------------------- |
| Draft 0 | 2026-10-25 | First draft (M0).                                                                                                                               |
| Draft 1 | 2026-10-25 | M1: vault lock and creation (§1), sort keys (§2.1), the tables of migration 1 (§3), field values (§3.3) and the op log (§5) are decided.        |
| Draft 2 | 2026-10-25 | M2: block content and mentions (§3.5) are decided.                                                                                              |
| Draft 3 | 2026-11-01 | M3: `related` is the `rel_type` of a relation drawn on the mind map; it has no `field_id` or `src_block_id` (§3.1).                             |
| Draft 4 | 2026-11-02 | M4: migration 2 adds `views` and the derived search tables (§3.1), and the `view` entity (§5.2). Proposed, with ADR-0020.                       |
| Draft 5 | 2026-11-02 | M5: migration 3 adds the files folder's bookkeeping (§3.1); the files folder (§6) and the full export and import (§7). Proposed, with ADR-0021. |
