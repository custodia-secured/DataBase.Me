# ADR-0024: The file watcher

- **Status:** Proposed
- **Date:** 2026-11-02
- **Milestone:** M5

## Context

The files folder (ADR-0006) must notice an edit made by another program and bring it into the vault
within two seconds (brief §8). On desktop that needs a file-system watcher; mobile rescans on resume
and never builds one (ADR-0006 lines 49-50). The brief names `notify` with a debouncer. The watcher
sits behind a `Watcher` trait in `dbme-mirror`, a desktop-only crate, and the engine also offers
`rescan()`, which derives the same events from size, mtime and hash, so the watcher is an
optimisation the engine never depends on for correctness.

`notify` is licensed **CC0-1.0**, which `deny.toml` did not allow and ADR-0009 admitted for JS build
tooling only. The survey in [M5.md](../milestones/M5.md) found no maintained alternative that is not
itself a wrapper of `notify`, and a watcher of our own means FFI into three platform APIs. Gate 1a
ran `cargo deny`; gate 1e (`crates/mirror/tests/gate_watcher.rs`, 14 probes) ran the Windows leg of
the event-pattern study; the macOS and Linux legs run in CI.

## Decision

**`notify` 8.2 with `notify-debouncer-full` 0.7**, in `crates/mirror` only, never a dependency of
`dbme-core`. The 9.0 and 0.8 release candidates are not adopted.

**The licence.** CC0-1.0 is a public-domain dedication with a permissive fallback licence. It is not
OSI-approved (Creative Commons withdrew its submission in 2012, over the clause that disclaims
patent rights), which is why ADR-0009 asks for this ADR. The grant is a **per-crate exception** in
`deny.toml` (`exceptions = [{ crate = "notify", allow = ["CC0-1.0"] }]`), so CC0-1.0 stays off the
general allow-list; ADR-0009's table has the row; the crate is compiled into the desktop app only.
`notify-types`, the debouncer and every other dependency are MIT or Apache-2.0. Checked on
2026-11-02: `cargo deny check licenses bans sources advisories` pass with the exception; `notify` is
the only crate in the workspace with platform code (`windows-sys`, `inotify`, `kqueue`, `objc2-*`),
and it is never built for the mobile targets.

**One debounce, owned by the caller**, 300 ms in the gate and 300-500 ms in the app; the debouncer
only stitches rename pairs by file ID. The engine, not the debouncer, carries the interpretation.

