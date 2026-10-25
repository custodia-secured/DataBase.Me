-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Migration 1: the vault and object core (M1). FORMAT.md §3 documents every table.
-- Tables used only by later milestones arrive with those milestones' migrations.

-- Facts about the vault itself, not changed by ops: its ID, when and by what it was created.
CREATE TABLE vault_meta (
  key   TEXT PRIMARY KEY NOT NULL,
  value TEXT NOT NULL
) STRICT;

CREATE TABLE types (
  id          TEXT PRIMARY KEY NOT NULL,
  name        TEXT NOT NULL,
  plural      TEXT,
  icon        TEXT,
  color       TEXT,
  extends     TEXT REFERENCES types(id),
  description TEXT,
  is_system   INTEGER NOT NULL CHECK (is_system IN (0, 1)),
  created_at  INTEGER NOT NULL,
  updated_at  INTEGER NOT NULL
) STRICT;

CREATE TABLE fields (
  id           TEXT PRIMARY KEY NOT NULL,
  type_id      TEXT NOT NULL REFERENCES types(id),
  key          TEXT NOT NULL,
  label        TEXT NOT NULL,
  kind         TEXT NOT NULL CHECK (kind IN ('text', 'rich_text', 'number', 'currency', 'boolean',
                 'date', 'datetime', 'duration', 'select', 'multi_select', 'relation', 'url',
                 'email', 'phone', 'geo', 'file', 'rating', 'formula', 'rollup', 'json')),
  options_json TEXT NOT NULL,
  required     INTEGER NOT NULL CHECK (required IN (0, 1)),
  sort_order   INTEGER NOT NULL,
  UNIQUE (type_id, key)
) STRICT;

CREATE TABLE objects (
  id          TEXT PRIMARY KEY NOT NULL,
  title       TEXT NOT NULL,
  summary     TEXT,
  parent_id   TEXT REFERENCES objects(id),
  sort_key    TEXT NOT NULL,
  props_json  TEXT NOT NULL,
  created_at  INTEGER NOT NULL,
  updated_at  INTEGER NOT NULL,
  captured_at INTEGER,
  event_start INTEGER,
  event_end   INTEGER,
  geo_lat     REAL,
  geo_lon     REAL,
  deleted_at  INTEGER,
  sensitivity TEXT NOT NULL CHECK (sensitivity IN ('normal', 'sensitive'))
) STRICT;
CREATE INDEX objects_parent ON objects (parent_id, sort_key, id);
CREATE INDEX objects_trash ON objects (deleted_at) WHERE deleted_at IS NOT NULL;

CREATE TABLE object_types (
  object_id TEXT NOT NULL REFERENCES objects(id),
  type_id   TEXT NOT NULL REFERENCES types(id),
  PRIMARY KEY (object_id, type_id)
) STRICT, WITHOUT ROWID;
CREATE INDEX object_types_type ON object_types (type_id, object_id);

CREATE TABLE blocks (
  id              TEXT PRIMARY KEY NOT NULL,
  object_id       TEXT NOT NULL REFERENCES objects(id),
  parent_block_id TEXT REFERENCES blocks(id),
  sort_key        TEXT NOT NULL,
  kind            TEXT NOT NULL,
  content_json    TEXT NOT NULL,
  created_at      INTEGER NOT NULL,
  updated_at      INTEGER NOT NULL
) STRICT;
CREATE INDEX blocks_object ON blocks (object_id, parent_block_id, sort_key, id);
CREATE INDEX blocks_parent ON blocks (parent_block_id) WHERE parent_block_id IS NOT NULL;

CREATE TABLE relations (
  id           TEXT PRIMARY KEY NOT NULL,
  src_id       TEXT NOT NULL REFERENCES objects(id),
  dst_id       TEXT NOT NULL REFERENCES objects(id),
  rel_type     TEXT NOT NULL,
  field_id     TEXT REFERENCES fields(id),
  src_block_id TEXT REFERENCES blocks(id),
  label        TEXT,
  props_json   TEXT NOT NULL,
  created_at   INTEGER NOT NULL,
  deleted_at   INTEGER
) STRICT;
CREATE INDEX rel_src ON relations (src_id, rel_type);
CREATE INDEX rel_dst ON relations (dst_id, rel_type);
CREATE INDEX rel_field ON relations (field_id) WHERE field_id IS NOT NULL;
CREATE INDEX rel_block ON relations (src_block_id) WHERE src_block_id IS NOT NULL;

CREATE TABLE blobs (
  hash       TEXT PRIMARY KEY NOT NULL,
  size       INTEGER NOT NULL CHECK (size >= 0),
  mime       TEXT,
  encrypted  INTEGER NOT NULL CHECK (encrypted IN (0, 1)),
  created_at INTEGER NOT NULL
) STRICT;

CREATE TABLE attachments (
  object_id TEXT NOT NULL REFERENCES objects(id),
  blob_hash TEXT NOT NULL REFERENCES blobs(hash),
  role      TEXT NOT NULL CHECK (role IN ('original', 'thumbnail', 'transcript', 'ocr_text', 'snapshot')),
  filename  TEXT,
  mime      TEXT,
  PRIMARY KEY (object_id, blob_hash, role)
) STRICT, WITHOUT ROWID;
CREATE INDEX attachments_blob ON attachments (blob_hash);

-- The append-only op log (FORMAT.md §5). Every other table above, except vault_meta, is what you
-- get by applying these ops in seq order to an empty vault.
--
-- Every change writes here, so every index costs every commit. seq is the rowid (no AUTOINCREMENT:
-- rows are never deleted, so seq never repeats anyway). op_id is unique, for sync (M13). The hash
-- chain already makes hash unique, and nothing in M1 looks ops up by group or target, so neither
-- has an index; the milestones that need those lookups add the indexes (ADR-0015).
CREATE TABLE ops (
  seq          INTEGER PRIMARY KEY,
  op_id        TEXT NOT NULL UNIQUE,
  group_id     TEXT NOT NULL,
  undo_of      TEXT,
  actor_id     TEXT NOT NULL,
  device_id    TEXT NOT NULL,
  ts           INTEGER NOT NULL,
  kind         TEXT NOT NULL,
  target_id    TEXT NOT NULL,
  payload_json TEXT NOT NULL,
  prev_hash    TEXT,
  hash         TEXT NOT NULL,
  signature    TEXT
) STRICT;

CREATE TRIGGER ops_append_only_update BEFORE UPDATE ON ops
BEGIN
  SELECT RAISE(ABORT, 'the op log is append-only');
END;
CREATE TRIGGER ops_append_only_delete BEFORE DELETE ON ops
BEGIN
  SELECT RAISE(ABORT, 'the op log is append-only');
END;
