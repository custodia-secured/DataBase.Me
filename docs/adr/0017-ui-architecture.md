# ADR-0017: UI architecture: packages, state, live views and styling under the CSP

- **Status:** Accepted
- **Date:** 2026-10-25
- **Milestone:** M2

## Context

M2 brings the first real views: Document, Outline, Table and Gallery. It also brings the
hierarchy sidebar and a split pane that shows two views side by side. Several constraints shape
the code:

- **Live consistency.** A change made in one view must show at once in every other view, with no
  reload. That includes changes from undo, and later from the files folder (M5) and sync (M13).
- **Large lists.** A table of 100,000 rows must scroll at 60 fps (ADR-0008), so no view can read a
  whole list.
- **The CSP.** The app's Content-Security-Policy (ADR-0010) refuses inline `<style>` elements and
  `style` attributes in markup. Several popular UI libraries rely on one or the other.
- **Reuse.** Plugins (M10) and later milestones will add views, so views must not depend on the
  shell.

## Decision

- **Three packages under the desktop shell:**
  - `@databaseme/client`: the generated bindings (moved from `apps/desktop/src`, ADR-0014),
    `unwrap`, the vault query hooks, the `vault-changed` listener, ULIDs made in the UI, and the
    e2e transport (ADR-0018).
  - `@databaseme/ui`: tokens, shared components and icons. The components are built on Radix
    primitives and the native `<dialog>`, styled by classes in `ui.css`.
  - `@databaseme/views`: the view registry, every view, the sidebar, the block editor
    (ADR-0016) and field editors.

  The shell (`apps/desktop/src`) composes them and owns layout only.

- **UI state lives in Zustand; vault data in TanStack Query.** The shell's store holds which object
  is open, each pane's view, the split, and the phone drawer. It never holds vault data. Every
  read of the vault is a TanStack Query hook whose key starts with `"vault"`.
- **Every change comes back as `vault-changed`.** Views never patch their own state after a
  command. On `vault-changed`, every active vault query is refetched; inactive ones are only
  marked stale. TanStack Query's structural sharing keeps unchanged results identical, so only
  what changed re-renders. The editor also listens directly, to tell its own echo from an outside
  change (ADR-0016).
- **Large lists are read in pages.** The store reads a page as IDs in order first, then a summary
  for each ID, and caps a page at 1,000 rows. The views ask for pages of 200 for the rows on
  screen, so a refresh after a change rereads only those pages. The Table and Gallery are
  virtualised with TanStack Virtual, and the sidebar tree is lazy and virtualised.
- **Keyboard first, ARIA patterns throughout.** The sidebar is an ARIA tree, and the Table and
  Gallery are ARIA grids with one tab stop and `aria-activedescendant`. The view switcher is a
  tab list. The `[[` picker follows the combobox pattern with a listbox, and the formatting bar is
  a toolbar. Every pointer
  action has a keyboard path: "Move to…" stands in for drag-and-drop.
- **Layout follows the window.** From 768 px wide the sidebar sits beside the content; narrower,
  it is a drawer in a modal `<dialog>`. From 1100 px a second pane can show another view of the
  same object.
- **Styling never injects a stylesheet.** All CSS is in bundled files. Styling through the CSSOM
  (`element.style.x = …`, as React's `style` prop and the virtualisers do) is allowed by the CSP
  and is used for positions and sizes only. Some libraries add `<style>` elements or `style`
  attributes, so:
  - TipTap runs with `injectCSS: false`.
  - Radix menus run with `modal={false}`. Modal Radix menus and dialogs lock scrolling by
    injecting styles.
  - Dialogs are the native `<dialog>` with `showModal()`, which gives a focus trap and inert
    background without script.
  - A Radix menu inside a `<dialog>` portals into that dialog. A modal dialog makes everything
    outside it inert, so a menu portalled to `<body>` could not be used.
- **Icons are inline SVG** from `lucide-react`, re-exported through `@databaseme/ui` so the set
  stays small and reviewed.

## Consequences

- Live consistency is one mechanism, not one per view. The e2e tests check it across a split pane:
  a rename in the Table shows at once in the Document's mention, the sidebar and the Gallery, and
  an undo reverts it everywhere.
- A change refetches every query on screen. That is cheap at M2's page sizes. If profiles show
  otherwise, `vault-changed` already carries the changed IDs to narrow it.
- A library that needs injected styles must be configured not to, or not used. The e2e tests fail
  on any CSP violation (ADR-0018), so a new one cannot slip in unnoticed.
- Views depend on `client` and `ui` only, so a plugin view can be built against the same two
  packages.

## Alternatives considered

- **A normalised client-side store kept in sync with the vault.** It would be faster to update,
  but it would duplicate the vault's rules in TypeScript and drift from them.
- **Redux Toolkit or Jotai for UI state.** Either would work. Zustand is smaller and needs no
  provider, and the UI state is small.
- **CSS-in-JS (emotion, styled-components).** These inject `<style>` elements at run time, which
  the CSP refuses. Allowing them would mean relaxing the CSP, which ADR-0010 reserves for an ADR
  of its own.
- **A component library (MUI, Mantine).** These are heavier and inject styles, and they are harder
  to bring in line with WCAG 2.2 target sizes and the CSP than Radix primitives with our own CSS.
- **Reading whole lists and virtualising in memory.** Simple, but 100,000 summaries per refresh
  would break the scroll budget and the memory budget.
