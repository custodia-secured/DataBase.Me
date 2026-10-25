# ADR-0001: Technology stack and identifiers

- **Status:** Accepted
- **Date:** 2026-10-25
- **Milestone:** M0

## Context

DataBase.Me has to run on Windows, macOS and Linux first and become a full iOS and Android app at
M14 from the same code. It must stay small and fast on an 8 GB, 4-core machine (ADR-0008), keep a
single core that also serves a CLI and a later server mode, and use the largest ecosystem of view
libraries we need (React Flow, Excalidraw, TanStack, TipTap). The research and alternatives are in
the build plan, §2.10 and §4.

## Decision

**Names.** The product is "DataBase.Me". Crate and npm names cannot contain dots, so code uses
`dbme`: crates `dbme-core`, `dbme-cli` and so on, the CLI `dbme`, vault folders `*.dbme`, and the
npm scope `@databaseme`. The repository is `DataBase.Me`. The app identifier is
`org.databaseme.desktop`; it is **provisional** until the trademark and domain check (build plan
§8) and must be final before the first release (M7), because it decides where each OS keeps the
app's data.

**Shell.** Tauri 2 (stable 2.x). Tauri 3 is in alpha and is not considered until it is stable.

**Core.** A Rust workspace (edition 2024). The compiler is pinned in `rust-toolchain.toml` (1.99.1
at M0) so that local builds and CI see the same lints; bumping it is its own pull request.

**UI.** React 19, TypeScript and Vite 8. Radix UI primitives for accessible components, Zustand for
UI state and TanStack Query for data from the core are added when the first real views need them
(M2). Theming is CSS variables (design tokens) in `packages/ui`.

**TypeScript 6.0, not 7.0.** TypeScript 7 (the native compiler) is out, but typescript-eslint 8
supports only TypeScript below 6.1, and type-aware linting is worth more to us than compiler speed
today. We move when typescript-eslint supports TypeScript 7.

**Package manager.** pnpm 12 through Corepack (the `packageManager` field). It gives strict
dependency resolution, workspaces and a built-in `licenses` command that the licence check uses.

**Editor.** TipTap/ProseMirror mapped onto the block tree (M2).

**Views.** React Flow (`@xyflow/react`) with ELK or dagre for the mind map and node editing;
Excalidraw for the freeform canvas; Sigma.js with graphology for the large WebGL graph (Cytoscape.js
optional); Vega-Lite as the serializable chart spec and Apache ECharts for heavy dashboards; MapLibre
GL for maps; TanStack Table and TanStack Virtual for grids. **tldraw is excluded**: its SDK needs a
licence key in production.

**IPC types** are generated from Rust (ts-rs or tauri-specta); the choice is an M1 ADR.

**Tests.** Vitest for TypeScript, `cargo test` and proptest for Rust, criterion for Rust
benchmarks, Playwright or tauri-driver for end-to-end tests (from M2).

## Consequences

- Tauri renders with each OS's own webview (WebView2, WebKit, WebKitGTK), so rendering differs by
  platform. CI builds and launches the app on all three (ADR-0008), and visual regression tests
  arrive with the views.
- The Rust core is shared by the desktop app, the CLI and later the server mode and mobile app.
- Pinning the compiler means a deliberate upgrade step; in exchange, clippy never breaks CI on its
  own.

## Alternatives considered

- **Electron.** One rendering engine everywhere, but in the comparison the build plan cites (§2.10)
  about 28 times the bundle size and 2.4 times the memory, no mobile target, and a Node process
  with full privileges.
- **Native UI per OS.** Best fidelity, but several UIs to build, which a volunteer project cannot
  afford.
- **Svelte or Solid.** Lighter, but the view libraries we need are React-first.
- **TypeScript 7.** Faster type checking, but it breaks type-aware ESLint today.
- **npm or Yarn.** Workable; pnpm's strictness catches undeclared dependencies, which the licence
  policy relies on.
