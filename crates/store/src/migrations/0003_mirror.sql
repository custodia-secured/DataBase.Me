-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Migration 3: the files folder's bookkeeping (M5, ADR-0021). FORMAT.md §3 documents every table.
--
-- None of these tables is written through the op log: they describe the state of files on disk,
-- which the log does not govern, and they can be rebuilt by a rescan of the folder. They carry no
-- foreign keys, like search_dirty, so a permanent delete replayed by `verify` never fails on them;
-- `dbme mirror-check` sweeps orphans instead.

-- One row per object the folder holds or held: where its file is, what was last synced, and the
-- Markdown that sync produced (the base of the next three-way merge).
CREATE TABLE mirror_state (
  object_id     TEXT PRIMARY KEY NOT NULL,
  rel_path      TEXT NOT NULL UNIQUE,
  path_key      TEXT NOT NULL UNIQUE,
  last_hash     TEXT,
  base_text     TEXT,
  synced_seq    INTEGER,
  synced_at     INTEGER,
  file_size     INTEGER,
  file_mtime_ms INTEGER,
  state         TEXT NOT NULL CHECK (state IN ('synced', 'pending', 'held', 'excluded', 'trashed'))
) STRICT;
CREATE INDEX mirror_state_state ON mirror_state (state);

-- The objects whose file must be written again. Every app-side transaction adds the objects it
-- changed; the mirror takes them and empties this.
CREATE TABLE mirror_dirty (
  object_id TEXT PRIMARY KEY NOT NULL
) STRICT, WITHOUT ROWID;

-- Facts about the folder as a whole: whether it is enabled, whether every file needs a rewrite.
CREATE TABLE mirror_meta (
  key   TEXT PRIMARY KEY NOT NULL,
  value TEXT NOT NULL
) STRICT;

-- Things the person must resolve: the Conflicts inbox.
CREATE TABLE conflicts (
  id               TEXT PRIMARY KEY NOT NULL,
  object_id        TEXT,
  rel_path         TEXT,
  kind             TEXT NOT NULL CHECK (kind IN ('edit_edit', 'edit_delete', 'parse_error',
                     'invalid_value', 'duplicate_id', 'dropped_file', 'bulk_change_hold',
                     'missing_id', 'cloud_conflict_file')),
  details_json     TEXT NOT NULL,
  conflict_copy_id TEXT,
  resolution       TEXT,
  created_at       INTEGER NOT NULL,
  resolved_at      INTEGER
) STRICT;
CREATE INDEX conflicts_open ON conflicts (created_at) WHERE resolved_at IS NULL;
