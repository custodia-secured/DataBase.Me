# ADR-0025: The three-way merge inside a block

- **Status:** Proposed
- **Date:** 2026-11-02
- **Milestone:** M5

## Context

When a file and the app both changed the same object since the last sync, the files folder merges
(ADR-0006 lines 33-36): a clean merge is applied, a true conflict keeps the app's version and turns
the file's into a conflict-copy object listed in the Conflicts inbox. M5 Decision 17 puts the merge
on the **model**, not the bytes: frontmatter field by field, the body per block id, and a text merge
only inside a block both sides changed. That text merge needs a diff3 that returns a decision, runs
in `dbme-core` (pure Rust) and is deterministic. ADR-0006 names `diffy` or a diff3 of our own over
`similar`.

Gate 1d (`crates/core/tests/gate_merge.rs`, 12 probes) ran `diffy` 0.5.2; its verifier read the
crate's source for the marker and hunk rules.

## Decision

**`diffy` 0.5** in `dbme-core`, called from `core/files/merge.rs` for one block's text at a time,
with `MergeOptions` set to `ConflictStyle::Diff3` (the default) and **`IncompleteHunkStyle::Git`**.

Verified by gate 1d on 2026-11-02:

- The three identities hold: `merge(b, b, x) == Ok(x)`, `merge(b, x, b) == Ok(x)`,
  `merge(b, x, x) == Ok(x)`.
- Edits on non-adjacent lines merge clean; edits on the **same or adjacent lines conflict**, as in
  git (touching changed ranges coalesce into one region). The same insertion or deletion on both
  sides merges clean; an edit against a delete conflicts.
- An empty base (first enable, M5 Decision 20b) conflicts unless both texts are byte-identical.
- `Err` carries `<<<<<<< ours`, `||||||| original`, `=======`, `>>>>>>> theirs` in `Diff3` style
  (marker length 7 by default; a label adds a space), which the inbox parses into base, ours and
  theirs per region. `merge_bytes` handles non-UTF-8.
- 10,000 lines with edits far apart merge in 9 ms (debug build); 100 repeated conflict merges are
  byte-identical.

Three rules follow, and they belong to `merge.rs`, not to the crate:

1. **Normalise before merging.** Diffy normalises nothing: a CRLF switch is a change of every line,
   and a dropped trailing newline is an edit of the last line, so either would conflict with any
   real edit of the block. Block text is normalised to LF with no trailing newline on both sides
   before the call; the serializer never writes CRLF.
2. **`IncompleteHunkStyle::Git`.** The default (GNU `diff3 -m`) glues the markers onto a last line
   that has no newline, which is every paragraph block; `Git` ends each side with a newline so the
   `Err` parses line by line.
3. **Within a short block the merge is all-or-nothing**: a paragraph is one line, so two edits to it
   conflict. That is accepted: the conflict copy (Decision 16) preserves the losing text, and the
   block-wise split keeps the conflict to the one block both sides touched. The interleavings
   generator expects a conflict for two edits on the same or touching lines of one block, and a
   clean merge only across blocks or non-adjacent lines.

## Consequences

- The merge returns a decision, which is what the inbox needs; the `Diff3` text is a by-product
  attached to the row, not a file written to disk.
- Formatting never reaches the merge, because the serializer and parser normalise first; a
  regression there shows up as spurious conflicts in `interleavings.rs`.
- There is no word- or character-level fallback: if same-paragraph conflicts prove frequent in the
  10,000-case run (gate 1f), the alternative below is the next step.

## Alternatives considered

- **A diff3 of our own over `similar` 3 (Apache-2.0) or `imara-diff` 0.2.** Structured conflict
  hunks and a possible word-level pass for paragraphs, at the cost of two to three hundred lines we
  would own and test. Not needed for the acceptance list; kept as the fallback if the conflict rate
  inside paragraphs warrants it.
- **Byte-level diff3 on the whole file.** Rejected by Decision 17: formatting differences and
  frontmatter re-serialisation would conflict constantly, and a conflict anywhere would lose the
  whole file's merge.
- **Last writer wins.** Loses content; the acceptance criterion is zero loss.

## Later changes

These notes record what the build found under this decision; the decision itself stands.

- **M5 (2026-11-02):** `merge.rs` normalises as rule 1 says and aligns `created` and `updated`
  across the three documents before merging, since the two are informational on the way in and
  would otherwise count as a conflict on every rewrite. The 10,000-case run (gate 1f) made 10,828
  conflicts in 229,030 steps, every one of them a deliberate edit of the same paragraph on both
  sides, so it says nothing about how often real editing would conflict inside one paragraph; the
  word-level fallback stays an alternative, to be judged on use, not on this run.
