# ADR-0020: Search, the query DSL and saved views

- **Status:** Proposed
- **Date:** 2026-11-02
- **Milestone:** M4

## Context

M4 adds full-text search, facets, the query DSL, saved views and a command palette (handoff brief
§8), with two acceptance figures: a search answers within 50 ms on 100,000 objects, on CI, and the
DSL has golden tests. The brief fixes the engine (SQLite FTS5, ADR-0002), the DSL's rough shape
(`from`, `where`, `search`, `relatedTo`, `sort`, `limit`), and that saved views are "a first-class,
exportable thing". It leaves open how the index is kept in step with a vault whose every change is
an op, how a query becomes SQL without any of it being spliced in, and where saved views live.

The index has to cover text that is not in one column: an object's body is the plain text of its
blocks, which live in JSON in another table. And the M1 budget, 10,000 objects created, edited,
deleted and undone in under 2 s, is measured on every change path; whatever search adds to a
change is paid 40,000 times there.

## Decision

- **Three derived tables, kept by the store, not by the op log.** Migration 2 adds `search_text`
  (one row per live object: `title`, `body`, `props_text`), `fts` (an FTS5 index over it, external
  content, `unicode61 remove_diacritics 2`, kept in step by triggers) and `search_dirty`. None of
  them is produced by ops, and `verify` does not replay them; all three can be rebuilt from the
  objects and blocks (`Vault::rebuild_search_index`), and a vault that reaches migration 2 with
  objects in it gets its index built in the migration's transaction.
- **Changes queue, reads flush.** A transaction notes the objects whose text it may have changed in
  `search_dirty`, one short insert each: every object op except a move, and the object of every
  block op. Nothing else happens at commit. The next search, query with search text, or `verify`
  brings those rows up to date first and empties the queue. So undo, redo, trash and restore are
  covered without search knowing about ops; a bulk change pays nothing for the index until someone
  searches; and a search never sees stale text. Measured on the M1 budget in a release build: with
  the index written at commit the run took 1.99 s against 2 s; with the queue it takes 1.28 s.
- **The body** is the object's summary, then its blocks in tree order, one line each: the text of
  inline blocks with mentions left out, the code of a code block, the description of an image.
  `props_text` is the string and number values of its fields, and the strings in lists.
- **Ranking and the typed text.** BM25 with the title weighted 10, field values 2, the body 1. What
  a person types never reaches FTS5's query syntax: it is split into words, each quoted, joined as
  "all of these"; the last word, while it is still being typed, matches as a prefix; text in double
  quotes is a phrase, and a quote always ends one. The snippet comes from whichever column matched
  best, the matching words in `[` and `]`.
- **The DSL** is a JSON `Query` in `dbme-core` (`query.rs`), with no SQL in it: `from` (`types`,
  each with the types that extend it, and `under`, everything below one object), `where` (a list of
  conditions that must all hold), `search`, `related_to` (one hop, either direction, optionally one
  relation type), `sort`, `limit` (at most 1,000) and `offset`. A condition names a built-in column
  with a leading `$` (`$title`, `$summary`, `$created_at`, `$updated_at`, `$captured_at`,
  `$event_start`, `$event_end`, `$sensitivity`) or a field by its key, and compares with `eq`,
  `ne`, `lt`, `le`, `gt`, `ge`, `contains`, `starts_with`, `is_empty`, `is_not_empty`, `in` or
  `has`. `Sort` moves from the store to the DSL, with a new `relevance`: the BM25 rank with search
  text, most recently changed first without. The store compiles a query to one statement with every
  value bound as a parameter (`compile.rs`); the only text from the query that reaches the SQL is
  its shape. `tests/golden/` holds the SQL and parameters of twelve queries.
