# Changelog

All notable changes to DataBase.Me are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- M8 Visual v1: eight more views behind **More views**, each with **Show as list** and a phone layout.
  **Kanban**, by any list field. **Calendar**, month and week, with an agenda on a phone. **Timeline**, from
  days to decades. **Charts**, drawn by Vega-Lite from numbers the store counts. **Dashboard**, saved charts and
  views as tiles. **Map**, on coastlines that come with the app, or a PMTiles file of your own, and with no
  address search. **Graph**, on WebGL, up to 200,000 pages, with a layout you can pause. **Canvas**, on Excalidraw,
  with cards that follow their pages. Nothing here makes a network request. ADR-0031 to ADR-0035.
- Filters and saved views on every view that lists pages: the filter editor writes query conditions, and
  **Save view** keeps them with the view's settings. Nothing is saved by looking at something.
- **Suggested** at the foot of **More views**: up to three views worth trying, each with its reason, from rules in
  the app. Their agreement with labelled fixtures is 95% on a held-out set, and the labels are the author's and not an
  independent expert's.
- New reads for these: aggregates in SQL, a profile of a page's contents, the whole graph in one call, and
  `related_to` to a depth of six.

### Changed

- **The vault format moves to migration 5**, which adds `canvas_items` (FORMAT.md §3.1). A vault opened by this
  version is refused by 0.1, as a newer vault always is.
- A full export also writes `canvas/<view-id>.json` for each canvas with anything on it.

### Fixed

- A page with a saved view can be deleted from the Trash. It failed on a foreign key.
- At 360 pixels the top bar's buttons were wider than the screen, with Redo off the edge and the page scrolling
  sideways.

- The Table can add columns ([#15](https://github.com/custodia-secured/DataBase.Me/issues/15)).
  Nothing in the app could create a field or a type, and the built-in types have no fields, so only
  an import could add a column. **New column** adds one and says which type it goes on, making a
  type named after the page when the pages have none to share. **Edit columns** renames and deletes
  them, and **New row** gives a new row the types of its table, so the columns stay on it. Five new
  commands (`column_target`, `column_add`, `field_rename`, `field_delete`, `row_add`). ADR-0030.
- The mind map's rename box follows a name that changes while it is open. F2 straight after naming
  a node could open the box on the old name, and then what was typed landed in front of the new
  name, or Enter with nothing typed wrote the old name back over it.
- A PDF the parser cannot finish no longer stops an import. The 10-second budget was only checked
  between pages, so a small hostile file could hold an import without end. The parser now runs on
  a thread of its own: after 20 seconds the file is attached without its text, with a warning. A
  parser left behind holds a core until the app closes, and after two, PDFs are attached without
  trying. A later change on ADR-0026.

## [0.1.0] - 2026-11-03

The MVP: everything below, from the foundations to the release. The app makes no network request
on its own, and 0.1 has no updater; the setting that will govern one is recorded and off. The
installers are not code-signed: check a download against `SHA256SUMS.txt`, and see the user
guide's [getting started](https://github.com/custodia-secured/DataBase.Me/blob/main/docs/user-guide/getting-started.md)
page for what each system warns.

### Added

- M7 MVP release 0.1: Simple and Organizer modes (a filter on what is shown, never a change to
  the vault; "Today" opens the day's page under Journal); settings in the app's configuration
  folder (`settings.json`: mode, theme, update checks, the last vault, onboarding); light and dark
  themes from a setting, with no flash of the wrong one; onboarding on the first launch (where the
  vault goes, the files folder, the mode, the theme, update checks, off and explained); a Settings
  and Help dialog (the vault, open another or make a new one, export to a folder, the files folder,
  the mode, the theme, update checks, "Set up again", the version, the documentation and issue
  addresses as text, and _Copy diagnostic report_, which names versions, counts and the last
  failures' kinds and never a path or a title); the zero-network gate in CI (the release app under
  `strace` on Linux, blocking) and an origin check on every end-to-end test, with the by-hand
  procedure for Windows and macOS in the user guide; the usability scenario as an end-to-end test
  on the four browser projects; the user guide and the documentation site (GitHub Pages from
  `docs/`); FORMAT.md v0.1; installers for Windows (MSI, NSIS), macOS (Apple Silicon and Intel)
  and Linux (AppImage, deb, rpm) from one tag, with SHA-256 sums and a draft release, signed when
  the certificates exist and said plainly when they do not. Three new commands (`settings_get`,
  `settings_set`, `app_diagnostics`) and the dialog plugin for the folder picker. ADR-0027 to
  ADR-0029.
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
- M6 ingestion v0: importing a Markdown or Obsidian folder (through the files folder's engine,
  links, frontmatter, tags and pictures kept), CSV and Excel tables (one object per row, under a
  Dataset holding the file, with the fields' kinds inferred and misfits kept as text), PDF text
  (a heading and paragraphs per page, the title, author and page count), pictures (EXIF date and
  place, camera, a thumbnail) and any other file as an attachment; a dry-run preview that writes
  nothing, duplicates found by content hash and skipped or linked, a receipt note, and one undo
  step per 10,000 objects. Every imported object carries provenance: a new op-logged entity
  (migration 4) that undoes, replays and exports with it, shown in the Document view with the
  object's files. An Import dialog from the header, the palette or a drop on the window; files
  dropped into the files folder are now extracted too. The CLI's `import` takes several sources,
  `--into` and `--dry-run`. Hostile files (malformed PDFs, a PNG bomb, formula cells, a sheet
  named like a path, a nested zip) are contained; the PDF parser's panics are caught per page.
  Three new commands and an `import-dropped` event. New crate `dbme-ingest`, in the coverage
  gate; proptest layers and nightly fuzzing for the CSV and PDF importers; the import budgets
  printed. FORMAT.md §3.1 (migration 4), §3.4 and §5; ADR-0026 and notes on ADR-0005, ADR-0012
  and ADR-0021.