**What the Windows leg verified** (Windows 11, NTFS, `std::fs`; binding on the engine's design):

- A quiet window's changes arrive in **one batch**; a same-folder rename is a single
  `Modify(Name(Both))` with both paths; a new subtree needs no re-arming; a file created before the
  watcher started is silent, which is what `rescan()` covers.
- **An atomic replace, ours or an editor's temp-and-rename, is `Remove` then `Create` of the final
  path**, never `Modify`, and the temp path is never reported. The echo ledger expects that pair,
  and a `Remove` never trashes while a `Create` of the same path is in the batch or the path still
  exists. The engine processes a batch as a set: `Create`s are resolved (by `id`, then path) before
  `Remove`s are applied.
- **`Modify(Any)` is the only data kind**: a truncate, an append, a rewrite with the same bytes and
  an mtime-only touch are indistinguishable without a hash, so size, mtime and hash are compared
  before any parse. Directory `Modify(Any)` events occur (a folder that receives a move) and are
  ignored.
- **A folder removal is one event with no children listed** (the debouncer's documented behaviour).
  The bulk-change guard (M5 Decision 3, `>= 50`) and the Trash count and enumerate from
  `mirror_state` rows under the removed path, never from events.
- **A move across folders is `Remove` + `Create` with no rename pair**, for a file and for a folder
  alike, and a moved folder's files are invisible. Identity comes from `id:` under the new path, and
  a directory `Create` triggers a `rescan()` of that subtree.
- **A case-only rename is not stitched** on NTFS: `Remove`, `Modify(Name(From))`,
  `Modify(Name(To))`, for `std::fs::rename`, `ren` and `move /Y` alike. It is resolved by the
  case-folded `path_key`.
- **`fs::rename` over a file another handle holds open with default sharing succeeds**; the blocked
  case (a handle without `FILE_SHARE_DELETE`) returns `ERROR_ACCESS_DENIED` (5),
  `ErrorKind::PermissionDenied`, not `ERROR_SHARING_VIOLATION` (32), and a retry after the handle
  closes succeeds. The retry path matches both codes: ten tries, then `pending`, never a drop.
- Fifty deletions in one loop arrive as fifty `Remove`s in one batch (the batch count is
  timing-dependent and not asserted).

**`PollWatcher`** stays available as a user-selectable backend for network mounts, which `notify`'s
documentation says may emit no events. If the licence exception were ever withdrawn, the desktop
would fall back to the rescan-on-resume path mobile uses, with no `PollWatcher` either, since it is
the same crate.

## Consequences

- The licence policy gains its first Rust exception, scoped to one crate and one artefact.
- The engine's rules above are tested in `echo.rs`, `moves.rs` and `guard.rs` on the in-memory file
  system, and `watcher.rs` repeats the event-pattern probes on three OSes with `continue-on-error`
  until the flake count is known.
- Unverified until CI runs the other legs: macOS (FSEvents) and Linux (inotify) event shapes,
  whether inotify's per-directory watches miss files created inside a brand-new folder (the
  `rescan()` on directory `Create` is the planned cover), cloud-synced folders, and an editor that
  writes its temp file and renames it more than one debounce window later (the engine treats a
  `Rename` whose source is an ignored temp name as a write to the destination).

## Alternatives considered

- **Periodic rescanning only, no watcher.** Correct, since `rescan()` exists anyway, but a
  two-second budget on a 10,000-file folder means hashing or stat-ing every file every second:
  battery and disk cost for nothing. It remains the fallback.
- **A watcher of our own.** FFI into `ReadDirectoryChangesW`, FSEvents and inotify, in a workspace
  that forbids `unsafe_code`; three platforms of edge cases `notify` has spent a decade on.
- **`hotwatch`, `watchexec`.** Wrappers of `notify`; they inherit the licence question and add
  weight.
- **`notify-debouncer-mini`.** Debounces but does not stitch renames, which rename-equals-rename
  (ADR-0006 line 30) needs.

## Later changes

These notes record what the build found under this decision; the decision itself stands.

- **M5 (2026-11-02):** the rules above are implemented in `NotifyWatcher` (`watcher.rs`), which
  turns debounced events into the engine's `FileEvent`s and is excluded from the coverage gate
  since `rescan()` covers its absence (M5 Decision 24). The tests named in Consequences landed
  under other names: `echo.rs` as said; the guard, moves and trash in `inbound.rs` and `zone.rs`
  on the in-memory file system; the event-pattern probes in `gate_watcher.rs` and `watcher.rs`.
  The probes assert what Windows reports and are `ignore`d elsewhere; CI's `watcher-probes` job
  runs them on Ubuntu, Windows and macOS with `--include-ignored` and `continue-on-error`, and
  uploads what they print as the record for the other two legs, to be read into this note at the
  close-out. The engine does not depend on the atomic-replace pair being `Remove` then `Create`:
  a batch is reduced to a set first, so any order of the same events reads the same. The budget
  with the real watcher, release, on the reference machine: a file edit reaches the app in 360 to
  370 ms with 10,000 pages, 300 ms of it the debounce.
- **M5 close-out (2026-11-03):** the other two legs ran in CI (run 41575339235). **Linux**
  (inotify): a new file is `Create(File)` between `Access(Open)` and `Access(Close(Write))`, an
  in-place write `Modify(Data(Any))` between the same two, a removal `Remove(File)` or
  `Remove(Folder)`; a same-folder rename, a case-only rename and a move across folders are each
  one `Modify(Name(Both))` with both paths, so inotify stitches what NTFS does not; a new folder is
  `Create(Folder)` and its files go unreported, which the engine's `DirCreated` listing covers.
  **macOS** (FSEvents): a change's batch often also carries a `Create(Folder)` of the root; a new
  file, a rename and an atomic replace arrive as `Modify(Name(Any))` or `Create(File)` without a
  pair; one removal came as `Modify(Data(Content))` and one not at all; the temp file of an atomic
  write is reported. On both, fifty deletes arrive once each and the folder removal is one event.
  The engine relies on none of the kind names: a batch is reduced to a set of changed and removed
  paths, and `rescan()` derives what a watcher missed. The file→app budget passed through this
  watcher on both systems (358 ms on Ubuntu, 342 ms on macOS, 300 ms of it the debounce).
  Since the record was read (2026-11-03), the probes are no longer `ignore`d off Windows: every
  shape goes through `expect`, which asserts on Windows and prints "as on Windows" or "differs
  from Windows" elsewhere, so the `watcher-probes` job records on Linux and macOS instead of
  failing by design, and `--include-ignored` is gone. The job stays advisory for the runners'
  timing alone; the engine's `watcher.rs` asserts on all three systems in the desktop jobs.
