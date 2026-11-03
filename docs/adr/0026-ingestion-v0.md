# ADR-0026: Ingestion v0: the importers, the run and provenance

- **Status:** Proposed
- **Date:** 2026-11-03
- **Milestone:** M6

## Context

The brief (§3, §4, §8, §10) asks for a first set of importers in M6: a Markdown or Obsidian
folder, CSV and XLSX with rows becoming objects, PDF text, images with EXIF and thumbnails, and any
file dropped on the window; import runs with a dry-run preview, a dedupe report and undo; a
`provenance` row on every imported object; and hostile files handled safely, with fuzzing for the
importers. The build plan (§2.5) names the `crates/ingest` crate and the importer contract.
Sidecars (Docling, whisper.cpp, OCR) are M12 and nothing here may depend on one.

M5 left two things for this milestone: the files folder's drop zone attached a dropped file to a
plain object and extracted nothing (ADR-0021, "no extraction"), and the folder's inbound path
wrote no provenance row, since the folder itself is the provenance in the hash chain (M5
Decision 1). The crates were verified at gate 1 before any code depended on them; the record is
in `docs/milestones/M6.md` ("Gate results") and the probes are tests in `crates/ingest/tests/`.

## Decision

**One crate, `dbme-ingest`, portable by construction.** It depends on `dbme-core`, `dbme-store`
and `dbme-mirror` and on pure-Rust parsers only: `csv` 1.4, `calamine` 0.38, `pdf-extract` 0.12
(over `lopdf` 0.42), `kamadak-exif` 0.6, `image` 0.25 with the JPEG, PNG, GIF and WebP decoders
and the JPEG encoder, and `infer` 0.22. `cargo tree` shows no C in it. It is not in the mobile CI
job until M14 needs it there.

**The contract.** `Importer { id(), accepts(&Sniffed) -> bool, plan(&Source, &Ctx) ->
Result<Extract> }`. A `Sniffed` is a file's name, extension, size and the MIME `infer` reads from
its first bytes; a `Source` is a canonical path to a file under 256 MiB or to a folder; a `Ctx`
is the vault, read only, and the run's options. An `Extract` is what the importer would make:
objects (title, type by name, properties, blocks, attachments by role, children), the types those
objects need (`TypeDraft`, created or completed by the apply), a folder for the engine, warnings,
and one `ProvenanceDraft` per object. The importers of v0, asked in this order: `markdown`,
`csv`, `xlsx`, `pdf`, `image`, and `file`, the generic one, which accepts everything and attaches
the original to a `Document` (a `Media` for `image/*`, `audio/*` and `video/*`).

**A run is a plan, then an apply.** `plan(vault, sources, options, importers) -> ImportPlan`
reads every source, runs the importer that accepts it and writes nothing: no blob is stored and no
transaction opened, so a dry run leaves no trace (Decision 7 in the milestone). The plan carries,
per source, the importer, the extract and the object count; and for the run, the duplicates, the
refused paths, every warning, the object count and how many transactions the apply will take.
`apply(vault, plan) -> ImportRun` stores the blobs and writes the objects in one transaction per
10,000 objects, splitting at whole sources; each transaction is one undo step. The run's ULID is
the plan's `run_id`; every provenance row names it, and there is no `import_runs` table: a run is
recoverable from `provenance` by its id and from the op log by its groups.

**Where imports land.** Under the parent the caller names, or under a new top-level Note titled
"Import <date>" whose body is the receipt (the counts, the duplicates, the warnings). No `Import`
system type.

**Dedupe by content hash.** A source whose hash is attached as `Original` to a live object is a
duplicate: listed with that object and skipped, or, under the `attach` policy, that object is
related under the import's parent instead. Within one run two identical files are one object. A
Markdown file is also deduplicated by its `id:` (M5 Decision 13). Rows of a table are not
deduplicated against objects in M6.

