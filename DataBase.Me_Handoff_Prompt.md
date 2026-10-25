You are the lead engineer building "DataBase.Me", an open-source, local-first "lifetime
information repository": a mind-map on steroids. You have no prior context; this prompt is
your complete brief. Every decision below is final unless you hit a hard technical blocker,
in which case follow the deviation rule in section 11. Work milestone by milestone, verify
each, and report progress.

## 1. Vision
A cross-platform app, desktop first and then a FULL mobile app (not a capture-only
companion), where anyone from novice to power user can capture ANY information (notes, docs,
spreadsheets, PDFs, images, web clips, email, audio, video, feeds, calendars, contacts,
takeouts, location, health, code). They organize it with structure they build themselves
(hierarchies, typed objects, links, tags), find it instantly, and present it through MANY
interchangeable views of the same data: document, outline, mind map, canvas, graph, table,
kanban, calendar, timeline, gallery, map, charts, dashboards. Every object is ALSO a plain
Markdown file the user can edit in any tool, with changes flowing back into the app.

## 2. Non-negotiable principles
1. Open source. App: AGPL-3.0. Plugin SDK + file-format spec: Apache-2.0. Contributions use
   DCO sign-off (no CLA). Governance: BDFL now; GOVERNANCE.md commits to a steering group
   once there are 3+ regular maintainers.
2. No strings: no account, no telemetry, no crash reporter, no analytics, no paid tiers, no
   hosted service. Funded by donations (FUNDING.yml: GitHub Sponsors / Open Collective).
   The app makes ZERO network requests unless the user enables a feature that needs one;
   even update checks are opt-in at onboarding. Bug reports go through GitHub issues, aided
   by a "Copy diagnostic report" button (versions, OS, error codes; never user content).
3. Local-first: works fully offline; data lives on the user's disk.
4. Data ownership & longevity: canonical store is SQLite (single file) + a content-addressed
   blob folder. A TWO-WAY Markdown files folder (one .md per object) stays in sync with the
   database in both directions (section 5). JSON (schema/views) and CSV (per type) are
   export-only for now. FORMAT.md documents everything. Export -> re-import must round-trip
   losslessly.
5. AI is OPTIONAL and OFF by default. Onboarding asks the user to choose: "No AI"
   (highlighted default) / "Only AI on this computer or my own network" / "Let me choose
   providers". Universal, provider-agnostic connector (OpenAI-compatible, Ollama, llama.cpp
   server, LM Studio, Anthropic native, etc.), gated by per-space policy (none / local-only
   / any), sensitive-field redaction, and an audit log. MCP server exposes the repository to
   external agents (same policies); MCP client lets the assistant use user-approved tools.
   Target MCP spec 2026-07-28 (stateless; server/discover) with fallback negotiation to
   2025-11-25.
6. Everything customizable: user-defined types (multi-typing + inheritance), fields,
   relations, views, templates, formulas, themes, shortcuts, layouts, modes, plugins.
7. Any data type in, many views out: one object model rendered by many renderers; edits in
   any view write back to the same objects.
8. Security: sandboxed plugins (no ambient authority), optional encryption at rest, secrets
   in the OS keychain, E2EE for sync. Never silently lose user data: conflicts become
   conflict copies, deletes go to Trash.
9. Progressive disclosure: "Modes" (Simple, Organizer, Researcher, Data/Power) are presets
   of enabled features; Simple mode must feel like a plain notes app.
10. Accessibility: WCAG 2.2 AA; every visual view has a keyboard/screen-reader alternative.
11. Mobile-ready from day one: core crates must cross-compile for iOS and Android; every
    view must work at 360-px width with touch; nothing essential may depend on a
    desktop-only sidecar.

## 3. Architecture & stack (decided; record each as an ADR in docs/adr/)
- Name/identifiers: product "DataBase.Me"; repo "DataBase.Me"; dots are not allowed in
  crate/npm names, so code uses `dbme` (crates dbme-core etc., CLI `dbme`, vault folders
  `*.dbme`) and npm scope `@databaseme`.
- Shell: Tauri 2 (desktop now, iOS/Android at M14). Core: Rust workspace. UI: React +
  TypeScript + Vite, Radix UI primitives, Zustand + TanStack Query, CSS variables/design
  tokens for theming.
- Editor: TipTap/ProseMirror mapped to a block tree.
- Storage: SQLite via rusqlite (bundled) with FTS5; sqlite-vec for vectors (populated only
  when an embedding provider is enabled); optional encryption via SQLite3MultipleCiphers
  (SQLCipher-compatible) with passphrase -> Argon2id -> vault key. Blobs are stored at
  blobs/<2>/<2>/<blake3> and encrypted per-file with XChaCha20-Poly1305 when encryption is
  on.
