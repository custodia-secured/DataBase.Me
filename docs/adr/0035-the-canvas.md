# ADR-0035: The canvas: Excalidraw, with no network

- **Status:** Proposed
- **Date:** 2026-11-05
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
3. **Copies the fonts' licences with them,** to `excalidraw-assets/NOTICE.txt`, from
   `apps/desktop/third-party/excalidraw-fonts-NOTICE.txt`.
4. **Keeps the Mermaid importer out.** `pnpm-workspace.yaml` overrides it with an empty package in
   `patches/no-mermaid`, so Mermaid, KaTeX, Cytoscape and what they bring are not even installed, and
   the app does not offer the dialog. (The first run on CI found one of them, `khroma`, with a licence
   `pnpm` reads as "Unknown" there: a package the app did not ship was failing its check.)

With these, the e2e build goes from 32 MB to 9 MB, the gate records no CSP report, and no file in the
build names the CDN.

**Licences (ADR-0009).** `fractional-indexing` is CC0-1.0, a public-domain dedication with a
permissive fallback, and not OSI-approved. It is a small file Excalidraw uses to order elements,
and it is granted as a per-package exception, as `notify` was (ADR-0024).

The package carries no licence files for its fonts and sits under an MIT field, so no check sees them.
So each font's own licence record was read, from the name table inside each WOFF2 file:

| Font          | What its file says                                                   | Ships as               |
| ------------- | -------------------------------------------------------------------- | ---------------------- |
| Assistant     | OFL-1.1, stated                                                      | OFL-1.1                |
| Virgil        | OFL-1.1, stated                                                      | OFL-1.1                |
| Comic Shanns  | MIT, stated                                                          | MIT                    |
| Nunito        | OFL-1.1, by its URL                                                  | OFL-1.1                |
| Lilita One    | OFL-1.1, by its URL                                                  | OFL-1.1                |
| Cascadia Code | A general Microsoft line and the OFL's URL; upstream's file: OFL-1.1 | OFL-1.1                |
| Excalifont    | "All rights reserved", and no licence at all                         | OFL-1.1: **see below** |
| Liberation    | Version 1.05, under the GPL with a font exception                    | Left out               |
| Xiaolai       | Not read: 13 MB, left out for its size                               | Left out               |

**Excalifont does not agree with itself.** Its file says "All rights reserved". Its publisher's page
says "Released under the OFL-1.1 license" and "(included in the font file)", which it is not. The app
ships it on that public statement, and the notice says exactly this. It is the font new text on a
canvas is drawn in, so the owner should see this, and the publisher could settle it with one line.

**Liberation is left out** because the only thing that asks for it is a server-side export of
Excalidraw's, which the app has no use for, and no one can choose it. Leaving it out avoids a GPL font
for nothing.

### How a canvas is kept

- **One row an element** (migration 5, FORMAT.md §3.1), through the op log like any other row, so a
  change is one undo, a vault replays to the same drawing and a full export carries it. The brief's
  draft columns are all there, plus `id`, since the log names a row by it, and the two timestamps every
  updated row has. `item_id` is Excalidraw's own name for the element, which other elements use to refer
  to it. `style_json` is everything else about it.
- **One canvas a page.** It is a view with the renderer `canvas`, owned by the page and made by the first
  thing put on it, so looking at an empty canvas writes nothing. It goes with its page, and undoing that
  brings it back. A saved dashboard cannot use one as a tile: it is not a query.
- **A card is an element with an `object_id`.** To Excalidraw it is an embeddable with the link
  `dbme:<id>`, which is never loaded: the app draws it, from a live read, so it follows its page's name.
  A card whose page is in the Trash says so and stays, as a restore brings it back to life. Deleting the
  page for good takes the card off every canvas, and undoing that puts it back. "Add card" offers what
  is under the page, up to 200, as every view shows one object and what belongs to it.
- **The app's Undo, not Excalidraw's.** Excalidraw's history knows nothing of the store, so its undo and
  redo actions are replaced with nothing, by name, and their keys go up to the app, and its buttons are
  hidden. A stroke, a drag or a text is written when the scene has been still for 250 ms and nothing is
  being drawn, as one change. Undo and leaving the view write what is in hand first.
- **The store is the truth, and what is being drawn is the person's.** Rows that differ replace the
  scene, unless a stroke is in hand, which goes first. `packages/views/src/canvas/sync.ts` holds this,
  with no library in it, and its tests are about the order things happen in. Three faults it was built
  through, each with a test now: Excalidraw empties its scene as it unmounts, which looked like everything
  being deleted, so the last write now runs before that and only an element marked deleted is ever removed;
  rows older than a stroke took it off the screen until the next read; and rows that say the same as before
  stay the same array, so an Undo that overtook the read after a stroke left that stroke on screen.
- **Pictures are not kept yet.** Their bytes belong in the blob store, with a link from the row that a
  replay and an export have to honour, and that is more than this task. A picture dropped on a canvas stays
  until the view closes, and the view says so. Pasting one is refused, with the same words. A web page in a
  frame is a request, and is not kept either.
- **Nothing on a canvas leads out.** The library, its link to a site of libraries, the embed tools, the
  links on shapes and Help's row of links are hidden, the export and open menus are off, and the main menu
  has Clear and Help. Finding text with Ctrl+F stays, in the sidebar the library shares. The link icon on
  a card opens the page. Hiding is CSS with `!important`, the one place the app uses it, because this has to
  hold through an upgrade; the CSP is still what stops a request, and the e2e fixtures fail on one.
- **The alternative.** "Show as list" has everything with its kind, position and size in the keyboard grid.
  Alt and an arrow move the chosen element 20 pixels, announced, and there are buttons for it too. Enter
  opens a card. It does not load Excalidraw.

## Consequences

- CJK handwriting style is lost on a canvas, and text there looks like the system's font.
- The app carries a small transform of a dependency's bundled code, which the build itself guards.
- **Migration 5 means 0.1 refuses a vault that has been opened by this version.** That is what a schema
  version is for (FORMAT.md §9), and 0.1 has not been published.
- A picture does not last, which a person can find surprising, so the view says so on the spot.
- What is hidden is hidden by Excalidraw's class names and test IDs. An upgrade that renames one brings the
  control back, and `canvas.spec.ts` is what notices.
- Every change the scene reports compares every element with what is stored. **Not measured** at thousands
  of elements. The brief sets no budget for a canvas.
- `@radix-ui` 1.0, which Excalidraw pins, declares peers up to React 18 while the app runs React 19.
  It works in the gate; it is a warning at install and a risk at the next React upgrade.

## Alternatives considered

- **Draw the canvas with React Flow (already here) and `perfect-freehand`.** Much smaller, and the
  stated fallback in the M8 plan if the gate failed. It did not fail, and the brief names Excalidraw.
- **`pnpm patch` instead of a plugin.** The code is one minified line of about 500 KB, so a patch
  file could not be reviewed.
- **Loosening the CSP for the CDN.** Against ADR-0028.
- **tldraw.** Production use needs a licence key, and the brief rules it out.
