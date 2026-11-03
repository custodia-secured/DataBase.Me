# ADR-0021: The files folder engine, the export, and what they commit

- **Status:** Proposed
- **Date:** 2026-11-02
- **Milestone:** M5

## Context

ADR-0006 decided that the files folder is a replica that syncs through the op log, as a second
device would, and left the library choices and the engine's shape to M5. The crates are
ADR-0022 to ADR-0025. This record is the engine: where it lives, how an outside edit becomes ops,
what provenance those ops carry, what undo does with them, how the two directions avoid losing or
echoing each other's writes, and how the export that FORMAT.md §7 promises is made lossless. The
plan and its twenty-seven decisions are in [M5.md](../milestones/M5.md); the acceptance list is
the brief's §8, quoted there.

Forces: the engine must run on desktop only (it watches files) while the file format must run on
every platform, since a vault rebuilt from a folder is the mobile import path; the brief's zero-loss
criterion must hold under any interleaving of app and file edits; an outside edit must be an
ordinary op, tamper-evident in the hash chain; and the folder's bookkeeping must survive a crash
at any point.

## Decision

**Two crates.** `crates/core/src/files/` is the format: paths, frontmatter, body, the block-wise
merge and `schema.json`, pure and portable. `crates/mirror` (`dbme-mirror`) is the engine over
`dbme-core` and `dbme-store`, never a dependency of the shell's UI and never of `dbme-core`;
`crates/export` (`dbme-export`) is the export and import. The watcher lives in the mirror crate
behind a `Watcher` trait (ADR-0024).

**Provenance is the device, not a column.** The store opens a second `Context` beside the app's,
with the same `actor_id` and a `device_id` derived from it (`Vault::mirror_identity`). Outside
edits commit through `Vault::transact_external`, so every op they make records that device in the
hash chain, and `verify` checks it like any other; the `provenance` table waits for M6, which
needs it for imports in general.

**Outside edits are not steps of the app's undo stack.** `transact_external` leaves the undo and
redo stacks as they were, as a second device's ops would. An app step that changed a block or
object the folder then changed again becomes `UndoBlocked` when undone, and is dropped from the
stack so the steps before it stay undoable (`Vault::undo`). This is a flagged deviation from the
brief's line 145; the block-wise merge limits it to the blocks both sides touched.

