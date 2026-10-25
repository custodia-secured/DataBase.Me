# DataBase.Me: A Research-Grounded Build Plan for an Open-Source, Local-First Lifetime Information Repository ("a mind-map on steroids")

Build it as one typed object graph stored in a single local SQLite vault plus a content-addressed blob folder. Expose that graph through many interchangeable views: mind map, canvas, graph, outline, table, kanban, calendar, timeline, map, charts and dashboards. Ship it as a Tauri 2 desktop app (and later a full mobile app from the same code) with a Rust core, a TypeScript/React UI, sandboxed WebAssembly plugins, and an AI layer that is off by default and connects through OpenAI-compatible, native-provider and MCP adapters. The research points the same way throughout. People find personal information mainly by navigating structures they built themselves, not by searching. Building visual node-link maps measurably helps learning. The products that last are the ones that never trap your data. So the foundation has to combine structure you can navigate, search that works, many lenses on the same data, and formats that will still open in 2060.

> **Status, 25 October 2026:** all ten open decisions are resolved and folded into this plan (see §8). The biggest changes from the first draft are: the product is named **DataBase.Me**; the Markdown files folder is **two-way from the MVP**; mobile is a **full app**, not a capture companion; and the project is **funded by donations**, with no hosted service and no telemetry.

## TL;DR

- **Architecture:** a typed-object property graph (objects, types with inheritance and multi-typing, typed bidirectional relations, block content, provenance) in SQLite. SQLite is a Library of Congress–recommended preservation format. Files live in a content-addressed blob store, history is kept in an append-only operation log, and every object is also kept as a plain Markdown file in a **two-way files folder**. You can edit those files in any editor, or open the folder in Obsidian, and the changes flow back into the app. Schema, views and tables are also exported as JSON and CSV, so you are never locked in. Search is SQLite FTS5 plus sqlite-vec, merged with Reciprocal Rank Fusion.\[1\]\[2\] Multi-device sync and sharing come later, using the Loro CRDT over end-to-end-encrypted peer sync.
- **What this product owns that no existing tool fully does:** (1) one data model rendered through every view type, with automatic chart and view recommendations grounded in perception research; (2) "lifetime" ingestion of every data type with full provenance; (3) plugins that are actually sandboxed through capability-scoped WebAssembly, unlike Obsidian, where plugins get full machine access; (4) AI that is optional, provider-agnostic, permissioned per collection, audit-logged, and exposed to outside agents through MCP.
- **How to build it:** milestone by milestone, starting with an MVP of vault, typed objects, Markdown editor, outline, table, mind map, search, the two-way files folder, import of notes/CSV/PDF/images, and export. Everything else comes in v1, v2 and later stages: canvas, charts, timeline and map; then plugins, AI and email/audio ingestion; then sync, the full mobile app and shared spaces. A complete, self-contained handoff prompt for the coding agent is the final section.

---

## 1. Vision and Positioning

**Vision.** A personal "information operating system" for your whole life. It is private by default, runs on your own machine, is free and open source, and can hold, connect and present anything you capture: a grocery list, a PhD literature review, family photos, 20 years of email, a fitness log, a business CRM.