**Provenance is an op-logged entity.** Migration 4 adds the `provenance` table with the brief's
columns (`id`, `object_id`, `source_kind`, `source_uri`, `import_run_id`, `original_blob`,
`extractor`, `extractor_version`, `confidence`, `accepted`, `created_at`); FORMAT.md §3.4 and §5
describe it. The entity takes `create` and `delete` only, like `attachment`: a row is immutable
and `accepted` is set when it is written. Its `target_id` is the `object_id`, so an object's
history and change events name the object. `confidence` is validated to 0..=1 (serde_json writes
NaN as `null`, which would never replay). `source_uri` and `import_run_id` are optional, for the
`manual` and `clip` rows of later milestones, and `original_blob` is not a foreign key to `blobs`
(a blob's row exists only once an attachment carries its size); the service checks the blob store
instead. So a run's provenance rows undo with the run, replay in `verify`, travel in `ops.jsonl`
and will sync. The files folder's inbound path still writes none (the folder is the provenance);
the drop zone's extraction does, as `import:<importer>`.

**The importers.** A table (CSV, or each sheet of a workbook) lands as a `Dataset` object
holding the file, with one object per row under it, typed by a type named from the file or the
sheet; an existing type of that name is reused and its fields' kinds rule; a field per column,
its kind inferred (boolean, number, date, datetime, URL, email, else text) when at most one cell
in ten, and never fewer than one, does not fit it; a date column with a time in it is a datetime
column; the title comes from the first column unless an option names another, and is not a
field; a cell that does not fit is kept as text under `props_json._import` and reported. PDF
text goes into paragraph blocks under a heading per page, with the page count, title and author
as fields of `Document`, inside a 10 s budget checked between pages; a file over 64 MiB or one the
parser refuses is attached without its text and with a warning. An image gives its EXIF date to
`captured_at`, its GPS to `geo_lat` and `geo_lon`, camera and orientation to fields of `Media`,
and a 256-pixel JPEG thumbnail attached as `Thumbnail`, never scaled up; the guard is the
importer's own pixel count (64 megapixels, read from the header) with `image::Limits` beside it,
since 16,384² grey is exactly the 256 MiB allocation limit. A Markdown or Obsidian folder is
imported by the files folder's engine in an import mode (`Mirror::import_from`): the bookkeeping
goes to scratch rows dropped at the end, every write is an ordinary `transact` (so one undo step
per file, the one exception to the run as a transaction), what would have been an inbox entry is
a warning on the receipt, a file whose `id:` the vault already has is taken as it is, and nothing
is written back to the folder. A file in such a folder that is not Markdown becomes the engine's
plain object and is handed to the matching importer (`folders::enrich_object`), which is also how
the drop zone's handoff works: the worker's inbound pass enriches every object the engine made
from a file it keeps as an attachment. Archives are plain files: nothing is unpacked, so a zip
bomb is a blob of its compressed size.

**Caps and safety.** A source over 256 MiB is refused before it is read; CSV cells are cut at
1 MiB and rows at 1,000,000, the rest counted; a column header that is no field key is slugged. A
cell a spreadsheet would run as a formula (`=`, `@`, or a `+` or `-` that starts no number) is
kept as text and counted in a warning; nothing is evaluated. No importer reads anything but its
source or writes anything but the vault; a folder import never follows a link out of its folder.
The PDF parser's panics are contained (`catch_unwind` around the load and around each page),
since the proptest layer found `pdf-extract` panicking on mutated files and one bad file must not
abort a run; the crate is pure Rust, so a panic is a bug, not corruption, and nothing of the
document is kept after one. The importers are fuzzed through `dbme_ingest::probe`, the CSV and
PDF paths on bytes in memory: proptest in every CI run, cargo-fuzz nightly beside the parser's
target.

**The API and the UI.** `import_plan(sources, options)`, `import_apply(plan)` and
`provenance_list(object_id)` in `dbme-api`; an apply reaches the UI as one `vault-changed`
spanning every step it took. The desktop shell turns the window's drop event into
`import-dropped` with the paths and reads nothing itself; the Import dialog takes those paths or
typed ones (the picker is M7), shows the plan, applies it and shows the receipt with Undo; the
Document view shows an object's origin ("Imported from <file> on <date> by the <importer>
importer") and the files it carries. The CLI's `dbme import <sources…> <vault>` takes `--into`
and `--dry-run`.

## Consequences

- Every imported object says where it came from, in a row that undoes, replays and exports with
  it; a run can be found again by its id.
- A dry run is free of side effects, so the dialog can preview anything, however large.
- The engine's import mode means Obsidian folders get M5's full reading (links, frontmatter,
  pictures, tags) with no second parser, at the price of one undo step per file.
- The caps are generous for a person's own files and tight enough that a hostile file costs
  bounded time and memory; the security tests and the nightly fuzzers hold that line.
- `catch_unwind` in the PDF importer is a deliberate exception to the rule that a panic in a
  dependency is a finding to fix or a crate to replace: the finding is recorded, the crate stays
  (its fallback, `pdf` 0.10, has not been needed), and the panic costs a page or a file's text.
- The informational budgets (10,000 CSV rows under 5 s, a 100-page PDF under 2 s, a 12-megapixel
  thumbnail under 500 ms, release) are printed by `crates/ingest/tests/budget.rs` on Ubuntu and
  gate nothing; the brief names no import budget.

## Alternatives considered

- **A side table for provenance, outside the op log.** Simpler to write, but a run's rows would
  not undo with the run, would be missing from `ops.jsonl` and could not sync.
- **An `import_runs` table.** The run's id in every row and its op groups already recover it; a
  table would be one more thing to migrate and export.
- **A second Markdown parser for imports.** The engine already reads Obsidian correctly; the
  import mode reuses it and keeps one reading of the format.
- **Unpacking archives.** A later importer with its own caps; in v0 nothing is unpacked, which is
  also what makes a zip bomb harmless.
- **A watchdog thread for PDF extraction.** The budget is checked between pages on the caller's
  thread; a page that takes long is bounded by the file's size and the parser, and a thread that
  cannot be stopped would not help.
- **Replacing `pdf-extract` over its panics.** The containment costs one page; the fallback crate
  was not shown to be safer at gate 1 and would be another reading of PDF to verify.
