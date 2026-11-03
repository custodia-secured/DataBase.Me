# Changelog

All notable changes to DataBase.Me are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- M0 foundations: the monorepo (Rust workspace and pnpm workspace), the Tauri 2 desktop shell,
  CI on Windows, macOS and Linux (formatting, linting, type checks, tests, builds, a launch test,
  dependency licence checks and the DCO sign-off check), the licences, governance, contribution,
  security and conduct documents, the FORMAT.md draft and ADR-0001 to ADR-0010.
- M1 vault and object core: `dbme-core` (ULIDs, fractional sort keys, BLAKE3 hashes, the data
  model with field-value validation, ops with their inverses, the hash chain, the 25 system types)
  and `dbme-store` (the SQLite vault with migrations and a one-writer lock, services for types,
  fields, objects, blocks, relations and attachments, undo and redo that never overwrite a later
  change, sort-key rebalancing, the deduplicating blob store, and `verify`, which replays the op log
  and checks every table). The CLI gains `dbme vault create` and `dbme verify`. The desktop app
  gains typed IPC commands and a `vault-changed` event, with TypeScript bindings generated from
  Rust, and its launch test now creates and undoes an object over IPC. CI gains coverage, iOS and
  Android cross-compile, and performance budget jobs. FORMAT.md §1, §2, §3 and §5 are decided;
  ADR-0012 to ADR-0015.
- M2 editor and basic views: the block editor (TipTap) with formatting, lists, to-dos, `[[`
  mentions and backlinks; the Document, Outline, Table and Gallery views, live across views and a
  split pane; the hierarchy sidebar with keyboard moves, drag-and-drop and "Move to…"; vault undo
  and redo everywhere; a phone layout with a drawer. The commands move into a `dbme-api` crate,
  and the UI into `@databaseme/client`, `@databaseme/ui` and `@databaseme/views`. End-to-end tests
  run every view in Edge and WebKit at desktop and phone sizes under the app's CSP, with axe and
  the table-scroll and typing budgets, in a new CI job. FORMAT.md §3.5 is decided; ADR-0016 to
  ADR-0018.
- M3 mind map: a Mind map view over the hierarchy and relations, laid out automatically. Add,
  rename, reorder, reparent, trash, fold and relate by mouse, keyboard or touch, with undo, live
  across views. Far away, a canvas draws the whole map as plain boxes. A new `related` relation
  type, and the `object_tree`, `relation_create` and `relation_trash` commands; ADR-0019.
- M4 search and query: full-text search over titles, bodies and field values (SQLite FTS5, kept in
  step with every change and its undo), ranked, with snippets; the query DSL (`from`, `where`,
  `search`, `related_to`, `sort`, `limit`), compiled to SQL with golden tests; facets per type and
  per field value; saved views, written through the op log; the command palette (Ctrl+K) with
  commands behind `>`; the results surface that narrows a query with facet chips and saves it as a
  view; a 100,000-object search benchmark in the budget job. Migration 2 adds `views`,
  `search_dirty`, `search_text` and `fts`. Nine new commands; ADR-0020.
- M5 two-way files folder and export: every object as a Markdown file with YAML frontmatter and
  block anchors in a folder that opens as an Obsidian vault (`files/` inside the vault, or a folder
  of your choosing); edits made there in any tool come back as ordinary changes, merged block by
  block against the last synced text, with a Conflicts inbox for a true conflict, an unreadable
  file, a bad value, a duplicate id, a cloud drive's conflicting copy, and a hold when many files
  change at once; renames and moves follow; deleted files go to the Trash; other files dropped in
  become objects with the file attached; a vault can be rebuilt from the folder alone. A Files
  dialog with Turn on, Sync now and Turn off, a status line, a hold banner and the inbox. JSON and
  CSV export, and a full export (`ops.jsonl`, `vault_meta.json`, `blobs/`) that imports to the
  same vault, hash for hash. The CLI gains `export`, `import` and `mirror-check`. Migration 3 adds
  `mirror_state`, `mirror_dirty`, `mirror_meta` and `conflicts`. Eight new commands and a
  `mirror-changed` event. New crates `dbme-mirror` and `dbme-export`; the file format in
  `dbme-core`. CI gains the 10,000-interleavings property, the files-folder budgets, the watcher
  probes on three systems, nightly fuzzing of the parser, and the export crate in the coverage
  gate. FORMAT.md §3.1 (migration 3), §6 and §7; ADR-0021 to ADR-0025.
