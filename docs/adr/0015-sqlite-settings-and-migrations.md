# ADR-0015: SQLite settings, migration policy and the performance budget

- **Status:** Accepted
- **Date:** 2026-10-25
- **Milestone:** M1

## Context

ADR-0002 put the canonical store in SQLite. M1 has to fix how a vault is opened and upgraded and
how durable each commit is. It also has to read the M1 budget, "10k-object CRUD + undo < 2 s"
(build plan, M1; ADR-0008), against what SQLite can do on the reference hardware.

## Decision

- **Connection settings** on every open: WAL journal, `synchronous = NORMAL`, foreign keys on, a
  5 s busy timeout, a 32 MiB page cache (so a bulk transaction is not spilled to the WAL before it
  commits; 64 MiB measured no faster), and a prepared-statement cache of 128 (the store uses a few
  dozen distinct statements). The bundled SQLite is used everywhere, so every platform runs the
  same version.
- **Durability.** In WAL mode with `synchronous = NORMAL`, a power cut can lose the last few
  committed transactions, but it cannot corrupt the database. A process crash loses nothing that
  was committed. The crash test kills a writer at random moments ten times and requires a clean deep
  verification each time.
- **Identification.** `application_id` is `0x44424D45` (`DBME`). An empty database becomes a
  vault. A database with another application ID, or with tables but no ID, is refused as
  `NotAVault`.
- **Migrations.** Migrations are SQL files compiled into the binary and numbered from 1, and
  `user_version` holds the last one applied. Each runs in its own transaction together with its
  `user_version` update, so an interrupted upgrade leaves the previous version intact. A vault
  whose `user_version` is newer than the build is refused (`TooNew`) and never written. Every
  migration updates FORMAT.md, and released migrations are never edited. Tables a milestone does
  not use arrive with that milestone's migration.
- **One writer per vault.** The app holds an OS lock on `vault.lock` (`File::try_lock`) for as long
  as the vault is open. A second opener gets `Locked`. The lock dies with the process, so a crash
  never leaves a vault stuck.
- **Indexes on `ops`.** Every change writes to `ops`, so every index there costs every commit.
  `seq` is the rowid, without `AUTOINCREMENT`. `op_id` is `UNIQUE` (needed by sync). `hash` has no
  index, since the chain already makes it unique, and there are no indexes on `group_id` or
  `target_id`: nothing in M1 looks ops up that way. The milestones that need those lookups add the
  indexes. Indexes are not part of the format (FORMAT.md §3).
- **Query shapes on the paths every change takes.** Each SQLite statement has a fixed cost per
  run, and on these paths it is paid once or more per object. Measured per call on the
  development laptop, for a lookup that finds nothing (the usual case): a plain indexed lookup
  takes 1.9 µs, the same lookup with an `ORDER BY` 4.8 to 5.8 µs (even when an index gives the
  order), a recursive subtree query 3.6 µs, and the attachment lookup written as a join 9.6 µs.
  So:
  - An `UPDATE` sets only the columns that changed. Setting an indexed column, even to its old
    value, makes SQLite rewrite that index entry. With every column in the `SET`, undoing 10,000
    trashings took 358 ms; with only the changed columns, 136 ms.
  - Subtrees of objects and blocks, and an object's block tree, are walked in Rust with one
    indexed lookup per row. Short per-object lists (relations, attachments, types) are sorted in
    Rust. The order is exactly the one the SQL `ORDER BY` gave; a test compares each read with its
    SQL definition.
  - An attachment's blob size comes from a subquery rather than a join. Every attachment has its
    blob (a foreign key), so the rows are the same.
  - Undo applies each inverse op to the row it has just read, instead of reading it a second time.
    Debug builds, and so the tests, read it again and check that it matches.

  Type inheritance still uses a recursive query. It runs only when an object's field values are
  checked.

- **How the 2 s budget is read.** The budget is measured as four bulk actions on 10,000 objects:
  create, edit, delete, then undo each of the three. Each action is one transaction, the way the
  app applies an import, a paste or a multi-selection. The release-mode test
  `ten_thousand_objects_are_created_edited_deleted_and_undone_within_budget` gates it in CI. A
  second test, not gated, makes every change its own transaction and prints the total for the
  record.

## Consequences

- Measured on the development laptop (Windows 11, NVMe, release build), the gated run takes
  1.05 to 1.08 s over six runs: create about 145 ms, edit 138 ms, delete 261 ms, and undoing the
  three 516 ms. Before the query changes above it took 1.59 to 1.64 s, and the first CI run, on
  `ubuntu-latest`, took 2.60 s and failed the gate: create 239 ms, edit 232 ms, delete 1,207 ms,
  undo 926 ms. CI runs the gated test and the measurement separately, since running them together
  slowed the gated run to 1.88 s on the laptop.
- The ungated run, 40,000 commits of one change each, takes 3.97 s: 99 µs per commit (4.59 s and
  115 µs before). The UI applies each user action as one transaction, and a person does not make
  40,000 separate edits in two seconds, but this figure is a deviation from a literal reading of
  the budget and is reported as one.
- The CI runner was 1.3 to 2.1 times slower than the laptop, phase by phase. At that ratio the
  gated run lands between about 1.4 and 1.8 s on CI; the next CI run measures it. The reference
  machine (ADR-0008) has not been measured yet. What remains is spread evenly: about 13 µs per
  op, of which 2 to 6 µs is building, checking and hashing the op and the rest is SQL. Dropping
  the gate is not the answer.
- Turning on `synchronous = FULL` later costs one more fsync per commit. That trade-off is
  revisited if users report losing their last edits after power loss.

## Alternatives considered

- **`synchronous = FULL`.** Durable against power loss, but every commit pays an extra fsync. The
  vault is not corrupted either way, and the op log shows exactly what survived.
- **Rollback journal instead of WAL.** Readers would block writers. WAL lets the UI read while an
  import writes.
- **A migration framework crate** (refinery, sqlx migrations). More dependencies for what one
  short function and numbered files already do. Those tools also keep their own bookkeeping table,
  where `user_version` is visible to any SQLite tool.
- **Deferring WAL checkpoints** (`wal_autocheckpoint = 0`, checkpointing when idle). It saved
  about 25 ms per bulk action, but it moves the work rather than removing it, and a vault left
  unchecked grows its WAL file without bound.
- **Handing every service's row to the op instead of reading it again,** as undo does. It saved
  3% of the gated run, at the cost of every service having to guarantee that the row it hands
  over is still the row as it is now.
- **Indexing `ops` by group, target and hash now.** Each index adds work to every commit, for
  lookups nothing uses yet. Dropping them was one of the changes that took the per-change run from
  15.2 s to 4.58 s.
