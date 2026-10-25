# ADR-0012: Op log: payloads, hashed bytes, groups, undo and redo

- **Status:** Accepted
- **Date:** 2026-10-25
- **Milestone:** M1

## Context

ADR-0002 made an append-only, hash-chained op log the history of a vault. It left M1 to decide how
big an op is, what it carries, exactly which bytes are hashed, and how undo and redo work. Those
choices are part of the vault format (FORMAT.md §5), so a reader in another language must be able
to check the chain and replay the log without our code. The log will also feed sync (M13) and
co-editing (M15). Whatever it records must survive several devices and several people editing the
same vault.

## Decision

- **One op changes one row.** Its kind is `<entity>.<action>` (`object.move`, `relation.trash`).
  There are three payload shapes. `create` carries the whole new row, `delete` the whole old row,
  and update-shaped ops (`update`, `move`, `trash`, `restore`) carry `{"from", "to"}` for each
  changed column. Object field values are diffed per key. Each kind lists the columns it may change,
  so a `title` change can never hide inside a `move`.
- **Every op states what it expects.** An op applies only if the row is as its payload says: absent
  for `create`, equal for `delete`, matching every `from` for an update. One engine applies every
  op (`dbme-store`'s `rows::apply` calling `dbme_core::Op::apply`). It then appends the op to the
  log in the same SQLite `IMMEDIATE` transaction. Services never write tables any other way.
- **The hash covers the stored text, length-prefixed, under a domain tag.** BLAKE3 hashes
  `dbme-op-1\0`, then `prev_hash`, the IDs, `ts`, `kind`, `target_id` and `payload_json`. Each text
  value is NULL-tagged and length-prefixed (FORMAT.md §5.4). `seq` is local and is not hashed;
  `signature` (M15) signs the hash.
- **One committed transaction is one undo step.** All its ops share a `group_id`. Undo appends the
  inverse ops, newest first, as a new group with `undo_of` set to the reversed group. Redo is the
  undo of an undo. Both are ordinary ops in the chain, so history only grows.
- **An undo never overwrites a later change.** Before undoing, the store checks that every row is
  still as the step left it (`Op::check_still_current`, ignoring `updated_at`). If not, it refuses
  with `UndoBlocked`, writes nothing and drops the step. The same holds when the inverse would break
  a foreign key added since.
- **Undo history is per session, in memory**, capped at 10,000 steps. It is not written to the
  vault. Creating a vault seeds the system types as ops, then clears the history, so a new vault
  has nothing to undo.
- **Verification replays the log.** `Vault::verify` checks SQLite's integrity and foreign keys, then
  walks the chain. It replays every op into an in-memory database and compares all eight tables row
  for row. It also checks every blob's presence and size; a deep check also rehashes each blob.
- **Deviation from the brief's schema.** `ops` gains `group_id` and `undo_of`. A `vault_meta` table
  holds the vault's ID, creation time, creating app and format version. Ops never change it.

## Consequences

- Every change costs one log row and one hash. Measured on the development laptop (Windows 11,
  NVMe): creating 10,000 objects in one transaction takes 144 ms. A single change in its own
  transaction takes about 100 µs, mostly SQLite's commit (ADR-0015).
- The log is complete enough to rebuild the tables, and the crash and property tests rely on that.
  Random edit sequences replay to identical tables; undoing everything returns to the start.
  Killing a writer at random moments always leaves a vault that verifies.
- Concurrent editors (the files folder in M5, sync in M13) cannot have their edits silently
  reverted by an undo.
- The log grows without limit. Compaction or archiving is a later decision; it must keep the chain
  checkable.
- Row payloads repeat whole rows, so deleting a large object logs its content once more. Accepted:
  it is what makes undo-of-delete and replay exact.

## Alternatives considered

- **One op per transaction, holding a list of changes.** Fewer rows, but conflicts in sync would be
  coarse, and replay checks could not say which change failed.
- **Hashing canonical JSON (RFC 8785).** Every reader would have to canonicalise identically,
  numbers included. Hashing the stored bytes avoids that entirely.
- **Snapshot undo** (storing whole before and after states per step). Larger, and it cannot detect
  that someone else changed one row in between.
- **Persisting undo history in the vault.** Undoing yesterday's edit after a restart surprises
  people, and two devices would share one stack. History is still in the log; restoring old
  versions is a separate, explicit feature.
- **Last-writer-wins undo** (always reverse, overwriting newer edits). Simple, but it loses data.
