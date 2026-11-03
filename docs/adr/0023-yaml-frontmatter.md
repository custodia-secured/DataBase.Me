# ADR-0023: The YAML crate for frontmatter

- **Status:** Proposed
- **Date:** 2026-11-02
- **Milestone:** M5

## Context

Every file in the files folder opens with a YAML frontmatter block (FORMAT.md §6.2): a fixed
envelope (`id`, `types`, `tags`, `created`, `updated`, `relations`, `links`, `sort`) around user
fields whose values are typed by the field kind, plus whatever keys Obsidian's property editor adds.
The crate that reads and writes it must keep key order (so a rewritten file does not churn in git or
a cloud drive), keep unknown keys, read `due: 2026-11-30` and `answer: no` back as the strings they
were written as, report a position for every error (the Conflicts inbox shows it), emit readable
block scalars, and be pure Rust, since it lives in `dbme-core`. ADR-0006 rules out `serde_yaml`
(unmaintained) and names `serde_yaml_ng`, `serde-saphyr` and `saphyr`.

The survey in [M5.md](../milestones/M5.md) read the four candidates; gate 1c
(`crates/core/tests/gate_yaml.rs`, 12 probes) ran `saphyr` 0.3.3 against the shapes above, and its
verifier read the crate's source for every headline claim.

## Decision

**`saphyr` 0.3** in `dbme-core`, no optional features; `YamlEmitter` with `multiline(true)` and
`quoted_value(false)`; the mapping between the `Yaml` tree and our structs written by hand (it is
also the fuzz surface).

Verified by gate 1c on 2026-11-02, with the source lines its verifier read:

- **YAML 1.2 Core Schema.** `no`, `on`, `yes`, `off`, dates and times load as strings
  (`scalar.rs:181-231`: only `null`, `true`/`false`, integers, floats, `.inf`/`.nan` resolve). The
  Norway problem does not arise. Two consequences for kind-driven decoding: `0755` is `Integer(755)`
  (decimal, not octal) and `1e3` is `Real`; `1_000` is a string. A hand-written unquoted scalar that
  resolved to a number is decoded by its field kind from the **raw spelling**, recovered by slicing
  the source at the `MarkedYaml` span's `index()` range, so `early_parse(false)` and a direct
  `saphyr-parser` dependency are not needed.
- **Mapping order is preserved** through load and emit (`LinkedHashMap`).
- **A duplicate key is a `ScanError`** at the second key's position (`loader.rs:429-430`), routed to
  the inbox as `parse_error`. The last-wins opt-out needs `saphyr_parser::Parser`, which `saphyr`
  does not re-export; it is not taken.
- **Positions.** `MarkedYaml` carries a `Span` of start and end `Marker`s (`index`, 1-based `line`,
  0-based `col`) on every node and key; errors carry a marker and a message, `Display` adds one to
  the column. A scanner error points one past the offending character.
- **The emitter quotes every string a reader could misread** (booleans, nulls, numbers, `08:21`,
  `1e3`, the empty string, leading space, anything containing `: # [ ] { } , -` or a newline, and
  anything starting with `& * ? | - < > = ! % @`) and leaves dates plain (`emitter.rs:518-567`);
  keys are always emitted plain through the same test (`emitter.rs:428`). The emitted tree loads
  back equal in all four option combinations, so the serializer needs **no quoting of its own**. The
  one question left is whether dates are quoted for Obsidian's Date property (gate 1h, item 13).
- **`[[Title]]` is emitted quoted**; an unquoted wikilink loads as a nested flow sequence, which the
  parser unwraps to the string (Obsidian's documentation also says link values must be quoted).
- **Every `dump` starts with `---\n` and ends without a newline** (a literal block with keep
  chomping would end in `...`); the serializer strips the leading line and writes the fences itself.
- **Flow nesting is capped at 255 levels** by the crate (`scanner.rs:1568-1575`, "recursion limit
  exceeded"); a 1 MB `[[[…` loads an error in under a millisecond.

What the crate does not provide, and `core/files` therefore does **before** `load_from_str`:

- **No size limit and no block-nesting limit.** Block nesting past a few hundred levels (900 in a
  debug build) overflows the parsing thread's stack and **aborts the process** (`parser.rs:544-608`,
  one frame per level, no counter). The pre-scan rejects a frontmatter over a size cap and over a
  nesting cap as `parse_error`, bytes untouched. Depth is counted as true block nesting: a stack of
  open block collections by column, `- ` runs on one line, and flow brackets, with tags, anchors
  and aliases stripped first; the root mapping is level 1 and 64 levels is generous for any
  Obsidian file. The fuzz test asserts the pre-scan, not the crate.
- **Exactly one document** between the fences: a stray `---` or `...` line yields a second document
  (`Null`), which is a `parse_error`; an empty block is an empty envelope (an empty string loads as
  zero documents, `---\n...\n` as `Null`).
- **`Yaml::Representation` nodes are never built for emission**: literal and folded styles hit
  `todo!()` in the emitter (`emitter.rs:305-306`).

## Consequences

- A few hundred lines of hand mapping, once, with positions for every error message; no serde
  derive, so a field kind's decoding is explicit.
- The pre-scan is part of the parser's contract: a file the pre-scan rejects never reaches the
  crate, and the release-build stack boundary (which moves) is never load-bearing.
- A rewritten file keeps the person's unknown keys (`extra`, round-tripped through
  `props_json._frontmatter`) but writes keys in the serializer's canonical order: the envelope, the
  user fields in schema order, `fields:`, `relations`, `links`, then the unknown keys. An inbound
  apply never rewrites, so the person's order survives until the app itself changes the object;
  the fixpoint then makes every later rewrite byte-stable. Obsidian's own re-serialisation after a
  property edit (block lists, quoting) is the one source of churn left, measured at gate 1h.

## Alternatives considered

- **`serde-saphyr` 2.0 (MIT OR Apache-2.0).** Serde ergonomics, duplicate keys as errors, `unsafe`
  denied, resource budgets. It would push the free-form `fields:` map through `serde_json::Value`
  (needs `preserve_order`, which the workspace does not enable), reads `y`/`on`/`off` as booleans by
  default, and rests on `granit-parser`, a single-owner crate first published six months ago. Kept
  as the fallback if hand mapping proves too heavy; its bus factor is re-checked first.
- **`serde_yaml_ng` 0.10, `serde_norway` 0.9.** Both forks of `serde_yaml`, both on the
  C-transliterated `unsafe-libyaml`, both YAML 1.1, neither released in nearly two years.
- **`serde_yml`.** Excluded without reading: a fork whose provenance was publicly disputed in 2024.
- **A YAML subset parser of our own.** The envelope is small, but user fields and Obsidian's
  properties are arbitrary YAML, and the fuzzing requirement is cheaper to meet on a parser that
  passes the YAML test suite.
