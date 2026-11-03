-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Migration 4: provenance (M6, ADR-0026). FORMAT.md §3 documents every table.

-- Where an object came from: one row per import or capture that made or updated it. Written
-- through the op log like the tables of migration 1, so a run's rows undo with the run, replay in
-- verify and travel in a full export. A row is never updated. `original_blob` names the source
-- file in the blob store, usually the object's `original` attachment; it is not a foreign key to
-- `blobs`, whose rows exist only once an attachment refers to a blob and carries its size.
CREATE TABLE provenance (
  id                TEXT PRIMARY KEY NOT NULL,
  object_id         TEXT NOT NULL REFERENCES objects(id),
  source_kind       TEXT NOT NULL,
  source_uri        TEXT,
  import_run_id     TEXT,
  original_blob     TEXT,
  extractor         TEXT NOT NULL,
  extractor_version TEXT NOT NULL,
  confidence        REAL NOT NULL CHECK (confidence >= 0 AND confidence <= 1),
  accepted          INTEGER NOT NULL CHECK (accepted IN (0, 1)),
  created_at        INTEGER NOT NULL
) STRICT;
CREATE INDEX provenance_object ON provenance (object_id, created_at);
CREATE INDEX provenance_run ON provenance (import_run_id) WHERE import_run_id IS NOT NULL;
