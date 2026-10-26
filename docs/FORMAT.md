<!-- SPDX-License-Identifier: Apache-2.0 -->

# DataBase.Me vault format

**Status: draft 2, 2026-10-25. Format version 0.** Nothing here is stable until FORMAT.md v0.1
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

Rules the schema does not express, which an app MUST keep:

- An object's parents never loop, and a block's parents never loop or leave its object. A type's
  `extends` chain never loops.
- Trashing an object sets `deleted_at` on it and on every live object below it; restoring clears
  them. An object is deleted for good only from the Trash, together with its blocks, its type
  assignments, its attachments and the relations that start or end at it.
- A system type (`is_system` = 1) is never deleted. A type is deleted only when no object has it,
  no type extends it and no relation uses one of its fields; its fields go with it.
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
| `views`        | Saved views: a query, a renderer and its settings                     | `id`, `name`, `renderer`, `query_json`, `config_json`, `owner_object_id`, `created_at`, `updated_at`                                                                                                                                                     |
| `canvas_items` | Positions on a canvas view                                            | `view_id`, `item_id`, `object_id`, `x`, `y`, `w`, `h`, `z`, `style_json`                                                                                                                                                                                 |
| `spaces`       | Top-level spaces and their AI policy                                  | `id`, `name`, `ai_policy` (`none`, `local_only`, `any`), `settings_json`                                                                                                                                                                                 |
| `ai_audit`     | Every AI call                                                         | `id`, `ts`, `provider`, `model`, `purpose`, `object_ids_json`, `bytes_sent`, `user_initiated`                                                                                                                                                            |
| `mirror_state` | Files-folder bookkeeping per object                                   | `object_id`, `rel_path` (unique), `last_hash`, `base_text` (last synced Markdown), `synced_seq`, `synced_at`                                                                                                                                             |
| `conflicts`    | Things the user must resolve                                          | `id`, `object_id`, `kind` (`edit_edit`, `parse_error`, `bulk_change_hold`, `missing_id`, `cloud_conflict_file`), `details_json`, `conflict_copy_id`, `created_at`, `resolved_at`                                                                         |
| `fts`          | FTS5 full-text index (derived; can be rebuilt)                        | `object_id`, `title`, `body`, `props_text`                                                                                                                                                                                                               |
| `vec_chunks`   | sqlite-vec embeddings (derived; only with an embedding provider, M11) | `chunk_id`, `embedding`                                                                                                                                                                                                                                  |

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
`object_type`, `block`, `relation` or `attachment`. The actions are:

| Action    | Entities             | Payload | Columns it may change                                                                                            |
| --------- | -------------------- | ------- | ---------------------------------------------------------------------------------------------------------------- |
| `create`  | all                  | row     | (inserts the row)                                                                                                |
| `delete`  | all                  | row     | (removes the row)                                                                                                |
| `update`  | `type`               | update  | `name`, `plural`, `icon`, `color`, `extends`, `description`                                                      |
| `update`  | `field`              | update  | `label`, `options_json`, `required`, `sort_order`                                                                |
| `update`  | `object`             | update  | `title`, `summary`, `captured_at`, `event_start`, `event_end`, `geo_lat`, `geo_lon`, `sensitivity`; field values |
| `update`  | `block`              | update  | `kind`, `content_json`                                                                                           |
| `update`  | `relation`           | update  | `label`, `props_json`                                                                                            |
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

## 6. The files folder (Draft, M5; rules in ADR-0006)

The files folder is a replica of the database that any Markdown tool can edit. It is designed to
open as an Obsidian vault.

### 6.1 Paths

- An object lives at `files/<Space>/<ancestors>/<Title>.md`. Its children live in a folder with the
  same name, next to it: `files/<Space>/<ancestors>/<Title>/`.
- File names are derived from titles. Characters that are invalid on any supported OS or that break
  wikilinks (`\ / : * ? " < > | # ^ [ ]`) are replaced, trailing dots and spaces are removed,
  Windows reserved names (`CON`, `NUL`, `COM1`, ...) are avoided, and siblings with the same title
  are told apart (Open: exact rule and length limit).
- The path is a convenience. The object's identity is the `id` in its frontmatter.

### 6.2 Frontmatter

```yaml
---
id: 01JB3R7Q8Y5K3M2N4P6Q8R0S2T
types: [Task]
tags: [travel, admin]
created: 2026-10-25T08:21:11.000Z
updated: 2026-10-25T09:02:40.000Z
fields:
  due: 2026-11-30
  status: Waiting
relations:
  assigned_to:
    - id: 01JB3R9C0D4E5F6G7H8J9K0M1N
      link: "[[Alex Doe]]"
---
```

- `id` is REQUIRED for a file the app wrote. A file without one is a new object, or is matched by
  content hash (ADR-0006).
- Relations carry both the target's ID (authoritative) and a wikilink (for Obsidian and for
  people). If the two disagree, the ID wins.
- Open (M5): whether user fields sit under `fields:` as above or at the top level as Obsidian
  properties, which would make Obsidian's property editor and Bases work on them at the cost of
  reserving key names.

### 6.3 Body

- Blocks are written as Markdown in order. A block that has an ID ends with an Obsidian block
  anchor: ` ^01JB3RB6...`. Open (M5): whether every block carries an anchor or only blocks that are
  referenced elsewhere.
- Links to other objects are wikilinks: `[[Title]]`, `[[path/Title|shown text]]`, `[[Title#^block]]`.
  Embeds (transclusions) are `![[Title]]` or `![[Title#^block]]`.
- Tags are `#tag` in the body or `tags:` in the frontmatter.

### 6.4 Not in the files folder

Canvas layouts, view configurations, the op log and the AI audit log stay in the database. They are
exported as read-only JSON.

## 7. Export folder (Draft, M5)

- `export/schema/types.json`: every type with its fields.
- `export/views/<view-id>.json`: one file per saved view.
- `export/data/<Type>.csv`: one CSV per type, one row per object: `id`, `title`, `parent_id`,
  `created_at`, `updated_at`, then the type's fields in `sort_order`. Multi-valued fields are JSON
  arrays in a single cell.

These files are export-only. A full export followed by an import MUST reproduce the vault
losslessly (M5 tests this).

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

| Version | Date       | Change                                                                                                                                   |
| ------- | ---------- | ---------------------------------------------------------------------------------------------------------------------------------------- |
| Draft 0 | 2026-10-25 | First draft (M0).                                                                                                                        |
| Draft 1 | 2026-10-25 | M1: vault lock and creation (§1), sort keys (§2.1), the tables of migration 1 (§3), field values (§3.3) and the op log (§5) are decided. |
| Draft 2 | 2026-10-25 | M2: block content and mentions (§3.5) are decided.                                                                                       |