**Core promise (the product's constitution):**
1. **Your data, your disk, forever.** Local-first and open formats. Every object is also a plain Markdown file you can edit with any tool, full export is one click, and it works offline.
2. **Any data in.** Notes, documents, spreadsheets, PDFs, images, web pages, email, audio, video, feeds, takeouts, calendars, contacts, location, health and code.
3. **Any view out.** The same object can appear as a node in a mind map, a row in a table, a card on a kanban board, a pin on a map or a point on a chart.
4. **Everything changeable.** Types, fields, views, themes, shortcuts, layouts, formulas, scripts and plugins.
5. **AI optional.** Off by default. Plug in any provider, local or cloud, with per-collection permissions and an audit log.
6. **Simple to start, deep when needed.** Progressive disclosure, preset "modes", and complexity the user can grow into.
7. **No strings.** No account, no telemetry, no paid tiers, no hosted service. Funded by donations.

**Name:** *DataBase.Me* (decided). Rust crate and npm package names can't contain dots, so code identifiers use the short form `dbme`: crates `dbme-core` and so on, the CLI `dbme`, vault folders `*.dbme`, and the npm scope `@databaseme`. The GitHub repository itself can be called `DataBase.Me`. Before the first public release, check trademarks and claim the GitHub organization, the npm scope and a domain.

**Positioning line:** "Obsidian's ownership, Tana's structure, Notion's views, Heptabase's canvas, Perkeep's permanence: open source and yours for life."

---

## 2. Research Findings

### 2.1 Theory of organizing information

**Navigation beats search for personal information, so structure matters.** Bergman, Whittaker and colleagues have repeatedly found that people retrieve their own files mainly by navigating folders, not by searching.\[3\]\[4\] One study (N = 519) and a 7-month longitudinal study found that preference survived the arrival of better desktop search. Their 2008 ACM TOIS paper reports "no evidence that using improved desktop search engines leads people to change their filing habits."\[5\] A 2015 fMRI study by Benn, Bergman, Whittaker and colleagues in *Scientific Reports* (5:14719) found that folder navigation activates "the posterior limbic (including the retrosplenial cortex) and parahippocampal regions", which are used in physical navigation, while "search activates the left inferior frontal gyrus", a language area. That suggests navigation costs less verbal attention. A dual-task study (Bergman, Tene-Rubinstein & Shalom, *Personal and Ubiquitous Computing*, 2013) found that "participants remembered significantly more words when retrieving by navigation than by search." Bergman et al.'s 2010 JASIST study of 296 participants and 1,131 file retrievals found an average retrieval depth of 2.86 folders and about 11.8 files per folder. Participants found 94% of their files, taking 14.76 seconds on average, and the authors modelled the trade-off between folder depth and folder size. **Design implication:** a pure "everything is search plus AI" product will feel disorienting. The app must offer stable, user-built places, such as hierarchies, spaces and pinned maps, that people can navigate spatially. Search and AI are the second retrieval path, not the only one.

**Building node-link maps helps people learn and remember.** Nesbit & Adesope's 2006 meta-analysis covered 55 studies, 67 effect sizes and 5,818 participants. Concept maps were "associated with increased knowledge retention" across settings.\[6\] Schroeder et al.'s 2018 follow-up pooled 142 effect sizes (n = 11,814) and found an overall g = 0.58. *Creating* maps (g = 0.72) helped more than *studying* maps someone else made (g = 0.43).\[7\] Nesbit & Adesope also noted that other constructive formats, such as outlines and lists, produced comparable effects. The active construction may matter more than the specific diagram.\[8\] **Design implication:** make it easy for users to *build* structure themselves (mind maps, outlines, links, canvases). AI should *suggest* structure, not silently impose it, because auto-generated maps give up the larger "creating" benefit.

**Graphical perception has an evidence-based hierarchy.** Cleveland & McGill (1984, *JASA*) ranked how accurately people decode visual encodings: position on a common scale first, then position on unaligned scales, then length, direction and angle, then area, then volume, curvature and colour saturation.\[9\]\[10\] Heer & Bostock ("Crowdsourcing Graphical Perception", CHI 2010) replicated Cleveland & McGill's study on Mechanical Turk and got the same ranking. They also extended it to area encodings such as bubble charts and treemaps. **Design implication:** default to dot plots, bar charts and line charts. Use pies, bubbles and treemaps only when part-to-whole or hierarchy is the point, and never for precise comparison.

**Organization schemes (all supported as lenses on one model, not as competing systems):**

| Scheme | What it's good for | How DataBase.Me supports it |
|---|---|---|
| **Hierarchy / taxonomy** (folders, outlines) | Navigation, spatial memory, novices | Spaces → collections → nested objects. Every object can have one "home" parent plus any number of other links. |
| **Faceted classification** | Filtering big collections along independent dimensions | Typed fields become automatic facets in every list view. |
| **Tags / folksonomy** | Low-friction capture | Tags are lightweight objects, so they can be renamed, merged and nested. |
| **Network / graph** (Zettelkasten, wikis) | Discovering connections, emergent ideas | Typed bidirectional relations, backlinks, unlinked-mention detection, graph view. |
| **Mind map** (radial tree) | Brainstorming, single-topic decomposition | Mind-map view over any tree or subgraph. Edits write back to real objects and relations. |
| **Concept map** (labelled links) | Explaining *how* things relate | Relation labels and types appear as edge labels. |
| **Zettelkasten** | Atomic notes plus links for thinking | "Atomic note" template and permanent IDs. |
| **PARA** (Projects/Areas/Resources/Archives) | Action-oriented life organization | Optional starter preset. |
| **Johnny.Decimal** | Strict numeric shelving for findability | Optional preset with auto-numbered collection IDs. |
| **LATCH** (Location, Alphabet, Time, Category, Hierarchy) | The finite ways to organize anything | Every collection gets the five LATCH lenses at no extra cost: map, A–Z, timeline, faceted grouping, tree. |
| **Knowledge graph / ontology** | Formal semantics, interoperability | Types and relations exportable to JSON-LD/RDF. schema.org mappings are optional. |
| **Spatial canvas** | Visual thinking, clustering, spatial memory | Infinite canvas where cards are live objects. |
| **Outliner / blocks / transclusion** | Fine-grained reuse | Block-level IDs. Embed any block or object anywhere, live. |
| **Progressive summarization** | Distilling over time | Highlight layers (bold → highlight → summary field) plus optional AI drafts. |

**Layering complexity for "anybody".** Users should see the minimum needed on day one: notes, folders, search. Typed fields, relations, views, formulas and plugins appear when a user reaches for them. "Modes" (Simple, Organizer, Researcher, Data/Power) are presets of features that are switched on, not separate products.

### 2.2 Prior art and competitive landscape

| Tool | Data model | Strengths people love | Why people leave or what's missing |
|---|---|---|---|
| **Obsidian** | Markdown files + YAML properties; Canvas (`.canvas` JSON); Bases (`.base`) | Plain files, longevity, huge plugin ecosystem (about one million users, per Obsidian's own 2023 estimate to Fast Company, based on app-bundle downloads because the app has no analytics) | Typed data was an afterthought. Bases first appeared in 1.9 early access (May 2025) and reached all users in August 2025, with a Bases API in 1.10 (a Maps view plugin was the example).\[11\]\[12\]\[13\] Plugins are unsandboxed: Obsidian's own docs say plugins "can access files on your computer… connect to internet… install additional programs". Not open source.\[14\]\[15\] |
| **Logseq** | Was an outliner over Markdown files; now moving to SQLite | Open source (AGPL), block references, local | Shows the file-to-database migration pain directly. The "2.0 Beta (DB version)" shipped July 13, 2026 (2.0.2 on Oct 7, 2026) with SQLite as the canonical store, and the file-based app was split off as "Logseq OG", which gets maintenance only.\[16\]\[17\] Its iframe plugin sandbox had escape CVEs in 2026.\[18\] |
| **Anytype** | Objects with Types and Relations; any-sync, a CRDT/DAG-based P2P sync\[19\]\[20\] | Local-first, E2EE, typed objects, a property can link to any type\[19\] | Its own sync protocol and format, so lock-in risk. Learning curve. |
| **AFFiNE** | Blocks + whiteboard ("edgeless") | Doc and canvas duality | Young, and its data model is less mature. |
| **Tana** | Nodes + composable supertags with fields; "Extend" inheritance | Most expressive schema (one node can carry #book, #gift and #recommendation and inherit fields from all three)\[21\] | Cloud-only. Export to Markdown/JSON flattens supertags into text and node references lose their targets, a textbook lock-in lesson.\[22\] |
| **Capacities** | Object types (one type per object, no inheritance)\[23\] | Friendly, guided object model | Less expressive than Tana. Proprietary cloud. |
| **Notion** | Blocks + databases | Views (table/board/calendar/gallery/timeline), templates | Cloud lock-in, weak offline support, slow when large. |
| **Heptabase / Scrintal / Kinopio** | Cards on whiteboards | Spatial thinking | Proprietary. Limited typed data and ingestion. |
| **Roam** | Block graph | Bidirectional links pioneered | Cloud, cost, stalled development. |
| **TheBrain** | Typed thought graph | 20+ year-old graphs; the longest-lived "lifetime" mind-map example | Proprietary, dated UI. |
| **Trilium / SiYuan / Joplin** | Tree notes / blocks / Markdown | Open source, self-hostable | Narrow on views or data types. |
| **Mem / Reflect** | AI-first notes | AI recall | Cloud, black box, AI not optional. |
| **Zotero** | Typed items (CSL) + attachments + annotations | Gold standard for typed research objects and citation export | Domain-specific. |
| **Perkeep (Camlistore)** | Content-addressed blobs; mutable "permanodes" modified by signed "claims"\[24\]\[25\] | Designed for lifetime storage with a schema "easy to recover… in five decades"\[26\] | Developer-oriented, minimal UI. Its architecture is the best inspiration here. |
| **Memex/WorldBrain, Hypothesis, Readwise** | Annotations anchored to web/PDF | Highlight-centric capture | Narrow scope. |
| **DEVONthink** | Files + AI classification | Ingests everything, "See also" | macOS-only, proprietary. |
| **Excalidraw / tldraw / Miro** | Freeform canvas | Visual thinking | Not knowledge stores. tldraw's SDK now needs a license key in production (since SDK 4.0, Sept 2025), which rules it out as a free embedded dependency. Excalidraw is MIT.\[27\]\[28\] |
| **Airtable / NocoDB / Baserow / Grist** | Relational tables | Typed data, views, formulas | Table-first. Weak for prose and visual thinking. |
| **Apple Photos / Immich / PhotoPrism** | Media + EXIF + ML | "On this day" memory surfacing, maps, faces | Media only. |
| **Paperless-ngx** | Documents + OCR + auto-tagging | Document ingestion pipeline | Documents only. |

**Gaps no existing tool fills, which this product should own:**
1. **True view polymorphism over typed data.** No tool lets the *same* typed objects move freely between mind map, canvas, table, kanban, calendar, timeline, map and Vega-Lite charts with edits flowing back.
2. **Lifetime-grade ingestion with provenance.** No PKM tool treats email, audio, takeouts, location and health as first-class objects with source, hash and import-run lineage.
3. **Typed schemas that export losslessly.** Tana and Capacities flatten types on export. DataBase.Me's export *is* its schema: types, fields and relations survive as JSON plus Markdown frontmatter.
4. **Sandboxed plugins.** Obsidian and VS Code run extensions with full user privileges.\[29\] Capability-scoped WebAssembly is now practical.
5. **Visualization intelligence.** Recommending the right chart for a field combination using perception research (Draco/Cleveland–McGill), not chart-type guesswork.
6. **AI that is truly optional and auditable**, with per-collection permissions, local-model parity, and MCP exposure to any agent.
7. **Open source + local-first + typed + visual**, all together. Anytype comes closest but uses a bespoke format and sync.

### 2.3 Universal data model research

- **Typed objects with user-defined schemas.** Anytype ("every object has one Type… Types and Relations"),\[20\]\[30\] Capacities (object types) and Tana (supertags with fields and inheritance) converge on the same idea: an object is an instance of a user-editable type that declares fields. Tana's *multi-typing* (one node, several supertags) and *Extend* (inheritance) are the most expressive.\[31\]\[32\] DataBase.Me should support both.
- **Property graph vs RDF.** A property graph (nodes and edges both carrying properties) fits user mental models and SQL better than raw triples. Keep RDF/JSON-LD as an *export* format for interoperability.
- **Mutable objects on immutable storage.** Perkeep models mutable data as a stable "permanode" whose state is "the result of combining all attribute-modifying claims which reference it, in order."\[24\]\[33\] That is event sourcing over content-addressed blobs. DataBase.Me uses the same pattern: an append-only op log is the history of record, and materialized tables give speed.
- **Content addressing for files.** Store each file once under its hash. That gives deduplication and integrity checking for free, and an import can never silently corrupt the original.
- **Blocks and transclusion.** Rich text is an ordered tree of blocks with stable IDs, so any block can be referenced or embedded elsewhere, as Roam and Logseq do.

### 2.4 Local-first architecture and sync

- **The seven ideals.** Ink & Switch's 2019 essay (Kleppmann, Wiggins, van Hardenberg, McGranaghan) defines them: no spinners, multi-device, offline, collaboration, longevity ("The Long Now"), privacy by default, and user ownership.\[34\]\[35\] Kleppmann later said (Local-First Conf 2024) that the ideals are better treated as a *gradient* than a checklist.\[36\] That justifies building single-device first and adding sync later.
- **SQLite as the application file format.** SQLite is a US Library of Congress recommended format for datasets (alongside CSV, JSON and XML).\[37\] Its developers promise backwards-compatible files and support "through the year 2050", and the file is "bit-for-bit identical" across architectures.\[38\] **Logseq's 2026 move to a SQLite canonical store confirms that a file-per-note design hits limits with typed, relational data.**\[17\]
- **Plain-text-first vs database-first.** Plain Markdown is easy to read in 50 years but weak at types, relations, transactions and performance at millions of objects. Database-first is strong on all of those but opaque. **Decision: database-first, with a two-way plain-text replica from day one.**
  - SQLite stays canonical.
  - A `files/` folder holds one Markdown file per object. Each file has YAML frontmatter (id, types, fields, and relations as IDs plus wikilinks), and its body is Markdown with block anchors. Files sit in folders that follow the object hierarchy.
  - The app writes changes out within seconds. A file watcher reads edits made in any other tool back in as ordinary operations.
  - **The files folder is treated as just another replica syncing through the op log**, the same way a second device will later. That is what keeps two-way editing safe.
  - JSON (schema and views) and CSV (one per type) are written alongside, export-only in the MVP.
  - All of it is documented in an open format spec. §3.4 sets out the merge and safety rules.
- **CRDT choice.** Benchmarks mostly come from interested parties, so treat them with caution:
  - **Loro**, per its own Oct 2024 native benchmark, decoded a reference document in 0.189 ms vs Automerge's 506 ms.\[39\]
  - A third-party 2026 benchmark PR (giosuel/imaginarium) found only Loro avoided cycles after concurrent tree moves, supported history trimming, and opened a 300-page doc in 2.7 ms. Yrs (Rust Yjs) was fastest per edit. Automerge took 377 ms per remote keystroke on large text.\[40\]
  - Yjs has by far the largest ecosystem.\[41\]
  - **Recommendation:** Loro for CRDT-backed rich text and its movable-tree CRDT, which suits mind maps and outlines.\[42\] Keep a sync-adapter boundary so Yjs/Yrs can be swapped in.
  - CRDTs only matter once there are multiple replicas, so the MVP uses plain SQLite transactions plus the op log.
- **Sync topology.** Device-to-device over LAN, or relayed through a user-hosted relay (a tiny Rust binary, or any WebDAV/S3/Syncthing-style dumb storage holding *encrypted* op batches and blobs). There is no mandatory vendor server.

### 2.5 Ingestion

- **Documents.** Docling, from IBM Research and now under the LF AI & Data Foundation, is MIT-licensed. It parses PDF, DOCX, PPTX, XLSX, HTML, images and audio into structured Markdown or JSON, with layout analysis, table structure and OCR, and runs "entirely locally on commodity hardware."\[43\]\[44\]\[45\] It is Python, so it runs as a sidecar. **Decision: the Python sidecar is approved.** The app offers a one-click "Install advanced document parsing" option. It creates a private, version-pinned Python environment inside the app's data folder (using a tool such as uv) and never touches the system Python. Built-in Rust parsers (PDF text extraction, calamine for spreadsheets, csv) still cover the basics, so nothing depends on the sidecar. On mobile, where it can't run, heavy jobs can be queued for a desktop on the same synced vault. Apache Tika (JVM) and Marker are alternatives behind the same interface.
- **Media.** ffmpeg for audio and video (probe, thumbnails, keyframes), exif/XMP readers for photos, whisper.cpp for local transcription, Tesseract or Docling OCR for images.
- **Web.** A browser extension that saves Readability-cleaned HTML → Markdown plus a full SingleFile HTML snapshot (WARC optional) and source URL/time.
- **Email.** mbox/.eml import and an IMAP connector. Messages become objects, threads become relations, attachments become blobs.
- **Personal archives.** Google Takeout, Apple Health export, ICS calendars, vCard contacts, chat exports, location history (GeoJSON/KML), RSS/Atom, Git repos.
- **Pipeline pattern.** Detect (MIME sniff + extension) → hash and dedupe → store blob → extract (text, metadata, structure) → map to type → enrich (OCR/transcribe/optional AI) → index (FTS, vectors) → record provenance. Every importer is a plugin with the same contract. Folder-watching gives "drop zones".

### 2.6 Retrieval

- **Hybrid search in a single file.** SQLite FTS5 (BM25) plus sqlite-vec (vector KNN), fused with Reciprocal Rank Fusion.\[1\]\[46\]\[47\]\[48\] Cormack et al. (2009) found k = 60 performed best on average.\[49\] The open-source sqlite-vec-benchmark (v0.1, mycman on GitHub) tested BEIR SciFact (5,183 docs, 300 queries, BGE-small-en-v1.5 embeddings, MacBook Air M4). It reports SQLite hybrid RRF beating either ranker alone (nDCG@10 0.736 vs 0.721 vector-only and 0.681 BM25-only), with 5.69 ms p99 latency and a 24.1 MB index. That benchmark is a single small dataset, so treat it as indicative. Keyword search keeps working when no embedding model is configured. That fits "AI optional": vectors are only built if the user enables an embedding provider.
- **Facets and saved queries.** Every typed field becomes a filter. Queries are saved as View objects.
- **Timeline and "on this day".** Every object has created, captured and event times, so a time lens is available for everything.
- **Resurfacing.** Optional spaced review using FSRS, the open algorithm Anki integrated in version 23.10 as an opt-in alternative to SM-2 ("Support for FSRS (which improves upon the scheduling provided by SM-2) is now integrated into Anki").\[50\]\[51\] Also "random from collection", "on this day" and "stale project" nudges.
- **Graph queries.** Neighbours, paths and n-hop expansion via recursive CTEs over the relations table.
- **Natural-language query** (AI on) is translated into the *same* structured query DSL and shown to the user before running, so results stay transparent and editable.

### 2.7 Visualization and presentation

- **Principle.** One object store, many renderers. A View = (query) + (renderer type) + (encoding config). Every renderer edits the same underlying objects.
- **Choosing visualizations.** Draco (UW Interactive Data Lab, InfoVis 2018 Best Paper) formalizes design knowledge as hard and soft constraints over Vega-Lite and learns weights from perception experiments.\[52\]\[53\] DataBase.Me should ship a lighter TypeScript rule engine inspired by Draco and Cleveland–McGill: given field types (quantitative, temporal, nominal, ordinal, geo, hierarchical, network) and the user's stated goal (compare, trend, distribution, part-to-whole, relationship, flow, location), rank candidate views and explain why ("Bar chart: values compared by position on a common scale, the most accurate encoding"). The Draco Python package can be an optional power-user plugin.
- **Library choices (licence-checked where verified):**
  - **Mind map, flowchart, node editing:** React Flow / xyflow (MIT) with ELK/dagre layouts.
  - **Freehand canvas:** Excalidraw (MIT). Avoid tldraw unless a commercial licence is bought: since SDK 4.0 production use needs a licence key, and without one "the editor will stop rendering after five seconds."\[27\]\[54\]
  - **Large graph view:** Sigma.js (WebGL) with graphology for 10⁵–10⁶ nodes; Cytoscape.js for analysis features.
  - **Charts:** Vega-Lite as the declarative, serializable spec, which is exportable and AI-friendly. Apache ECharts for high-volume interactive dashboards, Sankey, treemap and sunburst. Observable Plot optional.
  - **Maps:** MapLibre GL with user-chosen tile sources, including offline PMTiles.
  - **Tables:** TanStack Table + TanStack Virtual (MIT).
  - **Timeline and calendar:** custom virtualized timeline; a calendar component.
- **Accessibility.** WCAG 2.2 AA. Every visual view has a keyboard-navigable and screen-reader equivalent (table or outline), respects reduced motion, and uses colour-blind-safe palettes with redundant encodings.
- **Scale.** Virtualize every list and table. Use WebGL for graphs and big scatterplots. Aggregate in SQL before rendering. Use level-of-detail on canvas.

### 2.8 Customizability and plugins

- **Lesson from Obsidian.** Its docs say that "Due to technical reasons, Obsidian is unable to reliably restrict plugins to specific permissions," so plugins inherit full access.\[15\] In 2026 an Obsidian plugin (Shell Commands) was abused to deliver a remote-access trojan (Elastic Security Labs, "Phantom in the vault").\[55\]\[56\]
- **Lesson from VS Code.** Extensions run in an isolated Extension Host *process* for stability, but per Microsoft's own docs "the extension host has the same permissions as VS Code itself."\[29\]\[57\]
- **Lesson from Logseq.** Plugins run in sandboxed iframes, but CVE-2026-47901 allowed a plugin to inject event-handler attributes into the host DOM "due to a disabled Content Security Policy," which escaped the sandbox.\[58\]\[59\]
- **Design.**
  - **Logic plugins** run in WebAssembly via Extism (BSD-3-Clause, built on Wasmtime, Rust host SDK, plugin kits in Rust/JS/Go/Python and more). They have no ambient I/O: Wasmtime states "the only thing a WebAssembly instance can do is what is available through interfaces it has been explicitly linked with."\[60\]\[61\]\[62\]
  - Capabilities are granted per plugin through the manifest: `allowed_hosts` (Extism treats an *empty* list as no network but `null` as *all hosts*, so DataBase.Me must always pass an explicit list), `allowed_paths`, memory and timeout limits, plus DataBase.Me host functions such as `objects.read(scope)` and `objects.write(scope)`.\[60\]\[63\]
  - **UI plugins** run in sandboxed iframes with a strict CSP, talk to the host only through a typed postMessage RPC, and never get direct access to the host DOM.
  - Users see a permission prompt like a phone app's ("This plugin wants: read 'Recipes' collection; network access to api.example.com").
- **Non-plugin customization** (no code needed): user types and fields, templates, saved views, formulas (a safe expression language, spreadsheet-like), themes (CSS variables and design tokens), keyboard remapping, layout presets, and "modes".

### 2.9 Optional AI layer and universal connector

- **Provider abstraction.**
  - The OpenAI-compatible Chat Completions/Embeddings API is the de-facto lingua franca: Ollama, llama.cpp `llama-server`, LM Studio, vLLM and many cloud vendors expose it.
  - Add native adapters where features differ (Anthropic Messages API, Google, etc.).
  - Capabilities are discovered per model: chat, embeddings, vision, tool use, context length.
- **MCP (Model Context Protocol).** It is an open protocol, launched by Anthropic in November 2024 and donated to the Linux Foundation's Agentic AI Foundation in December 2025.\[64\] The 2025-11-25 revision added async Tasks and OAuth improvements.\[65\] The current 2026-07-28 revision makes the protocol stateless: it removes the initialization handshake and protocol-level sessions, adds mandatory `server/discover`, and moves Tasks to an extension.\[66\]\[67\] DataBase.Me should:
  1. **Run an MCP server** exposing the repository: tools for `search`, `get_object`, `create_object`, `link`, `query_view`, and resources for objects and views. It must honour the same per-collection AI permissions and run over stdio or localhost only by default. Target 2026-07-28 and negotiate down to 2025-11-25 for older clients.
  2. **Act as an MCP client**, so the built-in assistant can use user-approved external tools.
- **AI features, each individually toggleable:** auto-tag, suggest links, summarize, classify into types, extract fields (e.g., invoice → amount, date), suggest visualizations, natural-language → query DSL, transcription (local Whisper), and RAG chat over permitted collections.
- **Privacy controls:**
  - Global AI switch, off by default. **Decision: onboarding asks.** One onboarding screen explains in plain words what each choice means. It offers *No AI* (the highlighted default), *Only AI running on this computer or my own network*, or *Let me choose providers*. Any choice can be changed later in Settings.
  - "Local-only" mode that blocks cloud providers.
  - A per-collection AI permission of none, local-only or any provider.
  - Field-level redaction (e.g., a "Sensitive" flag).
  - An **AI audit log** recording which objects and chunks were sent to which provider, when and why.
  - AI outputs are stored as *suggestions* with provenance (`source: ai:model-name`) until the user accepts them.

### 2.10 Tech stack

- **Tauri 2 vs Electron.** Tauri uses the OS webview and a Rust backend. One measured comparison (Hopp) found an 8.6 MiB vs 244 MiB bundle and about 172 MB vs 409 MB memory for the same six-window app.\[68\] Many 2026 vendor blogs report similar gaps, but they vary.\[69\]\[70\] Tauri also has least-privilege native API permissions and iOS/Android targets.\[69\]\[71\] The trade-off is that webview rendering differs across platforms (WebKit on macOS and Linux, WebView2 on Windows), so the test matrix must cover all three.
- **Mobile is a full app (decided).** That makes Tauri 2's iOS and Android targets important. It also puts three constraints on everything built before mobile ships:
  - The core crates must compile for mobile targets, checked in CI from M1.
  - Every view must work on a small touch screen, tested at phone width from M2.
  - Nothing essential may depend on a desktop-only sidecar.
- **Rust core.** Memory safety, first-class SQLite (rusqlite), Loro, Extism/Wasmtime and whisper.cpp bindings, and one core shared between desktop, a headless "server mode" (local web UI for a home lab or other devices) and a CLI.
- **TypeScript + React frontend** for the largest ecosystem of the chosen view libraries (React Flow, Excalidraw, TanStack). TipTap/ProseMirror or Lexical for the block editor (TipTap/ProseMirror is MIT).
- **Licensing, governance and funding (decided).**
  - **AGPL-3.0** for the application. It stops a closed SaaS fork from taking the work without giving back. Logseq is a precedent.
  - **Apache-2.0** for the plugin SDK, the file-format spec and the client libraries, so anyone, including commercial projects, can build plugins and readers.
  - **Governance: BDFL now, steering group later.** BDFL ("benevolent dictator for life") just means the founder has the final say while the project is small. A `GOVERNANCE.md` commits to handing that to a small steering group once there are three or more regular maintainers.
  - **Contributions: DCO, not CLA.**
    - With a DCO (Developer Certificate of Origin), contributors add a one-line `Signed-off-by:` to each commit, certifying they wrote the code and have the right to contribute it. There's no legal paperwork, and a CI check enforces it.
    - A CLA (Contributor License Agreement) makes contributors sign an agreement giving the project extra rights, mainly so it can relicense or sell commercial licences later.
    - With donation funding and no paid tiers, the project doesn't need those rights, and a DCO is far friendlier to contributors.
  - **Funding: donations only**, through GitHub Sponsors and/or Open Collective, linked from `FUNDING.yml`. No paid tiers and no hosted relay. The sync relay is a small binary users run themselves.

### 2.11 Security and privacy

- **Threat model.** A lost or stolen laptop; malware or a malicious plugin; a compromised sync relay or cloud drive; an AI provider receiving data it shouldn't; a shared-space member overreaching; and long-term risks of key loss and format rot.
- **At rest.** Encrypted vault option using SQLCipher-compatible encryption (e.g., via SQLite3MultipleCiphers, which supports AES-256 and all SQLCipher variants).\[72\] Blobs are encrypted per file with XChaCha20-Poly1305 using keys derived from a vault key. The vault key is wrapped by a passphrase (Argon2id) and optionally cached in the OS keychain.
- **Recovery.** A printable recovery key or kit, because lifetime data needs a key-loss plan. The plain-text export can be written unencrypted to a user-chosen location *only on explicit request*.
- **Sync.** End-to-end encrypted op batches and blobs. The relay sees only ciphertext and sizes. Devices are paired by QR or short code.
- **Files folder vs encryption.** A plain-text files folder defeats encryption at rest. When vault encryption is on, the two-way files folder is off by default. Turning it on shows a clear warning and suggests keeping it on an encrypted drive.
- **No telemetry (decided).** No analytics, no crash reporter, and no automatic network calls of any kind unless the user turns on a feature that needs one. Even update checks are opt-in at onboarding, because they reveal the user's IP address to the download host. For bug reports, a *Copy diagnostic report* button builds a local report with no user content (versions, OS, recent error codes) that the user can paste into a GitHub issue. Issue templates in the repo ask for it.
- **Multi-user (later).** Per-space membership with roles (owner/editor/commenter/viewer). Space keys are shared through member public keys. Each user signs their ops, as Perkeep signs claims, giving attribution and tamper-evidence.
- **Plugins.** Capability manifests, signed releases, no auto-update without consent, and a kill switch.
- **Secrets.** API keys go in the OS keychain, never in the vault or exports.

---

## 3. Recommended Foundational Architecture

### 3.1 Layered overview

```
┌────────────────────────────────────────────────────────────────────┐
│ UI (TypeScript/React: Tauri desktop + mobile, or browser/server)   │
│  Shell · Command palette · View host · Editor · Settings/Modes     │
│  Renderers: Doc · Outline · MindMap · Canvas · Graph · Table ·     │
│  Kanban · Calendar · Timeline · Gallery · Map · Chart · Dashboard  │
├──────────────── typed IPC (commands + event stream) ───────────────┤
│ Rust core (dbme-core)                                              │
│  Object service · Schema/type service · Relation/graph service     │
│  Query engine (DSL → SQL) · Search (FTS5 + sqlite-vec + RRF)       │
│  Op log / history / undo · Provenance · Blob store (BLAKE3)        │
│  Ingestion + importers · Files mirror (two-way .md) · Exporter     │
│  Plugin host (Extism/Wasmtime) · AI connector · MCP server/client  │
│  Crypto (vault key, blob enc) · Sync engine (Loro, later)          │
├────────────────────────────────────────────────────────────────────┤
│ Storage: vault.sqlite · blobs/aa/bb/<hash> · files/ · export/      │
│ Optional sidecars: Docling (Python) · whisper.cpp · ffmpeg · OCR   │
└────────────────────────────────────────────────────────────────────┘
```

### 3.2 Vault layout on disk

```
MyVault.dbme/
  vault.sqlite            # canonical store (optionally encrypted)
  blobs/ab/cd/<blake3>    # content-addressed files (optionally encrypted)
  files/                  # TWO-WAY Markdown replica (toggleable; location configurable)
    <Space>/<Parent>/<Title>.md      # YAML frontmatter (id, types, fields, relations) + body
    <Space>/<Parent>/<Title>/...     # an object's children live in a folder named after it
  export/                 # export-only in the MVP
    schema/types.json  views/*.json  data/<Type>.csv
  plugins/  themes/  logs/ai-audit.jsonl
  FORMAT.md               # the open spec, so the vault documents itself
```

### 3.3 Core schema sketch (SQLite)

```sql
-- Identity: ULIDs (sortable, offline-safe). All timestamps are UTC ISO-8601 or epoch ms.
CREATE TABLE types (
  id TEXT PRIMARY KEY, name TEXT NOT NULL, plural TEXT, icon TEXT, color TEXT,
  extends TEXT REFERENCES types(id),          -- inheritance (Tana "Extend")
  description TEXT, is_system INTEGER DEFAULT 0, created_at INTEGER, updated_at INTEGER
);
CREATE TABLE fields (                          -- field definitions belong to a type
  id TEXT PRIMARY KEY, type_id TEXT REFERENCES types(id), key TEXT, label TEXT,
  kind TEXT CHECK (kind IN ('text','rich_text','number','currency','boolean','date',
       'datetime','duration','select','multi_select','relation','url','email','phone',
       'geo','file','rating','formula','rollup','json')),
  options_json TEXT,       -- choices, number format, relation target types, formula source
  required INTEGER DEFAULT 0, sort_order INTEGER, UNIQUE(type_id, key)
);
CREATE TABLE objects (
  id TEXT PRIMARY KEY, title TEXT, summary TEXT,
  parent_id TEXT REFERENCES objects(id),      -- one "home" in the hierarchy (navigation)
  sort_key TEXT,                              -- fractional index for ordering siblings
  props_json TEXT NOT NULL DEFAULT '{}',      -- field values keyed by field key
  created_at INTEGER, updated_at INTEGER, captured_at INTEGER, event_start INTEGER, event_end INTEGER,
  geo_lat REAL, geo_lon REAL, deleted_at INTEGER, sensitivity TEXT DEFAULT 'normal'
);
CREATE TABLE object_types (object_id TEXT, type_id TEXT, PRIMARY KEY(object_id, type_id)); -- multi-typing
CREATE TABLE blocks (                          -- rich content tree inside an object
  id TEXT PRIMARY KEY, object_id TEXT REFERENCES objects(id), parent_block_id TEXT,
  sort_key TEXT, kind TEXT,  -- paragraph, heading, list, todo, code, quote, table, embed, transclusion, view
  content_json TEXT, created_at INTEGER, updated_at INTEGER
);
CREATE TABLE relations (                       -- typed, directed edges; backlinks = reverse index
  id TEXT PRIMARY KEY, src_id TEXT NOT NULL, dst_id TEXT NOT NULL,
  rel_type TEXT NOT NULL,                     -- 'links_to','author_of','part_of', or user-defined
  field_id TEXT,                              -- set when the edge comes from a relation field
  src_block_id TEXT, label TEXT, props_json TEXT, created_at INTEGER, deleted_at INTEGER
);
CREATE INDEX rel_src ON relations(src_id, rel_type); CREATE INDEX rel_dst ON relations(dst_id, rel_type);
CREATE TABLE blobs (
  hash TEXT PRIMARY KEY,                      -- 'blake3:<hex>'
  size INTEGER, mime TEXT, encrypted INTEGER, created_at INTEGER
);
CREATE TABLE attachments (object_id TEXT, blob_hash TEXT, role TEXT, filename TEXT,
  PRIMARY KEY(object_id, blob_hash, role));   -- role: original, thumbnail, transcript, ocr_text, snapshot
CREATE TABLE provenance (
  id TEXT PRIMARY KEY, object_id TEXT, source_kind TEXT,  -- manual, import:<importer>, clip, feed, ai:<model>, plugin:<id>
  source_uri TEXT, import_run_id TEXT, original_blob TEXT, extractor TEXT, extractor_version TEXT,
  confidence REAL, accepted INTEGER DEFAULT 1, created_at INTEGER
);
CREATE TABLE views (                           -- saved lens = query + renderer + encoding
  id TEXT PRIMARY KEY, name TEXT, renderer TEXT,  -- table, kanban, mindmap, canvas, chart, map ...
  query_json TEXT, config_json TEXT,          -- e.g., a Vega-Lite spec, group-by, layout positions
  owner_object_id TEXT, created_at INTEGER, updated_at INTEGER
);
CREATE TABLE canvas_items (view_id TEXT, item_id TEXT, object_id TEXT, x REAL, y REAL, w REAL, h REAL,
  z INTEGER, style_json TEXT, PRIMARY KEY(view_id, item_id));
CREATE TABLE ops (                             -- append-only operation log: history, undo, sync, audit
  seq INTEGER PRIMARY KEY AUTOINCREMENT, op_id TEXT UNIQUE, actor_id TEXT, device_id TEXT,
  ts INTEGER, kind TEXT, target_id TEXT, payload_json TEXT, prev_hash TEXT, hash TEXT, signature TEXT
);
CREATE TABLE spaces (id TEXT PRIMARY KEY, name TEXT, ai_policy TEXT DEFAULT 'none', settings_json TEXT);
CREATE TABLE ai_audit (id TEXT PRIMARY KEY, ts INTEGER, provider TEXT, model TEXT, purpose TEXT,
  object_ids_json TEXT, bytes_sent INTEGER, user_initiated INTEGER);
CREATE TABLE mirror_state (                    -- two-way files folder bookkeeping
  object_id TEXT PRIMARY KEY, rel_path TEXT UNIQUE,
  last_hash TEXT,                             -- hash of the file content last written or read
  base_text TEXT,                             -- last-synced Markdown = common ancestor for 3-way merges
  synced_seq INTEGER, synced_at INTEGER       -- ops.seq at last sync
);
CREATE TABLE conflicts (                       -- nothing is ever silently dropped; the user resolves these
  id TEXT PRIMARY KEY, object_id TEXT,
  kind TEXT,                                  -- edit_edit, parse_error, bulk_change_hold, missing_id, cloud_conflict_file
  details_json TEXT, conflict_copy_id TEXT, created_at INTEGER, resolved_at INTEGER
);
CREATE VIRTUAL TABLE fts USING fts5(object_id UNINDEXED, title, body, props_text, tokenize='unicode61 remove_diacritics 2');
-- vectors (only if an embedding provider is enabled):
-- CREATE VIRTUAL TABLE vec_chunks USING vec0(chunk_id TEXT PRIMARY KEY, embedding float[<dim>]);
```

**Why this shape.**
- `objects` + `object_types` + `fields` gives Tana-style composable types and Anytype-style relations without changing the database schema when users add types.
- `parent_id` keeps a navigable home, as the Bergman findings call for.
- `relations` is the graph, and it also powers backlinks and the mind map.
- `ops` gives Perkeep-style history and provenance and is the future sync substrate.
- `provenance` makes lifetime ingestion trustworthy.
- `views` makes "many lenses" a first-class, exportable thing.
- `mirror_state` and `conflicts` make the two-way files folder safe. Every outside edit is merged against a known common ancestor, and anything that can't merge cleanly is kept as a conflict copy.
- Field values live in `props_json` for flexibility, with generated or indexed columns created on demand for hot fields.

**Built-in system types (editable, deletable where safe):** Note, Page, Task, Person, Organization, Place, Event, Project, Area, Resource, Book/Media, Bookmark/WebClip, Document, Image, Audio, Video, Email, Message, Contact, Dataset (wraps a CSV/sheet whose rows can become objects), Tag, Collection, Template, Highlight/Annotation, Journal (daily note).

### 3.4 Key subsystems

- **Query DSL (JSON, also typed as text).** `{ from: {types:[…], space, under}, where: [{field, op, value}], search: "…", relatedTo: {id, rel, depth}, sort, group, limit }`. It compiles to SQL and is shared by views, the MCP tools, AI natural-language querying and plugins.
- **View engine.**
  - A renderer registry: `{id, name, supports(dataShape) → score, component, configSchema}`.
  - The "Recommend views" panel scores every renderer for the current result set (field kinds, cardinality, presence of dates/geo/hierarchy/relations) plus the user's stated goal. Ranking follows the Cleveland–McGill and Draco-style rules.
  - Example rules: a hierarchy suggests outline, mind map or treemap; date fields suggest timeline or calendar; geo suggests map; one quantitative field across categories suggests a bar or dot chart; time × quantity suggests a line chart; two quantities suggest a scatterplot; flows suggest Sankey; relation-heavy data suggests a graph.
- **Ingestion contract.** `Importer { id, accepts(mime, ext, sniff) → bool, extract(blob, ctx) → ExtractResult { objects[], relations[], blocks[], derivedBlobs[], provenance } }`. Built-in Rust importers handle the common formats. Sidecar importers (Docling, whisper.cpp) are discovered and optional. Each run is an `import_run` with a dry-run preview, dedupe report and undo.
- **AI connector.** `Provider { id, kind: openai_compat|anthropic|ollama|…, baseUrl, models[], capabilities }`. Every call goes through a **policy gate**: AI enabled? Collection policy allows this provider? Sensitive fields redacted? Is the call written to `ai_audit`?
- **Plugin API surface (v1).** `objects.query/get/create/update`, `relations.*`, `blobs.read` (scoped), `ui.registerView/command/panel/importer/exporter/formulaFunction`, `events.subscribe`, `net.fetch` (allow-listed). The manifest declares its capabilities, and the user approves them.
- **Files mirror engine (two-way, MVP).** The rules that make editing from both sides safe:
  - **Writing out.** App changes are written within about 1 second (debounced) and atomically: write a temp file, then rename. The engine records each written file's hash so the watcher ignores the app's own writes, which prevents echo loops.
  - **Identity.** The `id:` in frontmatter is authoritative. Renaming or moving a file in another tool renames or reparents the object, and moving a folder reparents its whole subtree. If someone strips the id, the engine matches by content hash within the same batch of changes and otherwise treats the file as new.
  - **Reading in.** A watcher (the `notify` crate) debounces changes, parses the file, diffs it against `base_text`, and applies the result as normal ops with provenance `file:external`. Undo and history work exactly as for in-app edits.
  - **Merging.** If both sides changed since the last sync, the engine merges three ways: frontmatter field by field, and body text with a diff3-style merge. On a true conflict the app's version stays in place, the file's version is saved as a separate *conflict copy* object, and both are listed in a Conflicts inbox. **No edit is ever silently lost.**
  - **New and dropped files.** A new `.md` becomes a new object, typed from its frontmatter or as a Note. Any other file dropped into the folder goes through the ingestion pipeline, so the files folder doubles as a drop zone.
  - **Deletes.** A deleted file moves its object to the Trash, where it can be recovered. Nothing is ever hard-deleted.
  - **Bulk-change guard.** If one batch deletes or rewrites more than a threshold (default: 50 files or 10% of the folder), the engine pauses and asks. This protects against cloud-drive glitches, a `git checkout`, or an accidental folder wipe.
  - **Bad files.** Unparseable frontmatter is never applied. The file is left untouched and flagged in the Conflicts inbox.
  - **Cloud folders.** If the files folder sits inside Dropbox, iCloud, OneDrive or Syncthing, their conflict files (such as "conflicted copy") are recognised and routed to the Conflicts inbox.
  - **Obsidian-compatible.** Wikilinks, `#tags`, frontmatter properties and `^block-id` anchors follow Obsidian's conventions. The files folder opens as an Obsidian vault, and edits made there flow back.
  - **Database-only data.** Canvas layouts, view configs, the op log and the AI audit stay in the database. They are exported as read-only JSON in the MVP, and two-way JSON/CSV can come later.
  - **Scale and scope.** The mirror can be limited to chosen spaces. The first write of a large vault runs in the background with progress.
- **Mobile constraints (enforced from M1 so the full mobile app at M14 is a port, not a rewrite):**
  - Core crates compile for iOS and Android targets in CI.
  - Every view has a touch- and phone-width layout.
  - Desktop-only pieces (Python sidecar, ffmpeg, file watching) sit behind traits with mobile fallbacks or "defer to a desktop" job queues.
  - iOS forbids JIT compilation, so the plugin runtime sits behind a trait. Before M14, confirm whether the WebAssembly runtime can use an interpreter backend there (for example Wasmtime's Pulley interpreter or wasmi).
  - Local AI on phones can use a model server elsewhere on the user's own network.

---

## 4. Tech Stack Decision (with alternatives)

| Layer | Choice | Alternatives considered | Why |
|---|---|---|---|
| Shell | **Tauri 2**: desktop first, then the full iOS/Android app from the same codebase | Electron; native per-OS; local server + browser only | Small and lean, least-privilege, real mobile targets, Rust core. Server mode covers the browser use case. |
| Core language | **Rust** | TypeScript (Node), Go | Safety, performance, one core for desktop, server, CLI and mobile. Loro, Extism and SQLite ecosystems. |
| UI | **React + TypeScript + Vite**; Zustand/TanStack Query; Radix UI primitives; Tailwind or CSS vars | Svelte, Solid | Ecosystem for React Flow, Excalidraw, TanStack, TipTap. |
| Editor | **TipTap/ProseMirror** (block model mapped to `blocks`) | Lexical, BlockNote | Mature, extensible, CRDT bindings available. |
| DB | **SQLite (rusqlite, bundled)** + FTS5 + **sqlite-vec**; SQLite3MultipleCiphers for encryption | DuckDB (add later for analytics), LanceDB | LoC-recommended single file; hybrid search in one file. |
| Hashing | **BLAKE3** | SHA-256 | Fast. Record the algorithm in the hash prefix so it can change later. |
| Files mirror | **pulldown-cmark or comrak** (Markdown), a maintained YAML crate, **notify** (file watching), diff3-style 3-way merge | Hand-written parser | Mature Rust crates. Record each choice in an ADR, and don't use `serde_yaml`, which is no longer maintained. |
| Python sidecar | Private, version-pinned environment created on request with **uv** | System Python; bundled interpreter | Leaves the user's system untouched, reproducible, removable. |
| CRDT / sync (v2) | **Loro** behind a `SyncAdapter` trait | Yrs/Yjs, Automerge | Movable tree, history trimming, fast load. Yjs is the fallback. |
| Plugins | **Extism (Wasmtime)** + sandboxed iframes for UI | JS-in-process (Obsidian-style), Deno isolates | Real capability sandbox. |
| Visuals | React Flow, Excalidraw, Sigma.js/graphology, Cytoscape.js, Vega-Lite, ECharts, MapLibre, TanStack Table/Virtual | tldraw (licence key required), AG Grid | Licences and scale. |
| AI | OpenAI-compatible adapter + native adapters; **MCP server + client** (rmcp or official SDK) | LangChain-style frameworks | Thin, auditable, provider-neutral. |
| Packaging | Tauri bundler: MSI/NSIS, DMG (notarized), AppImage/deb/rpm/Flatpak; reproducible CI builds; signed updates (opt-in) | — | Cross-platform. |
| License | **AGPL-3.0 app; Apache-2.0 SDK + format spec** (decided) | MIT, MPL-2.0 | Copyleft protects a donation-funded project from closed forks. |
| Governance | **BDFL → steering group; DCO sign-off** (decided) | CLA | No relicensing is planned, so contributors keep their rights. |

### 4.1 Hardware and platform targets (decided)

| Tier | Target |
|---|---|
| **Desktop minimum** (also the reference machine for every performance budget) | 64-bit, 4-core CPU (x86-64 or ARM64, roughly 2018 or newer), 8 GB RAM, SSD, 2 GB free disk plus room for your data. Windows 10 22H2 or 11; the three most recent major macOS versions (Intel and Apple Silicon); Linux with WebKitGTK 4.1 (e.g., Ubuntu 22.04+, Debian 12+). |
| **Desktop recommended** | 16 GB RAM. |
| **Local AI (optional)** | 16 GB RAM for small local models (about 3–8B parameters, 4-bit quantized). 32 GB+ unified memory (Apple Silicon) or a GPU with 8 GB+ VRAM for comfortable use. Whisper base/small transcription runs on the minimum machine, just slowly for long recordings. Machines below the AI tier can point the app at a model server elsewhere on the user's own network. |
| **Mobile (M14)** | Current and two previous major iOS versions; Android 10+; 4 GB RAM. |

**Performance budgets** (on the desktop minimum machine; enforced by benchmarks in CI):

| What | Budget |
|---|---|
| Cold start to usable, 100k objects | < 2 s |
| Typing latency | < 16 ms (one frame) |
| Keyword search | < 50 ms at 100k objects; < 300 ms at 1M |
| Idle memory | < 300 MB |
| Table scroll | 100k rows at 60 fps |
| Mind map | 2,000 nodes interactive |
| Graph view | 100k nodes (WebGL) |
| Files folder round-trip | < 2 s each way |
| Lifetime scale | 1M objects and 1 TB of blobs work (slower, never broken) |

---

## 5. Feature Map (progressive disclosure built in)

**Modes (presets of switched-on features, changeable any time, the same on desktop and mobile):**
- **Simple:** notes, folders, search, daily journal, mind map, gallery. Types are hidden; everything is a "Note" or "File".
- **Organizer:** adds types and fields, tables, kanban, calendar, templates, tags, PARA/Johnny.Decimal starter kits.
- **Researcher:** adds web clipper, PDF annotation, citations (CSL), graph view, Zettelkasten kit, spaced review.
- **Data/Power:** adds charts and dashboards, formulas, query DSL, scripting and plugins, importers for takeouts/email/health, MCP.

| Stage | Features |
|---|---|
| **Core / MVP (0.1)** | Vault create/open; objects, types, fields (core kinds), relations, backlinks, hierarchy; block editor (Markdown-compatible); views: Document, Outline, Table, **Mind map**, Gallery, all usable at phone width with touch; FTS5 search + filters; command palette; **two-way files folder** (Markdown replica, file watcher, 3-way merge, Conflicts inbox, bulk-change guard, Obsidian-compatible); import Markdown folders (incl. Obsidian vaults), CSV/XLSX (rows → objects), PDFs (text), images (EXIF + thumbnails), drag-and-drop any file; content-addressed blobs with dedupe; op log with history and undo; JSON/CSV export + full export/import round-trip; light/dark themes; Simple and Organizer modes; onboarding (files-folder location, opt-in update checks); *Copy diagnostic report*; settings framework. |
| **v1** | Canvas (Excalidraw + object cards); Kanban, Calendar, Timeline, Map views; Charts (Vega-Lite) + dashboards + view recommender; graph view (Sigma.js); templates; formulas and rollups; saved views everywhere; web clipper extension; OCR; email import (mbox/.eml); ICS/vCard; RSS feeds; extra watched drop folders; vault encryption; backups (scheduled, versioned, to any folder/drive); keyboard remapping; theme editor; Researcher mode; accessibility pass. |
| **v2** | Plugin system (Extism + iframe UI) + SDK + registry index; AI connector (off by default, **chosen at onboarding**) with providers, embeddings, hybrid search, auto-tag/link/summarize/extract, NL query, AI audit log; MCP server + client; audio/video ingestion with local Whisper transcription; **one-click Docling sidecar**; Google Takeout, location, health, chat imports; IMAP live sync; resurfacing (on-this-day, FSRS review); Data/Power mode; server mode (browser UI on the LAN). |
| **v3: Everywhere** | Multi-device E2EE sync (Loro + LAN or self-hosted relay); **full mobile app** for iOS and Android (every view, offline vault on the device, works standalone or synced, heavy jobs handed to a desktop); shared spaces with roles and signed ops; real-time co-editing. |
| **Long-term** | Two-way JSON/CSV (schema, views, tables) in the files folder; publishing (static site export); DuckDB analytics; RDF/JSON-LD export and schema.org mappings; Draco plugin; "life timeline" and memory surfacing like Photos; format spec v1.0 frozen with a reference reader in two languages. |

---

## 6. Phased Build Roadmap with Acceptance Criteria

Milestones are renumbered from the first draft. The two-way files folder is its own milestone (M5), and sync, the full mobile app and sharing are now three separate milestones (M13–M15).

| Milestone | Scope | Acceptance criteria (all must pass in CI) |
|---|---|---|
| **M0 Foundations** | Monorepo, Tauri 2 shell, Rust workspace, React app, CI (lint, test, build on Win/macOS/Linux), ADR folder, FORMAT.md draft, LICENSE files, GOVERNANCE.md, CODE_OF_CONDUCT.md, CONTRIBUTING.md (DCO), FUNDING.yml, issue templates | App launches on all three OSes; CI green; DCO check blocks unsigned commits; ADR-0001…0008 recorded (stack, storage, IDs, licence, plugin model, two-way files folder, governance/DCO, platform targets). |
| **M1 Vault and object core** | SQLite schema + migrations, object/type/field/relation services, op log, blob store, typed IPC | ≥90% coverage on core crates; create/edit/delete/undo 10k objects in < 2 s; blob dedupe proven by test; crash-safety test (kill mid-write → vault opens, no corruption); **core crates cross-compile for iOS and Android targets**. |
| **M2 Editor and basic views** | Block editor, Document, Outline, Table (virtualized), Gallery; hierarchy sidebar; backlinks | Edit in one view and see it live in the others; 100k-row table at 60 fps on the minimum machine; keyboard-only operation of everything; **every view passes e2e tests at 360-px width with touch input**. |
| **M3 Mind map** | React Flow mind map over hierarchy and relations; editing writes back; auto-layout; touch gestures | Create/move/reparent updates `objects`/`relations`; round-trips with Outline; 2,000-node map interactive. |
| **M4 Search and query** | FTS5 index, filters/facets, query DSL, saved views, command palette | < 50 ms search on 100k objects; DSL golden tests; saved views persist and export. |
| **M5 Two-way files folder and export** | Markdown serializer/parser, atomic writer, watcher, 3-way merge, Conflicts inbox, bulk-change guard, Trash, cloud-conflict detection; JSON/CSV export; full export/import | App edit appears in the file in < 2 s and an outside edit appears in the app in < 2 s; **property test: 10,000 random interleavings of app and file edits lose zero content** (every change is merged or kept as a conflict copy); no echo loops; deleting 50 files triggers the guard; renaming or moving files renames or reparents objects; the folder opens in Obsidian with working links (fixture check); rebuilding a vault from the files folder alone reproduces every object, field, relation and block. |
| **M6 Ingestion v0** | Importers: Markdown/Obsidian vault, CSV/XLSX, PDF text, images/EXIF; drag-and-drop any file; import runs with dry-run preview and undo | Obsidian fixture keeps its links and frontmatter; provenance on every imported object; malicious-file fixtures (zip bombs, malformed PDFs, path traversal) are rejected safely. |
| **M7 MVP release (0.1)** | Modes (Simple/Organizer), settings, onboarding, themes, docs site, signed installers, *Copy diagnostic report* | Scripted usability scenario (capture, organize, find, edit a file outside the app, export) passes; **zero network requests on first launch with default settings** (network test); docs and FORMAT.md v0.1 published. |
| **M8 Visual v1** | Canvas, Kanban, Calendar, Timeline, Map, Charts/Dashboards, view recommender, graph view | Recommender's top pick matches ≥80% of an expert-labelled fixture set; 100k-node graph renders; every view has an accessible alternative and a phone layout. |
| **M9 Capture v1 and security** | Web clipper, OCR, email files, ICS/vCard, RSS, extra drop folders, encryption, backups | Encrypted vault unreadable without the passphrase; files folder is off by default on encrypted vaults and warns when enabled; backup/restore test; clipper saves Readability + SingleFile snapshot. |
| **M10 Plugins** | Extism host behind a runtime trait, manifest permissions, iframe UI sandbox with CSP, SDK + 3 sample plugins | No network, file or scope access without a grant; no host-DOM access from UI plugins. |
| **M11 AI and MCP** | Provider adapters, policy gate, audit log, embeddings + hybrid RRF search, AI features as suggestions, MCP server/client, **onboarding AI question** (also shown once to existing users after upgrade) | With "No AI" chosen, zero AI traffic (network test); collection policies enforced; every AI call audited; MCP conformance against a reference client. |
| **M12 Rich media and archives** | whisper.cpp, ffmpeg, **one-click Docling sidecar**, Google Takeout, IMAP | 1-hour audio transcribed locally and searchable; Takeout sample imported with provenance; sidecar install never modifies the system Python and uninstalls cleanly; the app works fully with the sidecar absent. |
| **M13 Sync** | Loro sync adapter, E2EE over LAN or a self-hosted relay (binary + docs), device pairing | Two devices converge after offline edits (property test); relay stores only ciphertext; each device's files folder stays consistent. |
| **M14 Full mobile app** | iOS + Android builds; all views and modes; standalone or synced vault; "process on my desktop" job queue for sidecar work; plugins via an interpreter backend if feasible; local AI via the user's network | The M7 usability scenario passes on a phone and a tablet; offline phone edits sync with a desktop; mobile performance budgets met. |
| **M15 Shared spaces and co-editing** | Spaces with roles, member keys, signed ops, real-time co-editing | Role permissions enforced in tests; signatures verified on every op; two users co-edit a document without loss. |

---

## 7. Risks and Mitigations

- **Scope explosion (the biggest risk).** Mitigation: strict milestones, MVP first, importers and views as plugins behind stable contracts.
- **Two-way files folder losing or duplicating data** (echo loops, cloud-drive glitches, `git checkout`, editors that rewrite whole files). Mitigation: files treated as a replica going through the op log; 3-way merge against a stored common ancestor; conflict copies instead of overwrites; Trash instead of deletes; bulk-change guard; large property-test and fuzz suites in M5. This is now the riskiest MVP component, so M5 has the toughest acceptance tests.
- **Mobile as a full app.** iOS forbids JIT, sidecars can't run on phones, and screens are small. Mitigation: mobile constraints enforced from M1/M2 (cross-compile in CI, phone-width tests), sidecar work deferred to a desktop, plugin runtime behind a trait with an interpreter option verified before M14.
- **Webview inconsistency across OSes (Tauri).** Mitigation: a cross-OS visual regression suite (Playwright + WebDriver for Tauri), and avoiding bleeding-edge CSS and JS.
- **Performance at lifetime scale (millions of objects, terabytes of blobs).** Mitigation: the §4.1 budgets enforced in CI, SQL aggregation, virtualization, WebGL, lazy blob loading, background indexing with progress.
- **Format rot.** Mitigation: the files folder, FORMAT.md, versioned migrations, hash-algorithm prefixes, and avoiding exotic SQLite features in canonical tables.
- **Plugin ecosystem vs sandbox friction.** Mitigation: generous but explicit capabilities, a good SDK, and samples.
- **AI privacy incidents.** Mitigation: off by default, an explicit onboarding choice, policy gate, audit log, local-only mode, sensitive-field redaction.
- **Python sidecar packaging (Docling).** Mitigation: optional, one-click into a private pinned environment, built-in Rust parsers for the basics.
- **CRDT maturity and choice.** Mitigation: defer to M13, keep it behind a `SyncAdapter` trait, and benchmark with your own data before committing.
- **Sustainability on donations alone.** A volunteer, donation-funded project can stall if the founder burns out. Mitigation: a tightly scoped MVP, excellent docs and ADRs so others can pick up work, a published governance path to a steering group, and transparent finances (Open Collective).
- **No telemetry means less insight into bugs.** Mitigation: the *Copy diagnostic report* button, good issue templates, and broad automated test coverage.
- **Licence contamination.** Mitigation: a dependency licence check in CI (cargo-deny, a JS licence checker). tldraw is excluded unless licensed.

---

## 8. Decisions (resolved 25 October 2026)

| # | Question | Decision | What it changed in this plan |
|---|---|---|---|
| 1 | Licence | **AGPL-3.0** for the app; Apache-2.0 for the SDK and format spec | Confirmed as drafted. |
| 2 | Name | **DataBase.Me** | Renamed throughout; code identifiers use `dbme` because crate and npm names can't contain dots (§1). |
| 3 | Files mirror | **Two-way from the start** | New files-mirror engine with merge and safety rules (§3.4); `mirror_state` and `conflicts` tables (§3.3); own milestone M5 with the toughest tests; JSON/CSV stay export-only until later. |
| 4 | Python sidecar | **Approved** | One-click, private, version-pinned environment; still never required (§2.5, M12). |
| 5 | Mobile | **Full app** | Mobile constraints enforced from M1/M2; dedicated M14; plugin runtime behind a trait for iOS (§2.10, §3.4). |
| 6 | Hosted relay | **No; funded by donations** | No paid tiers or hosted services; self-hosted relay only; FUNDING.yml; sustainability risk added (§2.10, §7). |
| 7 | AI at first run | **Ask at onboarding** | Onboarding screen with "No AI" as the highlighted default (§2.9, M11). |
| 8 | Telemetry | **None; issues go to GitHub** | No crash reporter; update checks opt-in; *Copy diagnostic report* button; zero-network first-launch test (§2.11, M7). |
| 9 | Governance | **Delegated → BDFL now, steering group at 3+ maintainers; DCO, not CLA** | GOVERNANCE.md and a DCO CI check in M0; plain-language explanation in §2.10. |
| 10 | Minimum hardware | **Delegated → 8 GB / 4-core / SSD desktop minimum; 16 GB for local AI** | New §4.1 with platform targets and performance budgets that every milestone measures against. |

**Still open, but not blocking the build:** trademark/domain check for "DataBase.Me", and which donation platform(s) to list.

---

## 9. HANDOFF PROMPT (copy-paste into the cloud coding session)

The same prompt is also delivered as a separate file, `DataBase.Me_Handoff_Prompt.md`, for easy copying.

```
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
```

## Sources

1. [Integrating sqlite-vec with FTS5 for Combined Search Capabilities · Issue #48 · asg017/sqlite-vec · GitHub](https://github.com/asg017/sqlite-vec/issues/48)
2. [Simon Willison on full-text-search](https://simonwillison.net/tags/full-text-search/)
3. [(PDF) Improved Search Engines and Navigation Preference in Personal Information Management](https://www.researchgate.net/publication/220515581_Improved_Search_Engines_and_Navigation_Preference_in_Personal_Information_Management)
4. [Full article: File hyper-searching explained](https://www.tandfonline.com/doi/full/10.1080/07370024.2024.2379838)
5. [The effect of folder structure on personal file navigation](https://www.semanticscholar.org/paper/The-effect-of-folder-structure-on-personal-file-Bergman-Whittaker/667fa2d18d25c4866c739e936469d25a2d38ddc1)
6. [Learning With Concept and Knowledge Maps: A Meta-Analysis](http://www.sfu.ca/~jcnesbit/articles/nesbitadesope2006.htm)
7. [ERIC - EJ1179084 - Studying and Constructing Concept Maps: A Meta-Analysis, Educational Psychology Review, 2018-Jun](https://eric.ed.gov/?id=EJ1179084)
8. [Ziele und M\\"oglichkeiten des Einsatzes von Knowledge-Maps in mathematik-historischen Veranstaltungen](https://arxiv.org/pdf/1912.08088)
9. [Graphical perception · The Explanation Lab](https://theexplanationlab.com/lessons/cleveland/)
10. [William S. Cleveland & Robert McGill (1984)](https://www.textbookofusability.com/references/cleveland1984.html)
11. [Obsidian October 2025 - Obsidian](https://obsidian.md/blog/2025-obsidian-october/)
12. [Obsidian 1.9.0 Desktop (Early access) - Obsidian](https://obsidian.md/changelog/2025-05-21-desktop-v1.9.0/)
13. [Obsidian on X: "Introducing Bases, a new core plugin that lets you turn any set of notes into a powerful database. Now available to everyone with Obsidian 1.9! https://t.co/PXbwzAeO8T" / X](https://x.com/obsdmd/status/1957541774249128122)
14. [obsidian releases](https://github.com/kpaede/obsidian-releases)
15. [anpigon's picture](https://huggingface.co/spaces/anpigon/obsidian-qa-bot/blob/84af0cbfd03597c4f288de37a1443f88c2861107/docs/obsidian-help/Extending%20Obsidian/Plugin%20security.md)
16. [Releases · logseq/logseq · GitHub](https://github.com/logseq/logseq/releases)
17. [Logseq Ships Its Long-Awaited 2.0 Beta, Moving Notes From Markdown Files to a Database — Kompozy · Kompozy](https://kompozy.io/news/logseq-2-0-db-version-beta)
18. [Logseq CVEs and Security Vulnerabilities - OpenCVE](https://app.opencve.io/cve/?vendor=logseq)
19. [Anytype: From Second Brain to Social Brain with Types and Graphs](https://volodymyrpavlyshyn.medium.com/anytype-from-second-brain-to-social-brain-with-types-and-graphs-e6eb6611ec7d)
20. [Object Types and Relations](https://deepwiki.com/anyproto/anytype-heart/3.1-object-types-and-relations)
21. [Capacities vs Tana: which should you choose in 2026?](https://fabric.so/comparison/capacities-vs-tana)
22. [Tana vs Capacities: supertags or objects in 2026?](https://dexi.net/compare/tana-vs-capacities)
23. [Switching from Tana to Capacities - Capacities Documentation](https://docs.capacities.io/migration/switching-from-tana)
24. [Permanodes - Perkeep](https://perkeep.org/doc/schema/permanode)
25. [Schema - Perkeep](https://perkeep.org/doc/schema/)
26. [perkeep/doc/overview.txt at master · perkeep/perkeep · GitHub](https://github.com/perkeep/perkeep/blob/master/doc/overview.txt)
27. [tldraw vs Excalidraw vs Miro: Best Open Source Whiteboard in 2026](https://meetrix.io/blogs/open-source-whiteboard-tools/)
28. [Open-source tldraw alternatives in 2026 — Quickdraw](https://tryquickdraw.com/blog/open-source-tldraw-alternatives)
29. [Extension runtime security](https://code.visualstudio.com/docs/configure/extensions/extension-runtime-security)
30. [Types](https://doc.anytype.io/anytype/organize/types)
31. [Supertags - Tana Outliner Learn](https://tana.inc/docs/supertags)
32. [Tana Structured Notes Advanced: Building a Personal Knowledge Graph with Supertags — DigiToolBook Blog](https://digitoolbook.com/en/blog/tana-ai-structured-notes)
33. [Keeping things in Perkeep](https://dee.underscore.world/blog/keeping-things-in-perkeep/)
34. [What are the seven ideals of local-first software?](https://www.offlineprotocol.com/learning/local-first/seven-ideals-of-local-first-software)
35. [The Local-First Software Paradigm and Core Ideals](https://local-first.com/articles/local-first-paradigm/)
36. [Local-first software](https://en.wikipedia.org/wiki/Local-first_software)
37. [LoC Recommended Storage Format](https://www.sqlite.org/locrsf.html)
38. [Long Term Support](https://sqlite.org/lts.html)
39. [Native Benchmarks](https://loro.dev/docs/performance/native)
40. [M1.5: CRDT benchmark (Loro vs Yrs vs Automerge), ADR-0009 confirmed by giosuel · Pull Request #16 · giosuel/imaginarium · GitHub](https://github.com/giosuel/imaginarium/pull/16)
41. [Yjs vs Automerge vs Loro: CRDT Libraries 2026 — PkgPulse Guides](https://www.pkgpulse.com/guides/yjs-vs-automerge-vs-loro-crdt-libraries-2026)
42. [CRDT Applications (Yjs, Automerge, Local-First Software) - The HLD Handbook](https://hld.handbook.academy/curriculum/architecture-patterns/crdt-applications/)
43. [Docling Technical Report](https://arxiv.org/pdf/2408.09869)
44. [Docling Review: What IBM's Document-to-Markdown Converter Ac](https://thunderbit.com/blog/docling-review)
45. [Docling Documentation - IBM Docling Technical Manual ...](https://docling.org/doc/)
46. [NB-M2-08: Hybrid RRF Retrieval Engine (SQLite FTS5 + sqlite-vec) · Issue #15 · harzhtiwari/Notebench · GitHub](https://github.com/harzhtiwari/Notebench/issues/15)
47. [SQLite is Enough. Lexical, Semantic, and Hybrid Search with scrydb](https://arxiv.org/pdf/2608.24060)
48. [Hybrid Search: Smart Search Architecture with FTS5 + Vector + RRF](https://ceaksan.com/en/hybrid-search-fts5-vector-rrf)
49. [Retrieval-Enhanced Named Entity Recognition](https://arxiv.org/pdf/2410.13118)
50. [Changes in 23.10 - Anki Changes - AnkiWeb](https://changes.ankiweb.net/changes/23.10.html)
51. [Anki](https://en.wikipedia.org/wiki/Anki)
52. [Allen School News Archive » Draco, a constraint-based model for formalizing principles of good visualization design, earns Best Paper at InfoVis 2018](https://news.cs.washington.edu/2018/11/05/draco-a-constraint-based-model-for-formalizing-principles-of-good-visualization-design-earns-best-paper-at-infovis-2018/)
53. [Draco: Formalizing Visualization Design Knowledge as Constraints](https://idl.uw.edu/draco)
54. [License key • tldraw Docs](https://tldraw.dev/sdk-features/license-key)
55. [Obsidian plugin was abused to deploy a remote access trojan](https://news.ycombinator.com/item?id=48088576)
56. [Obsidian Shell Commands Abuse Shows a New Malware Playbook](https://www.penligent.ai/hackinglabs/obsidian-shell-commands-abuse-shows-a-new-malware-playbook/)
57. [Our Approach to Extensibility - vscode-docs](https://vscode-docs.readthedocs.io/en/stable/extensions/our-approach/)
58. [Logseq security issues in 0.10.15 · Issue #530323 · NixOS/nixpkgs · GitHub](https://github.com/nixos/nixpkgs/issues/530323)
59. [CVE-2026-47901: Logseq Sandbox Escape XSS Vulnerability](https://www.sentinelone.com/vulnerability-database/cve-2026-47901/)
60. [plugins: WASM host runtime, package format, and capability policy · Issue #798 · cyrex562/calibre-oxide · GitHub](https://github.com/cyrex562/calibre-oxide/issues/798)
61. [Security - Wasmtime](https://docs.wasmtime.dev/security.html)
62. [extism - crates.io: Rust Package Registry](https://crates.io/crates/extism)
63. [The Manifest](https://extism.org/docs/concepts/manifest/)
64. [Model Context Protocol Specification Version Timeline - Version-by-Version Changes and Adoption Milestones](https://hidekazu-konishi.com/entry/mcp_specification_version_timeline.html)
65. [MCP 2025-11-25 is here: async Tasks, better OAuth, extensions, and a smoother agentic future — WorkOS](https://workos.com/blog/mcp-2025-11-25-spec-update)
66. [Model Context Protocol](https://en.wikipedia.org/wiki/Model_Context_Protocol)
67. [Specification](https://modelcontextprotocol.info/specification/)
68. [Tauri vs. Electron: performance, bundle size, and the real trade-offs](https://www.gethopp.app/blog/tauri-vs-electron)
69. [Tauri vs Electron 2026: 20-50x Smaller, 5x Less RAM](https://rustify.rs/articles/rust-tauri-vs-electron-2026)
70. [Tauri vs Electron \[2026\]: 96% Smaller Apps, 1 Winner](https://tech-insider.org/tauri-vs-electron-2026/)
71. [Electron vs Tauri 2026: Bundle Size, RAM, Security and Team Fit — PkgPulse Guides](https://www.pkgpulse.com/guides/electron-vs-tauri-2026)
72. [WxSQLite3](https://en.wikipedia.org/wiki/WxSQLite3)
