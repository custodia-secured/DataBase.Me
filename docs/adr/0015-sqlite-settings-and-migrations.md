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
- **Recursive queries** (subtrees, type inheritance) use `CROSS JOIN` to fix the join order.
  Without it, SQLite's planner picked a slower order for the recursive step.
- **How the 2 s budget is read.** The budget is measured as four bulk actions on 10,000 objects:
  create, edit, delete, then undo each of the three. Each action is one transaction, the way the
  app applies an import, a paste or a multi-selection. The release-mode test
  `ten_thousand_objects_are_created_edited_deleted_and_undone_within_budget` gates it in CI. A
  second test, not gated, makes every change its own transaction and prints the total for the
  record.

## Consequences

- Measured on the development laptop (Windows 11, NVMe, release build), the gated run takes
  1.59 to 1.64 s: create about 137 ms, edit 156 ms, delete 570 ms, and undoing the three 740 ms.
  Run straight after the 40,000-commit measurement it took 1.88 s, so CI runs the two separately.
- The ungated run, 40,000 commits of one change each, takes 4.59 s: 115 µs per commit, nearly
  all of it SQLite's commit to disk. The UI applies each user action as one transaction, and a
  person does not make 40,000 separate edits in two seconds, but this figure is a deviation from a
  literal reading of the budget and is reported as one.
- The margin is about 20% on a fast development machine, and neither the CI runner nor the
  reference machine (ADR-0008) has been measured yet. Trashing and deleting go through about ten
  queries and two ops per object, and undo re-reads every row it reverses; those are the places
  to optimise if the gate fails there. Dropping the gate is not the answer.
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
- **Indexing `ops` by group, target and hash now.** Each index adds work to every commit, for
  lookups nothing uses yet. Dropping them was one of the changes that took the per-change run from
  15.2 s to 4.58 s.