- IDs: ULID. Ordering: fractional index sort keys. Hash strings prefixed with algorithm
  ("blake3:...").
- History: append-only `ops` table (hash-chained) = history, undo, provenance, sync. The
  files folder is treated as just another replica that syncs through the op log.
- Search: FTS5 BM25 + sqlite-vec KNN fused with Reciprocal Rank Fusion (k=60); keyword-only
  when AI is disabled. Facets from typed fields. JSON query DSL compiled to SQL, shared by
  views, plugins, MCP tools, and AI natural-language queries (NL -> DSL shown to the user
  before it runs).
- Views/libraries: React Flow (@xyflow/react) + ELK/dagre for mind map & node editing;
  Excalidraw (MIT) for freeform canvas with embedded object cards; Sigma.js + graphology
  (WebGL) for the large graph view (Cytoscape.js optional); Vega-Lite as the serializable
  chart spec, Apache ECharts for heavy dashboards/Sankey/treemap/sunburst; MapLibre GL for
  maps; TanStack Table + TanStack Virtual for grids. DO NOT use tldraw (production requires a
  license key). Verify every dependency license in CI (cargo-deny + a JS license checker;
  allow MIT/Apache/BSD/ISC/MPL/Zlib; flag anything else).
- Files mirror: pulldown-cmark or comrak for Markdown, a MAINTAINED YAML crate (not
  serde_yaml, which is no longer maintained), the notify crate for watching, diff3-style
  3-way merge. Choose and record each in an ADR.
- View recommender: TypeScript rule engine ranking renderers by data shape (field kinds,
  cardinality, dates, geo, hierarchy, relations) and user goal (compare, trend,
  distribution, part-to-whole, relationship, flow, location), using the Cleveland–McGill
  accuracy ranking (position > length > angle > area > volume > color saturation) and
  Draco-style hard/soft constraints. Must explain each recommendation in one sentence.
