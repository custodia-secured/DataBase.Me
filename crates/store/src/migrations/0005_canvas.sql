-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Migration 5: the canvas (M8, ADR-0035). FORMAT.md §3 documents every table.

-- One element of a canvas: a shape, a line, a piece of text, or a card, which is an element with an
-- `object_id`. One row an element, rather than one scene in the view's settings, so that undo, the op
-- log and a later merge see single elements (M8 Decision 2). `item_id` is the drawing library's own name
-- for the element, which other elements use to refer to it; `z` is its fractional place in the stack;
-- `style_json` is everything else about it. Written through the op log like the tables of migration 1.
CREATE TABLE canvas_items (
  id         TEXT PRIMARY KEY NOT NULL,
  view_id    TEXT NOT NULL REFERENCES views(id),
  item_id    TEXT NOT NULL,
  object_id  TEXT REFERENCES objects(id),
  x          REAL NOT NULL,
  y          REAL NOT NULL,
  w          REAL NOT NULL,
  h          REAL NOT NULL,
  z          TEXT NOT NULL,
  style_json TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  UNIQUE (view_id, item_id)
) STRICT;
CREATE INDEX canvas_items_object ON canvas_items (object_id) WHERE object_id IS NOT NULL;
