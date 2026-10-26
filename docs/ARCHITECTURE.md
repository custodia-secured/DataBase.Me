# Architecture

DataBase.Me is one typed object graph, stored in a local SQLite vault and a content-addressed blob
folder, shown through many interchangeable views, and mirrored as plain Markdown files. This page
is the map; the decisions behind it are in [adr/](adr/), and the on-disk format is in
[FORMAT.md](FORMAT.md).

## Layers

```
┌────────────────────────────────────────────────────────────────────┐
│ UI (TypeScript/React in the Tauri webview; later mobile and server)│
│  Shell · Command palette · View host · Editor · Settings/Modes     │
│  Renderers: Doc · Outline · MindMap · Canvas · Graph · Table ·     │
│  Kanban · Calendar · Timeline · Gallery · Map · Chart · Dashboard  │
├──────────────── typed IPC (commands + event stream) ───────────────┤
│ Rust core                                                          │
│  Object, schema and relation services · Query engine (DSL → SQL)   │
│  Search (FTS5 + sqlite-vec + RRF) · Op log, history, undo          │
│  Provenance · Blob store (BLAKE3) · Ingestion · Files mirror        │
│  Export · Plugin host (Extism) · AI connector · MCP · Crypto · Sync │
├────────────────────────────────────────────────────────────────────┤
│ Disk: vault.sqlite · blobs/ · files/ · export/                     │
│ Optional sidecars: Docling (Python) · whisper.cpp · ffmpeg · OCR   │
└────────────────────────────────────────────────────────────────────┘
```

## How a change flows

1. A view calls a typed IPC command (for example "set field `due` on object X").
2. The core validates it, writes an op to the op log and updates the materialised tables in **one
   SQLite transaction**, then emits an event.
3. Every open view receives the event and re-reads what it shows, so all views stay consistent
   (ADR-0017).
4. The files mirror sees the new op and rewrites the object's Markdown file (debounced, atomic).
5. An edit made to that file in another tool travels the other way: watcher, parse, three-way merge
   against the last synced text, and new ops with provenance `file:external` (ADR-0006).

Undo, history, provenance and later sync all come from the op log (ADR-0002).

## Code map

| Path                         | Role                                                                             | Milestone         |
| ---------------------------- | -------------------------------------------------------------------------------- | ----------------- |
| `apps/desktop/`              | Tauri 2 app: Rust shell (`src-tauri/`) and the React UI                          | M0                |
| `crates/core/` (`dbme-core`) | Domain: objects, types, relations, ops, query DSL, policies. No I/O.             | M0, grows from M1 |
| `crates/store/`              | SQLite schema, migrations, FTS, vectors, blob store, crypto. All SQL lives here. | M1                |
| `crates/api/` (`dbme-api`)   | The commands the UI calls, as plain functions; the shell wraps them (ADR-0017)   | M2                |
| `crates/e2e-server/`         | Test only: serves the e2e UI build and runs `dbme-api` on real vaults (ADR-0018) | M2                |
| `crates/mirror/`             | Two-way files folder engine                                                      | M5                |
| `crates/ingest/`             | `Importer` trait, built-in importers, sidecar manager                            | M6                |
| `crates/export/`             | JSON/CSV export, full export and import                                          | M5                |
| `crates/plugins/`            | `PluginRuntime` trait and the Extism host                                        | M10               |
| `crates/ai/`                 | Provider adapters, policy gate, audit                                            | M11               |
| `crates/mcp/`                | MCP server and client                                                            | M11               |
| `crates/sync/`               | `SyncAdapter` and the Loro implementation                                        | M13               |
| `crates/cli/` (`dbme`)       | Command-line tool: import, export, verify, query, mirror-check                   | M0, grows         |
| `apps/clipper/`              | Browser extension (web clipper)                                                  | M9                |
| `apps/relay/`                | Self-hostable end-to-end-encrypted sync relay                                    | M13               |
| `packages/client/`           | Generated bindings, vault queries and the change listener (ADR-0014, ADR-0017)   | M2                |
| `packages/ui/`               | Design system: tokens, themes, shared components                                 | M0, grows         |
| `packages/views/`            | Renderer registry and every view, all responsive and touch-ready                 | M2                |
| `packages/recommender/`      | View and chart recommendation rules                                              | M8                |
| `packages/sdk/`              | Plugin SDK (Apache-2.0)                                                          | M10               |

Crates and packages are created when their milestone starts, not before.

## Boundaries that matter

- **Core crates stay portable.** `dbme-core` and `dbme-store` must compile for iOS and Android
  (checked in CI from M1). Anything desktop-only, such as file watching or sidecars, sits behind a
  trait (ADR-0008).
- **All SQL lives in `crates/store`**, as prepared statements.
- **The UI never touches the disk.** It talks to the core only through typed IPC commands, each
  granted explicitly to the windows that need it (ADR-0010).
- **Plugins never run in the app's process or DOM** (ADR-0005).
- **Nothing reaches the network** unless the user turns on a feature that needs it.