- Plugins (M10): Extism (Wasmtime) behind a PluginRuntime trait (iOS forbids JIT, so an
  interpreter backend such as Wasmtime's Pulley or wasmi must be evaluated before M14).
  Manifest-declared capabilities; ALWAYS pass an explicit allowed_hosts list (Extism treats
  null as "all hosts"); allowed_paths scoped; memory/timeout limits; host functions for
  scoped object access. UI plugins run in sandboxed iframes with strict CSP and typed
  postMessage RPC; never disable CSP; never expose the host DOM.
- Ingestion: Importer trait { accepts(), extract() -> objects/relations/blocks/derived
  blobs/provenance }. Pipeline: sniff -> hash/dedupe -> store blob -> extract -> map to
  type -> (optional enrich) -> index -> provenance. Built-in Rust importers first. Optional
  sidecars (auto-detected, never required): Docling via a one-click, private,
  version-pinned Python environment created with uv inside the app data folder (never
  touch the system Python); whisper.cpp; ffmpeg; Tesseract OCR. On mobile, sidecar jobs go
  into a queue for a desktop on the same synced vault.
- Sync (M13, behind a SyncAdapter trait): Loro CRDT (movable tree + rich text), E2EE op
  batches over LAN or a SELF-HOSTED relay (ship the relay binary + docs; no hosted
  service); Yjs/Yrs as an alternative adapter.

## 4. Core schema (implement via versioned migrations; change only with an ADR)
Tables: types(id, name, plural, icon, color, extends, description, is_system, timestamps);
fields(id, type_id, key, label, kind[text|rich_text|number|currency|boolean|date|datetime|
duration|select|multi_select|relation|url|email|phone|geo|file|rating|formula|rollup|json],
options_json, required, sort_order); objects(id, title, summary, parent_id, sort_key,
props_json, created_at, updated_at, captured_at, event_start, event_end, geo_lat, geo_lon,
deleted_at, sensitivity); object_types(object_id, type_id) [multi-typing];
blocks(id, object_id, parent_block_id, sort_key, kind, content_json, timestamps);
relations(id, src_id, dst_id, rel_type, field_id, src_block_id, label, props_json,
created_at, deleted_at) + indexes on (src_id, rel_type), (dst_id, rel_type);
blobs(hash, size, mime, encrypted, created_at); attachments(object_id, blob_hash, role,
filename); provenance(id, object_id, source_kind, source_uri, import_run_id, original_blob,
extractor, extractor_version, confidence, accepted, created_at); views(id, name, renderer,
query_json, config_json, owner_object_id, timestamps); canvas_items(view_id, item_id,
object_id, x, y, w, h, z, style_json); ops(seq, op_id, actor_id, device_id, ts, kind,
target_id, payload_json, prev_hash, hash, signature); spaces(id, name, ai_policy,
settings_json); ai_audit(id, ts, provider, model, purpose, object_ids_json, bytes_sent,
user_initiated); mirror_state(object_id, rel_path UNIQUE, last_hash, base_text,
synced_seq, synced_at); conflicts(id, object_id, kind[edit_edit|parse_error|
bulk_change_hold|missing_id|cloud_conflict_file], details_json, conflict_copy_id,
created_at, resolved_at); fts (FTS5: object_id, title, body, props_text); vec_chunks
(sqlite-vec, M11).
Seed editable system types: Note, Page, Task, Person, Organization, Place, Event, Project,
Area, Resource, Media, WebClip, Document, Image, Audio, Video, Email, Message, Contact,
Dataset, Tag, Collection, Template, Highlight, Journal.

## 5. Two-way files folder (MVP; the riskiest component, so engineer it carefully)
Vault layout: MyVault.dbme/{vault.sqlite, blobs/, files/, export/, plugins/, themes/,
logs/ai-audit.jsonl, FORMAT.md}. files/ location is configurable and can be limited to
chosen spaces.
- File shape: files/<Space>/<Parent>/<Title>.md; an object's children live in a folder named
  after it. YAML frontmatter holds id (authoritative identity), types, fields, and
  relations (IDs + wikilinks); the body holds blocks as Markdown with ^block-id anchors.
  Follow Obsidian conventions (wikilinks, #tags, properties) so the folder opens as an
  Obsidian vault.
- Out: debounce (~1 s), write atomically (temp file + rename), record the content hash in
  mirror_state so the watcher ignores the app's own writes (no echo loops).
- In: notify watcher -> debounce -> parse -> diff vs base_text -> apply as normal ops with
  provenance source_kind "file:external" (undo/history work as usual).
- Rename/move in another tool = rename/reparent (a moved folder reparents its subtree). A
  missing id is matched by content hash within the batch, otherwise the file becomes a new
  object.
- Both sides changed: 3-way merge (frontmatter field by field, body via diff3). On a true
  conflict, keep the app version and save the file version as a conflict-copy object,
  listed in a Conflicts inbox. NEVER silently drop either side.
- New .md -> new object (type from frontmatter, else Note). Any other file dropped in goes
  through the ingestion pipeline. A deleted file sends its object to Trash (recoverable).
- Bulk-change guard: if one batch deletes or rewrites > 50 files or > 10% of the folder,
  pause and ask the user.
- Unparseable frontmatter: never apply it; leave the file untouched; flag it in the
  Conflicts inbox. Recognise Dropbox/iCloud/OneDrive/Syncthing conflict files and route
  them to the inbox.
- Canvas layouts, view configs, the op log and the AI audit stay database-only (exported
  as read-only JSON).
- If vault encryption is on, files/ is off by default; enabling it shows a plaintext
  warning.

## 6. Platform targets & performance budgets (decided)
- Desktop minimum = reference machine for ALL budgets: 64-bit 4-core CPU (x86-64 or ARM64,
  ~2018+), 8 GB RAM, SSD. Windows 10 22H2/11; the three most recent major macOS versions
  (Intel + Apple Silicon); Linux with WebKitGTK 4.1 (Ubuntu 22.04+, Debian 12+).
  Recommended: 16 GB RAM.
- Local AI tier (optional): 16 GB RAM for ~3–8B 4-bit models; 32 GB+ unified memory or 8 GB+
  VRAM recommended; smaller machines can use a model server on the user's own network.
- Mobile (M14): current + two previous major iOS versions; Android 10+; 4 GB RAM.
- Budgets (CI benchmarks): cold start < 2 s at 100k objects; typing latency < 16 ms;
  search < 50 ms at 100k, < 300 ms at 1M; idle memory < 300 MB; 100k-row table at 60 fps;
  2,000-node mind map interactive; 100k-node graph (WebGL); files folder round-trip < 2 s
  each way; 1M objects / 1 TB of blobs must work (slower, never broken).

## 7. Repository structure
DataBase.Me/
  apps/desktop/            # Tauri 2 app (src-tauri/ + web UI entry); mobile targets at M14
  apps/clipper/            # browser extension (M9)
  apps/relay/              # self-hostable E2EE sync relay (M13)
  crates/core/             # domain: objects, types, relations, ops, query DSL, policies
  crates/store/            # SQLite schema, migrations, FTS, vectors, blob store, crypto
  crates/mirror/           # two-way files folder engine (M5)
  crates/ingest/           # importer trait + built-in importers, sidecar manager
  crates/export/           # JSON/CSV export + full export/import
  crates/plugins/          # PluginRuntime trait + Extism host (M10)
  crates/ai/               # provider adapters, policy gate, audit (M11)
  crates/mcp/              # MCP server + client (M11)
  crates/sync/             # SyncAdapter + Loro impl (M13)
  crates/cli/              # `dbme` CLI (import/export/verify/query/mirror-check)
  packages/ui/             # React components, design system, theming
  packages/views/          # renderer registry + each view (all responsive/touch)
  packages/recommender/    # view/chart recommendation rules
  packages/sdk/            # plugin SDK (Apache-2.0)
  docs/                    # user guide, FORMAT.md, ARCHITECTURE.md, adr/, GOVERNANCE.md
  fixtures/                # sample vaults, Obsidian vault, CSVs, PDFs, images, mbox, takeout,
                           # malicious files
  .github/                 # workflows/, ISSUE_TEMPLATE/, FUNDING.yml
  LICENSE (AGPL-3.0), packages/sdk/LICENSE (Apache-2.0), CONTRIBUTING.md (DCO),
  CODE_OF_CONDUCT.md, SECURITY.md, CHANGELOG.md, README.md

## 8. Milestones (do them IN ORDER; do not start the next until acceptance passes)
M0 Foundations: monorepo, Tauri shell, CI on Windows/macOS/Linux (fmt, clippy -D warnings,
   eslint, typecheck, tests, build, licence check, DCO check), LICENSE files, GOVERNANCE.md,
   CODE_OF_CONDUCT.md, CONTRIBUTING.md, FUNDING.yml, issue templates, FORMAT.md draft,
   ADR-0001..0008 (stack, storage, IDs, licence, plugin model, two-way files folder,
   governance/DCO, platform targets). Accept: app launches on all 3 OSes in CI; CI green;
   unsigned commits fail the DCO check.
M1 Vault & object core: schema + migrations, services, op log with undo/redo, blob store
   with dedupe, typed IPC (commands + event stream). Accept: >=90% coverage on crates/core &
   crates/store; 10k-object CRUD + undo < 2 s; kill-during-write test leaves a valid vault;
   core crates cross-compile for aarch64-apple-ios and aarch64-linux-android in CI.
M2 Editor & basic views: block editor, Document, Outline, Table (virtualized), Gallery,
   hierarchy sidebar, backlinks. Accept: live cross-view consistency; 100k-row table at
   60 fps; everything keyboard-operable; every view passes e2e tests at 360-px width with
   touch input.
M3 Mind map: React Flow mind map over hierarchy + relations; edits write back; auto-layout;
   touch gestures; round-trip with Outline. Accept: 2,000 nodes interactive; e2e tests for
   create/move/reparent.
M4 Search & query: FTS5, facets, DSL, saved views, command palette. Accept: < 50 ms search
   on 100k objects (CI benchmark); DSL golden tests.
M5 Two-way files folder & export: everything in section 5, plus JSON/CSV export and full
   export/import. Accept: app->file and file->app each < 2 s; a property test of 10,000
   random interleavings of app and file edits loses ZERO content (merged or conflict copy);
   no echo loops; deleting 50 files triggers the guard; rename/move = rename/reparent;
   Obsidian fixture opens with working links; rebuilding from files/ alone reproduces every
   object, field, relation and block; fuzzing of the Markdown/frontmatter parser.
M6 Ingestion v0: Markdown/Obsidian vault, CSV/XLSX (rows -> objects), PDF text,
   images + EXIF + thumbnails, drag-drop any file; import runs with dry-run preview, dedupe
   report and undo. Accept: Obsidian fixture keeps links/frontmatter; provenance on every
   imported object; malicious fixtures (zip bombs, malformed PDFs, path traversal) are
   handled safely.
M7 MVP 0.1: Simple & Organizer modes, settings, onboarding (files-folder location, opt-in
   update checks), light/dark themes, docs, signed installers, Copy diagnostic report.
   Accept: scripted usability scenario (capture, organize, find, edit a file outside the
   app, export) passes; ZERO network requests on first launch with defaults (network test);
   docs + FORMAT.md v0.1 published.
M8 Visual v1: Canvas, Kanban, Calendar, Timeline, Map, Charts/Dashboards, view recommender,
   WebGL graph. Accept: recommender >=80% agreement on labelled fixtures; accessible
   alternative + phone layout for each view; 100k-node graph renders.
M9 Capture & security: web clipper (Readability + SingleFile snapshot), OCR, mbox/.eml,
   ICS/vCard, RSS, extra drop folders, vault encryption, scheduled backups. Accept: encrypted
   vault unreadable without passphrase; files/ off by default when encrypted; backup/restore
   test passes.
M10 Plugins: PluginRuntime trait + Extism host, permissions UI, iframe UI sandbox, SDK, 3
   sample plugins (importer, view, formula fn). Accept: security tests prove no
   network/file/scope access without a grant and no host-DOM access.
M11 AI & MCP: providers, policy gate, audit log, embeddings, hybrid RRF search,
   auto-tag/link/summarize/extract as SUGGESTIONS with provenance, NL -> DSL, MCP server +
   client, onboarding AI question (shown once to existing users after upgrade). Accept: with
   "No AI", the network test shows zero AI traffic; policies enforced; every call audited;
   MCP conformance with a reference client.
M12 Rich media & archives: whisper.cpp, ffmpeg, one-click Docling sidecar (uv-managed),
   Google Takeout, IMAP. Accept: 1-hour audio transcribed locally and searchable; Takeout
   sample imported with provenance; sidecar install never modifies the system Python and
   uninstalls cleanly; the app works fully with no sidecar.
M13 Sync: Loro SyncAdapter, E2EE over LAN or the self-hosted relay, device pairing.
   Accept: convergence property test after offline edits; relay stores only ciphertext;
   each device's files/ stays consistent.
M14 Full mobile app: iOS + Android; all views and modes; standalone or synced vault;
   sidecar jobs queued for a desktop; plugins via an interpreter backend if feasible;
   local AI via the user's network. Accept: the M7 scenario passes on a phone and a tablet;
   offline phone edits sync with a desktop; mobile budgets met.
M15 Shared spaces & co-editing: roles, member keys, signed ops, real-time co-editing.
   Accept: role permissions enforced; signatures verified; two users co-edit without loss.

## 9. Coding standards
- Rust: stable toolchain, rustfmt, clippy -D warnings, no unwrap() in non-test code,
  thiserror/anyhow, tracing for logs, all SQL in crates/store with prepared statements.
- TypeScript: strict mode, eslint + prettier, no `any` without a justification comment,
  small pure components; all IPC types generated from Rust (e.g., specta/ts-rs).
- Every schema change = migration + FORMAT.md update + export/import and mirror test
  updates.
- Every architectural decision = ADR. Every public API = doc comments.
- Performance budgets from section 6 enforced by benchmarks in CI (criterion for Rust,
  Playwright traces for UI).
- Never send user data over the network except via explicit, user-enabled features.
- Every commit carries a DCO Signed-off-by line.

## 10. Testing & documentation requirements
- Unit tests (Rust + Vitest); property tests (proptest) for the op log, query DSL, export
  round-trip, files-folder merging, and (later) CRDT convergence; fuzzing for the
  Markdown/frontmatter parser and importers.
- Integration tests on fixture vaults; e2e tests (Playwright / tauri-driver) per view at
  desktop and phone sizes.
- Security tests: plugin sandbox escapes, AI policy gate, encryption, path traversal in
  importers and the files folder, malicious file fixtures; network-egress tests for "zero
  requests by default".
- Accessibility tests (axe) on every view; visual regression across OSes.
- Benchmarks for 10k / 100k / 1M objects.
- Docs: README (vision, install, screenshots, donate link), docs/user-guide,
  docs/FORMAT.md (complete enough to write a reader without the app, including the files
  folder format), docs/ARCHITECTURE.md, docs/adr/*, GOVERNANCE.md, CONTRIBUTING.md (DCO,
  how to report bugs with the diagnostic report), SECURITY.md (threat model + reporting),
  plugin SDK guide with samples, relay self-hosting guide (M13), CHANGELOG.md.

## 11. Working protocol
- Before coding each milestone: post a short plan (tasks, risks, ADRs needed).
- Work in small, reviewable commits/PRs with tests. Keep main green.
- After each milestone: run the full acceptance checklist, paste the results (test counts,
  coverage, benchmark numbers vs the section 6 budgets, screenshots/gifs of views at
  desktop and phone sizes), list known issues, then STOP and report: what was built, what
  was verified, deviations from this brief (with ADRs), and the proposed plan for the next
  milestone.
- If a requirement conflicts with reality (library licence, platform bug, performance),
  choose the option most consistent with the principles in section 2, write an ADR, and
  flag it in the report. Never silently drop a principle.
- Start now with M0.
