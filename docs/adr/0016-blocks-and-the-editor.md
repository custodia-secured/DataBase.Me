# ADR-0016: Blocks and the editor

- **Status:** Accepted
- **Date:** 2026-10-25
- **Milestone:** M2

## Context

An object's text is a tree of blocks in the vault (FORMAT.md §3.1), and M2 adds the editor that
writes it. Four things pull against each other here:

- The vault must stay the only source of truth, with one history and one undo, while the editor
  needs instant local edits.
- Other views can change the same blocks while the user is typing, and so can undo.
- Mentions must be queryable as backlinks.
- An older app must not damage blocks a newer app or a plugin wrote.

The handoff brief names TipTap/ProseMirror for the editor.

## Decision

- **Blocks are stored as kinds with a JSON content.** M2 knows nine kinds: `paragraph`, `heading`,
  `bullet`, `numbered`, `todo`, `quote`, `code`, `divider` and `image`. FORMAT.md §3.5 fixes the
  shape of each kind's `content_json`. Text kinds hold `inline`, an array of runs. A run is
  `{"text", marks?, "link"?}` or `{"mention": "<object ID>"}`. The marks are `bold`, `italic`,
  `underline`, `strike` and `code`, each stored as `true` or left out. A line break is a `\n`
  inside a text run.
- **The core checks content.** `dbme_core::block::validate_block` checks it on every write.
- **Unknown kinds and keys are kept.** A kind this version does not know only needs a lower-case
  identifier for its name and a JSON object for its content. Keys a known kind does not use are
  also kept. The editor shows an unknown block as an atom that carries the block and its children
  untouched.
- **Only list items nest.** As in Markdown, `bullet`, `numbered` and `todo` (and unknown kinds) may
  have child blocks; the other kinds may not. An item's children are the blocks indented under it.
- **The editor is TipTap, core only.** It uses `@tiptap/core`, `@tiptap/pm`, `@tiptap/react` and
  the MIT StarterKit and list extensions. No Pro or cloud extension is used. `injectCSS` is off, so
  TipTap injects no `<style>`; the editor's styles live in `views.css`.
- **The mapping is pure.** `packages/views/src/editor/mapping.ts` turns a block tree into a
  ProseMirror document and back as functions over JSON. Property tests (fast-check) check that
  every valid tree round-trips unchanged.
- **The client makes block IDs.** The UI makes each new block's ULID the moment the block exists
  (`packages/client/src/ulid.ts`), before any round trip. The store accepts a client ID and
  refuses one that another block already has. After every change, an editor plugin gives a fresh
  ID to any block node that has none, or that repeats an earlier node's ID (a split, a paste, a
  duplicate). A list item's first paragraph mirrors the item's ID. Wrapping a paragraph in a list,
  or lifting it back out, therefore keeps its block and changes only its kind.
- **One flush is one undo step.** The editor keeps a snapshot of the blocks it last loaded or
  saved. Each flush sends the diff from that snapshot to the document as one `blocks_apply` call,
  which is one transaction. The diff puts creates, moves and updates in document order, then deletes
  only the top-most of the removed blocks, then changes the kinds of blocks that lose the right to
  have children. A flush happens 400 ms after the last change, at most 2 s after the first, on blur,
  and before undo or redo.
- **There is no editor-local history.** StarterKit's `undoRedo` is off. Ctrl+Z, Ctrl+Shift+Z and
  Ctrl+Y flush, then call the vault's undo and redo.
- **Outside changes are reconciled on read.** On `vault-changed` the editor reads its blocks again.
  If they match the snapshot, they are its own echo and are ignored. Otherwise it flushes unsaved
  typing and then shows what the vault holds. If a flush is refused because the vault changed
  underneath, it is diffed again against the vault and sent once more. The editor's document wins,
  and what it overwrote stays in the vault's history.
- **Mentions are relations.** The vault adds or edits a block and keeps that block's `mentions`
  relations matching the mentions in its content, in the same transaction and so the same undo
  step. Backlinks are these relations read in reverse, with their source titles and a snippet.
- **Sort keys stay short.** A key longer than 16 characters makes its siblings rebalance in the
  same step (ADR-0013). That takes about 70 inserts at one spot.

## Consequences

- Undo works the same everywhere: one key undoes the last change, whichever view made it.
- A pause in typing costs at most one IPC round trip per flush. The editor never waits on the
  vault to show a keystroke. The typing budget (ADR-0018) measures this.
- Blocks from a newer app survive an older one, so long as the older one does not delete them.
- The diff and the mapping are the riskiest code in M2. Both are pure and property-tested against
  random block trees, including any tree turned into any other.
- A paste or split that duplicates an ID is fixed in the editor before it reaches the vault. The
  vault refuses a duplicate anyway.

## Alternatives considered

- **Lexical, Slate or BlockNote.** Lexical and Slate would need the block model built from scratch;
  BlockNote builds on TipTap and adds its own block schema, which would sit between ours and
  ProseMirror. TipTap's core gives ProseMirror's model with less code to own.
- **Saving every keystroke.** That would put the vault in the typing path and give one undo step
  per character.
- **ProseMirror's own history, synced to the vault.** That would make two histories that disagree
  as soon as another view changes the object.
- **Storing the whole document as one JSON value.** Simpler to save, but it would lose per-block
  IDs, and with them block links, partial sync (M13) and the files folder's mapping (M5).
- **IDs made by the vault.** A new block would have no ID until its first save, so a split
  followed at once by a move could not be expressed.
