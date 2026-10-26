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
