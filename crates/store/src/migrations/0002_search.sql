-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Migration 2: search and saved views (M4, ADR-0020). FORMAT.md §3 documents every table.

-- Saved views: a query, a renderer and its settings. Written through the op log like the tables of
-- migration 1.
CREATE TABLE views (
  id              TEXT PRIMARY KEY NOT NULL,
  name            TEXT NOT NULL,
  renderer        TEXT NOT NULL,
  query_json      TEXT NOT NULL,
  config_json     TEXT NOT NULL,
  owner_object_id TEXT REFERENCES objects(id),
  created_at      INTEGER NOT NULL,
  updated_at      INTEGER NOT NULL
) STRICT;

-- The objects whose text changed since the index last looked at them. Every transaction adds the
-- objects it touched; the next search brings their rows up to date and empties this.
CREATE TABLE search_dirty (
  object_id TEXT PRIMARY KEY NOT NULL
) STRICT, WITHOUT ROWID;

-- The text of every live object, one row each. Derived: it can be rebuilt from objects and blocks
-- at any time.
CREATE TABLE search_text (
  doc        INTEGER PRIMARY KEY,
  object_id  TEXT NOT NULL UNIQUE,
  title      TEXT NOT NULL,
  body       TEXT NOT NULL,
  props_text TEXT NOT NULL
) STRICT;

-- The full-text index over search_text. External content: the index holds only the terms, and
-- reads the text back from search_text by rowid for snippets.
CREATE VIRTUAL TABLE fts USING fts5(
  title, body, props_text,
  content='search_text', content_rowid='doc',
  tokenize='unicode61 remove_diacritics 2'
);

CREATE TRIGGER search_text_insert AFTER INSERT ON search_text
BEGIN
  INSERT INTO fts (rowid, title, body, props_text)
  VALUES (new.doc, new.title, new.body, new.props_text);
END;
CREATE TRIGGER search_text_delete AFTER DELETE ON search_text
BEGIN
  INSERT INTO fts (fts, rowid, title, body, props_text)
  VALUES ('delete', old.doc, old.title, old.body, old.props_text);
END;
CREATE TRIGGER search_text_update AFTER UPDATE ON search_text
BEGIN
  INSERT INTO fts (fts, rowid, title, body, props_text)
  VALUES ('delete', old.doc, old.title, old.body, old.props_text);
  INSERT INTO fts (rowid, title, body, props_text)
  VALUES (new.doc, new.title, new.body, new.props_text);
END;
