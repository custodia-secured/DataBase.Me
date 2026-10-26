# ADR-0014: Typed IPC: generated bindings and the change event

- **Status:** Accepted
- **Date:** 2026-10-25
- **Milestone:** M1

## Context

The React UI talks to the Rust core only through Tauri commands and events. If the TypeScript side
declares those types by hand, they drift from the Rust side. The result is runtime errors no
compiler catches. The UI also has to learn about every change to the open vault, whether from its
own commands, an undo, or later the files folder (M5) and sync (M13), so that every view can
refresh.

## Decision

- **Bindings are generated from Rust.** Commands are annotated with `#[specta::specta]`, and
  `tauri-specta` writes `apps/desktop/src/bindings.ts`: a typed `commands` object, an `events`
  object, and every type they use. The file is checked in, so the UI builds without Rust. It is
  excluded from ESLint and Prettier.
- **CI fails on stale bindings.** The `dbme-desktop` test `bindings_are_current` regenerates the
  file and compares it with the checked-in copy. Run it with `DBME_WRITE_BINDINGS=1` to update the
  file. A second test checks that every command appears in `build.rs`, the capability file, the
  registered handler and the source, so none can be forgotten (ADR-0010).
- **Core types stay free of IPC concerns.** `dbme-core` and `dbme-store` derive `specta::Type`
  only behind an optional `specta` feature, which only the desktop crate turns on. The mobile
  builds and the CLI do not compile specta.
- **Errors are a closed set.** A command returns `Result<T, IpcError>`. `IpcError` is
  `{kind, message}`, where `kind` is one of `no_vault`, `not_found`, `rule`, `undo_blocked`,
  `locked`, `bad_path`, `too_new`, `invalid` and `storage`. The UI branches on `kind`; `message` is
  for people. The UI's `unwrap()` turns an error into a thrown `CommandError`.
- **One change event.** After every committed change, undo or redo, the shell emits
  `vault-changed` with the `Committed` record: `group_id`, `undo_of`, the `seq` range, and the
  sorted IDs of the changed objects and rows. tauri-specta names events in kebab case, so the
  plan's working name `vault://changed` became `vault-changed`. Losing an event never loses data;
  it only delays a refresh.
- **64-bit integers cross as JavaScript numbers.** Every `i64` sent over IPC is a millisecond
  timestamp or an op `seq`, far below 2^53. The bindings use
  `dangerously_cast_bigints_to_number`, so the UI gets plain numbers instead of `bigint`. A future
  `i64` that can exceed 2^53 must cross as a string.
- **Pinned release candidates.** `tauri-specta` and `specta` are release candidates
  (`=2.0.0-rc.26`) and `specta-typescript` is `=0.0.13`. All three are pinned exactly and upgraded
  together, in their own pull request.
- **The launch test exercises IPC for real.** In smoke-test mode the UI creates a vault in a
  temporary folder, creates an object, checks it is listed, undoes it, and waits for both
  `vault-changed` events, all through the generated bindings. Only then does it report ready; a
  failure exits with status 1 (ADR-0008).

## Consequences

- Renaming a Rust field or command breaks the TypeScript build, or the bindings test, before it
  can break the app.
- The main window's capability now grants the 15 app commands, plus `core:event:allow-listen` and
  `core:event:allow-unlisten` for the change event. Each grant is listed and reviewed as ADR-0010
  requires.
- The bindings test runs the desktop crate's unit tests. On Windows a test binary needs the Common
  Controls v6 manifest that tauri-build normally links only into the app. On MSVC, `build.rs`
  embeds `windows-app-manifest.xml` into every binary through the linker instead.
- Depending on release candidates carries upgrade risk. The pins and the generated-file check make
  any behaviour change visible.

## Alternatives considered

- **ts-rs.** Mature and stable, but it generates types only; the command wrappers and event
  helpers would be hand-written and could drift. It stays the fallback if tauri-specta stalls.
- **Hand-written TypeScript types.** No tooling risk, and no protection against drift.
- **One event per entity or action.** More precise, but every view would subscribe to many
  events. One event carrying the changed IDs lets each view decide whether it cares.
- **Strings for every `i64`.** Safe for any value, but timestamps would need parsing everywhere in
  the UI for no benefit at today's ranges.

## Later changes

These notes record what later milestones did under this decision; the decision itself stands.

- **M2 (2026-10-25):** the bindings moved to `packages/client/src/bindings.ts`, so every UI package
  can use them (ADR-0017). The commands themselves moved into the `dbme-api` crate; the shell's
  Tauri commands wrap them one line each, and the end-to-end test server calls the same functions
  (ADR-0018).
