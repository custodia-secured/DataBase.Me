<!-- SPDX-License-Identifier: Apache-2.0 -->

# DataBase.Me vault format

**Status: draft 0, 2026-10-25. Format version 0.** Nothing here is stable until FORMAT.md v0.1
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

## 2. Encodings (Decided, ADR-0003)

- **Text** is UTF-8 without a byte-order mark. Files written by the app use LF line endings.
- **IDs** are ULIDs: 26 characters of Crockford base32, upper case. Readers SHOULD accept lower
  case.
- **Timestamps** are integer milliseconds since the Unix epoch (UTC) inside the database, and
  ISO 8601 UTC strings with millisecond precision (`2026-10-25T08:21:11.000Z`) in files and exports.
- **Hashes** are `<algorithm>:<lowercase hex>`, for example `blake3:9f86...`. The only algorithm
  today is BLAKE3 with a 256-bit output. A reader MUST NOT guess an unknown algorithm.
- **Sort keys** are strings that order siblings lexicographically (byte order). Ties are broken by
  ID. The key algorithm is fixed in M1 (Draft).
- **JSON columns** (`*_json`) hold UTF-8 JSON text.

## 3. The database, `vault.sqlite` (Draft, M1)

The file is an ordinary SQLite 3 database.

- `PRAGMA application_id` is `0x44424D45` (the ASCII bytes `DBME`).
- `PRAGMA user_version` is the number of the last schema migration applied. An app MUST NOT write
  to a vault whose `user_version` is newer than it understands.
- The journal mode is WAL.

### 3.1 Tables

The schema below is the draft from the handoff brief. M1 fixes column types and constraints.

| Table          | Purpose                                                               | Columns                                                                                                                                                                                                                                                                             |
| -------------- | --------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `types`        | Object types (user-editable)                                          | `id`, `name`, `plural`, `icon`, `color`, `extends` (parent type ID, for inheritance), `description`, `is_system`, `created_at`, `updated_at`                                                                                                                                        |
| `fields`       | Field definitions of a type                                           | `id`, `type_id`, `key` (unique within the type), `label`, `kind`, `options_json`, `required`, `sort_order`                                                                                                                                                                          |
| `objects`      | Every object                                                          | `id`, `title`, `summary`, `parent_id` (its one home in the hierarchy), `sort_key`, `props_json` (field values keyed by field key), `created_at`, `updated_at`, `captured_at`, `event_start`, `event_end`, `geo_lat`, `geo_lon`, `deleted_at` (set when in the Trash), `sensitivity` |
| `object_types` | Which types an object has (an object can have several)                | `object_id`, `type_id`                                                                                                                                                                                                                                                              |
| `blocks`       | The content tree inside an object                                     | `id`, `object_id`, `parent_block_id`, `sort_key`, `kind`, `content_json`, `created_at`, `updated_at`                                                                                                                                                                                |
| `relations`    | Typed, directed links; backlinks are the reverse direction            | `id`, `src_id`, `dst_id`, `rel_type`, `field_id`, `src_block_id`, `label`, `props_json`, `created_at`, `deleted_at`                                                                                                                                                                 |
| `blobs`        | Files in the blob store                                               | `hash`, `size`, `mime`, `encrypted`, `created_at`                                                                                                                                                                                                                                   |
| `attachments`  | Which object uses which blob, and how                                 | `object_id`, `blob_hash`, `role` (`original`, `thumbnail`, `transcript`, `ocr_text`, `snapshot`), `filename`                                                                                                                                                                        |
| `provenance`   | Where an object came from                                             | `id`, `object_id`, `source_kind` (`manual`, `import:<importer>`, `clip`, `feed`, `file:external`, `ai:<model>`, `plugin:<id>`), `source_uri`, `import_run_id`, `original_blob`, `extractor`, `extractor_version`, `confidence`, `accepted`, `created_at`                            |
| `views`        | Saved views: a query, a renderer and its settings                     | `id`, `name`, `renderer`, `query_json`, `config_json`, `owner_object_id`, `created_at`, `updated_at`                                                                                                                                                                                |
| `canvas_items` | Positions on a canvas view                                            | `view_id`, `item_id`, `object_id`, `x`, `y`, `w`, `h`, `z`, `style_json`                                                                                                                                                                                                            |
| `ops`          | The append-only operation log                                         | `seq`, `op_id`, `actor_id`, `device_id`, `ts`, `kind`, `target_id`, `payload_json`, `prev_hash`, `hash`, `signature`                                                                                                                                                                |
| `spaces`       | Top-level spaces and their AI policy                                  | `id`, `name`, `ai_policy` (`none`, `local_only`, `any`), `settings_json`                                                                                                                                                                                                            |
| `ai_audit`     | Every AI call                                                         | `id`, `ts`, `provider`, `model`, `purpose`, `object_ids_json`, `bytes_sent`, `user_initiated`                                                                                                                                                                                       |
| `mirror_state` | Files-folder bookkeeping per object                                   | `object_id`, `rel_path` (unique), `last_hash`, `base_text` (last synced Markdown), `synced_seq`, `synced_at`                                                                                                                                                                        |
| `conflicts`    | Things the user must resolve                                          | `id`, `object_id`, `kind` (`edit_edit`, `parse_error`, `bulk_change_hold`, `missing_id`, `cloud_conflict_file`), `details_json`, `conflict_copy_id`, `created_at`, `resolved_at`                                                                                                    |
| `fts`          | FTS5 full-text index (derived; can be rebuilt)                        | `object_id`, `title`, `body`, `props_text`                                                                                                                                                                                                                                          |
| `vec_chunks`   | sqlite-vec embeddings (derived; only with an embedding provider, M11) | `chunk_id`, `embedding`                                                                                                                                                                                                                                                             |

### 3.2 Built-in types

Every new vault contains these editable system types: Note, Page, Task, Person, Organization,
Place, Event, Project, Area, Resource, Media, WebClip, Document, Image, Audio, Video, Email,
Message, Contact, Dataset, Tag, Collection, Template, Highlight, Journal.

### 3.3 Field kinds and value encoding (Draft, M1)

Field values live in `objects.props_json` under the field's `key`.

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

## 4. Blob store (Decided layout, ADR-0002; encryption Open, M9)

A blob with hash `blake3:<hex>` is stored at `blobs/<hex[0..2]>/<hex[2..4]>/<hex>`, for example
`blobs/9f/86/9f86d0...`. Blobs never change. When vault encryption is on, each blob is encrypted
with XChaCha20-Poly1305; the envelope format is specified in M9.

## 5. The op log (Open, M1)

Every change is an op appended to `ops`. Each op's `hash` covers its own content and the previous
op's `hash` (`prev_hash`), so altering or removing an op breaks the chain. M1 specifies the op
kinds, the payload shapes and the exact bytes that are hashed.

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
  document.
- An app MUST refuse to modify a vault with a newer format than it supports, and SHOULD open it read
  only.

## 10. Changes to this document

| Version | Date       | Change            |
| ------- | ---------- | ----------------- |
| Draft 0 | 2026-10-25 | First draft (M0). |