**No watcher and no timer inside the engine.** `Mirror` takes `FileEvent`s from the caller and
offers `rescan()`, which derives the same events from each row's recorded size and time against
the folder, so a platform without a watcher, a missed event or a crash is covered by one call. One
debounce, owned by the caller (the API's worker, 300 to 500 ms). File I/O is one trait, `Fs`, with
`StdFs` for the disk and `MemFs` for tests, which counts writes and can fail the next write or
rename on request.

**The app side marks what to write.** `Txn::finish` records, in `mirror_dirty`, the owning object
of every op, the `src_id` of a relation, and on a title change or a move every object below the
moved ones and every object linking to them (one recursive query), since wikilinks name paths. A
type or field change sets `full_rewrite_needed` in `mirror_meta`. `pump_out` drains the table,
writes parents before children, renames before it writes, and updates `mirror_state` only after
the file is on disk.

**A write never overwrites what has not been read.** Before writing a dirty object's file,
`pump_out` compares the file's size and modification time with the row's, as `rescan` does. If
they differ, the file holds an edit the vault has not seen: the object stays dirty and is reported
as deferred; the next rescan takes the file in, the merge sees both sides, and the pass after
writes the result. A file the folder could not read is held the same way while its `parse_error`
entry is open, and is written over once the person has dismissed it. The interleavings property
found the overwrite on its first seed; with the check removed it fails there still.

**Reading in, as a batch.** Events are reduced to a set of changed and removed paths; a path both
gone and back is a change; a removal whose path still exists did not happen; a new directory lists
its files, since watchers do not report them. The bulk-change guard then counts removals and
rewrites of tracked files: `>= 50`, or more than ten percent of the tracked files for removals,
holds the whole batch as one `bulk_change_hold` entry, resolved by `Proceed` or `Restore`.
Identity resolves `id` → path → content hash among the files that disappeared in the same batch →
new; nothing is ever trashed on an inference. An unknown `id` is adopted, a trashed one restored, a
duplicate on a second live path re-issued. A file's `id` is registered by path before the batch is
applied, so links between files of one batch resolve. A deleted file trashes that object only, its
children reparented to the grandparent first. A cloud drive's conflicting copy is an inbox entry,
never an object.

**The merge is on the model.** With the base parsed from `mirror_state.base_text`, the vault's
version and the file's: frontmatter field by field, the body per block id, and `diffy` inside a
block both sides changed (ADR-0025). The app's version wins a true conflict; the file's becomes a
conflict-copy Note with the raw bytes attached, listed as `edit_edit`; the file is rewritten only
after the copy commits. A value the vault refuses is an `invalid_value` entry and the file is
written from the app, so the refused part never sits in the merge base. An inbound apply writes
nothing back unless the file needs its `id`, its anchors, or the app's winning version.

**Echo prevention is the hash.** `mirror_state.last_hash` is the hash of the bytes last written or
read; a file whose bytes hash to it is the folder's own write, noted and skipped, whatever event
reported it. There is no in-flight ledger: the hash is enough because the row is updated only after
the file is on disk, and a crash between the two leaves a stale row the next rescan resolves
without an op.

**Paths, trash and exclusion.** FORMAT.md §6.1 is normative: a taken `path_key` gets ` (2)`, names
are cut to 120 bytes and relative paths to 200. A trashed object's file moves to
`.dbme/trash/<id>.md` and returns on restore; a permanently deleted one is removed. A sensitive
object is `excluded` and has no file. Dot paths other than `.dbme/` are never read.

**The folder's location.** `files/` inside the vault by default; an override lives per vault id in
the app's config folder (`files-folders/<vault id>.txt`), not in `vault_meta`, because a machine
path inside a portable vault breaks when the vault moves.

**The worker and the commands.** `dbme-api` holds the engine beside the vault while
`mirror_meta.folder_enabled` is set, starts it on `vault_open` and `vault_create`, and runs one
worker thread: woken by the app's own commits (drain `mirror_dirty`) and by the watcher (rescan,
pump in, pump out), with the one debounce. Eight commands: `mirror_status`, `mirror_enable`,
`mirror_disable`, `mirror_sync_now` (pump out, rescan, pump in, pump out again), `conflicts_list`,
`conflict_resolve`, `mirror_hold_decide` and `export_run`; a `mirror-changed` event carries the
status and the inbox, and an inbound batch is one `vault-changed` (ADR-0014's note).

**The export.** `export(vault, dir, full)` writes FORMAT.md §7's reader files: `schema/types.json`,
`views/<id>.json` and `data/<Type>.csv`. Those are lossy by design, so the full export adds
`vault_meta.json`, `ops.jsonl` (every column, in `seq` order) and `blobs/`, and `import` is
`Vault::create_from_ops`: an empty database under the exported `vault_id`, without the built-in
seed, replaying every op as `verify` does, so the hash chain is reproduced or the import fails and
leaves nothing. `.dbme/schema.json` in the folder is the same content as `types.json`, written by
core, so a folder can be rebuilt without the database.

**`mirror-check`.** The CLI reads a closed vault's bookkeeping (counts per state, dirty objects,
open conflicts, the rewrite flag, rows naming missing objects) and, given the folder, the drift
between the two; an open vault is `StoreError::Locked`.

## Consequences

- Every outside edit is in the op log under its own device, so history, `verify` and later sync
  see it; nothing in the files folder bypasses the chain.
- The acceptance list is one test each, in `crates/mirror/tests/` and `crates/export/tests/`:
  `interleavings.rs` (zero loss under 10,000 random interleavings against a model of this record),
  `echo.rs`, `budget.rs`, `inbound.rs` and `zone.rs` (the guard, identity, moves, trash, the
  inbox's kinds, the drop zone), `obsidian.rs`, `security.rs`, `crash.rs`, `roundtrip.rs`.
- The database grows by the Markdown corpus (`base_text`), as ADR-0006 said.
- A file edit over a paragraph the app also edited since the last sync is a conflict, not a
  character-level merge (ADR-0025); the conflict copy keeps the losing text.
- Deviations carried into M5's close-out, each flagged: outside edits are off the undo stack;
  `sort:` is read but not applied inbound (a position, not a key, is needed); the `provenance`
  table waits for M6; the files folder cannot yet be limited to chosen spaces (no `spaces` table);
  the encryption warning waits for the encrypted vault (M9); the macOS budget is printed, not
  gated.

## Alternatives considered

- **The engine in the shell, in TypeScript.** It would put the merge and the file format in the
  webview, where the mobile rebuild and the CLI's import could not use them, and every file would
  cross the IPC boundary.
- **A `provenance` column on `ops`.** A column is cheap but would change the hashed bytes of every
  op (ADR-0012) for one consumer; the device is already hashed and already means "who wrote this".
- **Outside edits as undo steps.** The brief asked for it. An undo would then revert another
  person's or program's change from inside the app, and a step interleaved with the app's own
  would make the stack's preconditions fail far more often; the deviation is recorded instead.
- **A byte-level merge of the whole file.** Rejected by ADR-0025's reasoning: re-serialisation
  would conflict constantly.
- **An in-flight ledger of expected events for echo prevention.** Planned at first; the recorded
  hash covers every case the ledger would, including the crash in between, with no state to keep
  in step with the watcher.
- **Writing over an unread file and relying on the rescan to merge afterwards.** The rescan cannot
  merge what the write destroyed; the deferral costs one `stat` per dirty object.