- **The index drives the join.** A query with search text reads `FROM fts CROSS JOIN search_text
CROSS JOIN objects`, which fixes that order, as SQLite documents for `CROSS JOIN`, and its type
  filter is an `EXISTS` on `object_types` per candidate rather than an `IN` over every object of
  the types. Left to choose, the planner ran the full-text match once per object when a type filter
  and a field condition were also present: 29 s on 100,000 objects. With the order fixed but the
  `IN`, gathering every object of a common type cost 29 ms; with `EXISTS`, the same query takes
  4.6 ms.
- **Facets** are the counts of a query's results per type (`$type`) or per value of one field, most
  common first, up to 50; a list value (a multi-select) counts each item. They are computed over the
  whole result, not one page.
- **Saved views go through the op log.** A `views` row (`name`, `renderer`, `query_json`,
  `config_json`, `owner_object_id`) is written by `view.create`, `view.update` and `view.delete`
  ops, like every other table, so Undo covers saving, renaming and deleting a view, and `verify`
  replays them. A view's query is checked when the row is, so a malformed query never lands.
- **The palette** is Ctrl+K (⌘K) anywhere, and a Search button in the header for phones. It is an
  ARIA combobox over a listbox. Typing searches the vault and shows a title with its snippet; Enter
  opens, Ctrl+Enter shows every result; a line that starts with `>` lists commands instead. The
  results surface shows a query's results with facet chips that narrow it, and saves a search as a
  view; a saved view opens there from the sidebar or the palette, and can be narrowed further,
  renamed, saved or deleted.
- **Nine commands:** `search`, `objects_query`, `objects_query_count`, `facets`, `views_list`,
  `view_get`, `view_save`, `view_update`, `view_delete`. 45 in all (ADR-0010).

## Consequences

- The first search after a large change pays for indexing it: about 0.3 s after 10,000 edits on
  the development machine. A single edit costs the next search one row.
- `Reader` writes to the derived tables when it flushes. That is not a change to the vault (no op,
  nothing to undo), but it means a read can take the write lock for a moment.
- Measured in a release build on the development machine, 100,000 objects, half with a paragraph,
  best of three: one common word 5.4 ms, two words 2.1 ms, a prefix 5.0 ms, a phrase 1.0 ms, a rare
  word 0.05 ms, a field value 12.9 ms, a search with a type filter and a field condition 4.6 ms, its
  count 2.1 ms and its facets 2.8 ms, all gated at 50 ms. Seeding took 4.5 s, and the first search
  after it brought the index up to date with all 100,000 objects in 1.3 s. One page of everything
  (16 ms) and the type facet of everything (21 ms) are printed, not gated.
- The `search` command keeps the plain title lookup (`object_search`) beside it, which "Move to…"
  and "Relate to…" still use: those want titles that start with what was typed, not bodies.
- Only select and multi-select fields become facet chips; other kinds wait for the filter editor
  (M8), as do saved views in the Table and Gallery ("saved views everywhere", build plan v1).
- The brief's "typed as text" form of the DSL is not a second grammar: the palette's `>` prefix and
  plain words are all the typing there is in M4. A text grammar arrives with NL → DSL (M11).

## Alternatives considered

- **SQL triggers on `objects` and `blocks` keeping `fts` directly.** The body is text drawn from
  block JSON by rules that live in Rust (`plain_text`, mentions left out); a second copy of those
  rules in SQL would drift.
- **Writing the index at commit.** Simplest, and correct, but it put the M1 budget at 1.99 s of 2 s
  in a release build here, which is a failure on a shared runner.
- **`fts` holding the text itself, with `object_id` as an unindexed column.** Replacing a row by
  `object_id` then scans the whole index; the content table has a unique index on it.
- **Letting the planner choose the join order.** See the 29 s above.
- **Views outside the op log, like `blobs`.** Saving and deleting a view are things people do and
  undo; and `verify` could not check them.
- **`SELECT … LIKE` over titles and bodies.** Fine for 1,000 objects; no ranking, no snippets, and
  no 50 ms at 100,000.
