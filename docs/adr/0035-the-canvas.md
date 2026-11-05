# ADR-0035: The canvas: Excalidraw, with no network

- **Status:** Proposed
- **Date:** 2026-11-04
- **Milestone:** M8

## Context

The brief names Excalidraw (MIT) for the freeform canvas and rules out tldraw. The app promises no
network request unless a feature the person turned on needs one (ADR-0010, ADR-0028), and its CSP is
strict and static. M8's gate 1d put Excalidraw 0.18.1 on a page with that CSP in Edge and WebKit, at
desktop and phone sizes. Out of the box it did two things the app cannot accept:

- Its font loader always adds a URL on a public CDN (`esm.sh`) after the path the app gives it, for
  every font face: 230 CSP reports on one load. The CSP stopped every one of them, so no request
  left the machine, but the app would be attempting them.
- It brings a large tree for one optional feature, the Mermaid importer: Mermaid, KaTeX, Cytoscape
  and D3, about 23 MB of the e2e build with the fonts.

Otherwise it drew and reported its elements with no violation of `style-src 'self'`, no worker
problem and no uncaught error.

## Decision

Excalidraw stays, pinned to an exact version, with a Vite plugin in the app
(`apps/desktop/vite-plugin-excalidraw.ts`) that:

1. **Points the font fallback at the app's own copy.** The build fails if the plugin cannot find the
   fallback in a build that loads Excalidraw, so an upgrade cannot bring it back unnoticed.
2. **Copies the fonts into the build, without Xiaolai.** Xiaolai, the CJK face, is 13 MB of the
   13.5 MB. CJK text on a canvas falls back to the system's fonts.
3. **Keeps the Mermaid importer out.** `pnpm-workspace.yaml` overrides it with an empty package in
   `patches/no-mermaid`, so Mermaid, KaTeX, Cytoscape and what they bring are not even installed, and
   the app does not offer the dialog. (The first run on CI found one of them, `khroma`, with a licence
   `pnpm` reads as "Unknown" there: a package the app did not ship was failing its check.)

With these, the e2e build goes from 32 MB to 9 MB, the gate records no CSP report, and no file in the
build names the CDN.

**Licences (ADR-0009).** `fractional-indexing` is CC0-1.0, a public-domain dedication with a
permissive fallback, and not OSI-approved. It is a small file Excalidraw uses to order elements,
and it is granted as a per-package exception, as `notify` was (ADR-0024). The fonts are understood
to be OFL-1.1, Comic Shanns MIT. The package carries no licence files for them and sits under an MIT
field, so no check sees them. **Not yet verified:** M8's canvas task reads each font's own licence
record and keeps the licence texts with the app before any of them ships.

## Consequences

- CJK handwriting style is lost on a canvas, and text there looks like the system's font.
- The app carries a small transform of a dependency's bundled code, which the build itself guards.
- Excalidraw's menu entries that lead elsewhere (its library browser, its links, the embeds of web
  pages) are refused by the CSP if used. The canvas task must hide them, and its e2e test opens each
  menu to confirm that nothing tries.
- `@radix-ui` 1.0, which Excalidraw pins, declares peers up to React 18 while the app runs React 19.
  It works in the gate; it is a warning at install and a risk at the next React upgrade.

## Alternatives considered

- **Draw the canvas with React Flow (already here) and `perfect-freehand`.** Much smaller, and the
  stated fallback in the M8 plan if the gate failed. It did not fail, and the brief names Excalidraw.
- **`pnpm patch` instead of a plugin.** The code is one minified line of about 500 KB, so a patch
  file could not be reviewed.
- **Loosening the CSP for the CDN.** Against ADR-0028.
- **tldraw.** Production use needs a licence key, and the brief rules it out.
