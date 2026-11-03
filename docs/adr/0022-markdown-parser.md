# ADR-0022: The Markdown parser for the files folder

- **Status:** Proposed
- **Date:** 2026-11-02
- **Milestone:** M5

## Context

The files folder (ADR-0006) writes every object as a Markdown file and reads edits back. Blocks are
stored as `content_json` (ADR-0016), so the round trip is normalising: the serializer renders a
block, the parser converts Markdown back into block content. The parser therefore has to do four
things well: find the frontmatter block, split the body into blocks whose text can be sliced from
the source, read a trailing `^id` anchor, and extract wikilinks, embeds and tags. It runs in
`dbme-core`, which cross-compiles for iOS and Android, so it must be pure Rust with no C or platform
code, and it must survive hostile input (brief §10).

ADR-0006 names `pulldown-cmark` or `comrak`. The survey in [M5.md](../milestones/M5.md) read both
and `markdown-rs`; gate 1b (`crates/core/tests/gate_markdown.rs`, 13 probes) ran `pulldown-cmark`
0.13.6 against every shape the format needs, and gate 1h fetched Obsidian's documented rules for
links, block identifiers, properties, embeds and tags (`fixtures/obsidian-gate/README.md`).

## Decision

**`pulldown-cmark` 0.13 with `default-features = false`**, a dependency of `dbme-core`. Options:
`ENABLE_WIKILINKS`, `ENABLE_YAML_STYLE_METADATA_BLOCKS`, `ENABLE_TABLES`, `ENABLE_TASKLISTS` and
`ENABLE_STRIKETHROUGH` (without it `~~x~~` is plain text). Every parse goes through
`Parser::new_ext(..).into_offset_iter()`, so each event carries its byte range.

Verified by gate 1b on 2026-11-02:

- `![[Title]]` and `![[attachments/x.png]]` are `Tag::Image` with `LinkType::WikiLink`, the range
  starting at the `!`; `[[a/b|shown]]` and `[[a#^id]]` are `Tag::Link` with the raw target in
  `dest_url` (`#^id` kept), `has_pothole` for the piped form and the shown text as the inner `Text`.
  Embeds, aliases and block links therefore need no fallback parser (M5 Decisions 9 and 10).
- Ranges nest: every event lies inside its enclosing `Start`, `End` carries the same range as
  `Start`, siblings do not overlap, so block text is sliced from the source by range. A paragraph's
  range includes its trailing newline; a fence's and a metadata block's do not.
- A trailing anchor sits in the block's **last `Text` event** (after inline code it is its own
  `Text`). `#tag` is plain text everywhere but at line start. Both get a scanner of ours.
- Two-space indentation nests lists three deep; `- [ ]` gives `TaskListMarker`; `3.` gives
  `List(Some(3))`; a four-backtick fence holds `---` and three-backtick lines as one `Text`.
- 1 MB of `[[`, 10,000 nested `>` and a 100 kB line parse without panic in under 200 ms (debug); the
  cost of a hostile file is its event count, not its time.

Five rules follow from the probes and from Obsidian's documentation, and they bind the file shape:

1. **Frontmatter comes only from a `MetadataBlock` whose range starts at offset 0**, sliced from the
   block's range, never concatenated from `Text` events (CRLF splits them across lines). The body is
   then parsed again with the metadata option **off**: the metadata-block scan fires at any indent-0
   block start, so a `---` followed by a non-blank line and any later `---` would swallow the blocks
   between them as YAML. `---` followed by a blank line is a rule plus a setext heading, losing the
   frontmatter silently; the inbox's `missing_id` path expects that.
2. **Dividers are written `***`, never `---`**, for the same reason, and because `para\n---` is a
   setext h2.
3. **An anchor on its own line is surrounded by blank lines.** Directly after a table it is read as
   a one-cell table row; after a list item or quote line it is a lazy continuation inside that
   block. Obsidian's documentation puts a list's, quotation's, callout's or table's identifier "on a
   separate line, with a blank line before and after", a paragraph's at the end of its line, and a
   single list item's on the item's line. Headings and fenced code are not mentioned; whether a lone
   `^id` after them attaches is settled by the recorded opening (gate 1h, items 2, 7, 8), and until
   then the serializer writes it that way. A lone `^id` line attaches only to a preceding block
   whose anchor belongs on its own line and that has none yet, so Decision 11's empty paragraph (an
   anchor alone after an anchored paragraph) is not ambiguous.
4. **List nesting indents by the marker width**, not by two spaces: CommonMark requires a nested
   item to reach the parent's content column (three spaces under `1. `, four under `10. `), and
   Obsidian follows CommonMark here. Two spaces suffice under `- ` only.
5. **Tags are case-insensitive** (documented), so `Tag` objects are matched by case-folded title; a
   nested tag carries `/`, which FORMAT.md §6.1 strips from file names, so the title and the file
   name of such a `Tag` differ.

What the documentation does not state, and only the opening settles: how a bare `[[Name]]` resolves
when two files share the basename, whether resolution is case-insensitive, and whether a quote's
identifier on its last line resolves. The link resolver in `obsidian.rs` is therefore **our rule**,
written down here once the opening is recorded, not Obsidian's.

## Consequences

- No AST-to-Markdown serializer: writing is string building from `content_json`, and the parser is
  the only place Markdown grammar lives. Fidelity for someone else's formatting is not promised; the
  golden folder shows what changes.
- The frontmatter and the body are two parses with different options. A `dbme` fence, a `---` pair
  and a lone anchor line are the three shapes the serializer must never produce by accident; the
  fixpoint test (`serialize(parse(serialize(o))) == serialize(o)`) catches a regression.
- Size and depth caps are ours, applied before the parser sees the bytes: the crate bounds time, not
  memory.
- `ENABLE_YAML_STYLE_METADATA_BLOCKS` also accepts `...` as a closing fence; the serializer writes
  `---` only.

## Alternatives considered

- **`comrak` 0.59 (BSD-2-Clause).** Full GFM, an AST and `format_commonmark`, wikilinks and front
  matter as its own extensions. Its default features pull a C library through `syntect-onig`, which
  would break the mobile build of core; positions are line and column, not byte offsets; and the
  normalising serializer is not needed when the body is rendered from `content_json`. Kept as the
  fallback only if a gate had failed; none did.
- **`markdown` 1.0.0 (MIT).** The cleanest safety story, positions on every node, but no wikilinks
  and no release in eighteen months.
- **A parser of our own.** The format is a subset, but the subset still includes lists, quotes,
  fences, tables and inline marks, and the fuzzing requirement (brief §8) is far cheaper to meet on
  a parser that already has a fuzz corpus.

## Later changes

These notes record what the build found under this decision; the decision itself stands.

- **M5 (2026-11-02):** gate 1h's opening settled the open shapes: a quote's identifier goes on its
  own line, as for headings and fences; the serializer writes every anchor that way and the
  fixtures were corrected to it. The link resolver's rule for a bare name shared by two files is
  ours (`obsidian.rs`), as this record expected. Links inside a table stay text with the table,
  which the format stores verbatim; a picture Obsidian named by path is not an app attachment (those
  are named by hash) until M6 imports foreign files, so its embed stays text and Obsidian still
  shows it. The parser is fuzzed in two layers: `files_fuzz.rs` (proptest, every run) and the
  `fuzz/` cargo-fuzz crate, nightly (`.github/workflows/fuzz.yml`).
