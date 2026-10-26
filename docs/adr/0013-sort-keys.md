# ADR-0013: Sort keys: the fractional-index algorithm

- **Status:** Accepted
- **Date:** 2026-10-25
- **Milestone:** M1

## Context

ADR-0003 chose fractional index sort keys for ordering siblings: strings that sort by bytes, where
a key can always be made between two others. It left the alphabet and algorithm to M1. The keys
are stored in the vault and replayed from the op log. So the algorithm must be deterministic and
written down, and other implementations should be able to produce the same keys.

## Decision

- **Algorithm:** David Greenspan's "Implementing Fractional Indexing", as in the CC0
  `fractional-indexing` library. It uses 62 digits `0-9A-Za-z`. A key is a length-tagged integer
  part plus an optional fraction that never ends in `0` (FORMAT.md §2.1). The first key is `a0`.
  Appending and prepending change only the integer part, so a list built by appending keeps keys of
  two or three characters.
- **Implementation:** `dbme_core::SortKey`, pure Rust with no dependency. Its known-answer test uses
  the reference implementation's published cases, and property tests check ordering and
  validation. `SortKey::n_between` makes `n` evenly spread keys at once, for bulk inserts and
  rebalancing.
- **Ties:** equal keys are allowed, for example after two devices insert at the same place.
  Siblings sort by key, then by ID. No random jitter is added to keys for now.
- **Rebalancing:** `Txn::rebalance_objects(parent)` and `Txn::rebalance_blocks(object, parent)`
  give one parent's live children fresh, short keys in their current order. They emit ordinary
  `move` ops, so rebalancing is one undoable step and needs no op kind of its own. Nothing runs it
  automatically yet; the UI milestones decide when.

## Consequences

- Inserting repeatedly at one spot lengthens keys by about one character every five inserts:
  12 characters after 60 inserts after the same sibling, measured in the store's tests.
  Rebalancing brings them back to two or three characters.
- Readers only compare strings; they never need the algorithm.
- Without jitter, two devices that insert between the same pair produce the same key. The ID
  tie-break keeps the order stable and identical everywhere. Sync (M13) may add jitter if
  interleaving matters there.

## Alternatives considered

- **LexoRank.** It works, but its buckets and background rebalancing protocol are more machinery
  than we need.
- **Integer positions.** Every insert renumbers siblings, which conflicts in sync (ADR-0003).
- **Floating-point positions.** Repeated halving runs out of precision after about 50 inserts at
  one spot.
- **A smaller alphabet (base 10 or 36).** Keys grow faster. Base 62 is the largest alphabet of
  plain ASCII letters and digits, which keeps keys safe in JSON, file names and URLs.

## Later changes

These notes record what later milestones did under this decision; the decision itself stands.

- **M2 (2026-10-25):** rebalancing now runs on its own. When a move, a create or an editor flush
  leaves a sort key longer than 16 characters, `dbme-api` rebalances that parent's children in the
  same transaction, so the same undo step (`LONG_SORT_KEY`, ADR-0016).
