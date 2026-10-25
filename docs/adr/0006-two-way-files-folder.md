# ADR-0006: Two-way Markdown files folder

- **Status:** Accepted
- **Date:** 2026-10-25
- **Milestone:** M0 (implemented in M5; library choices are M5 ADRs)

## Context

Users must be able to read and edit every object as a plain Markdown file in any tool, including
Obsidian, and see those edits in the app (build plan §8, decision 3). The database stays canonical
(ADR-0002). Editing from two sides invites echo loops, lost edits, duplicates, and mass damage from
cloud-drive glitches or a `git checkout`. This is the riskiest component of the MVP, so its rules
are fixed now and its acceptance tests in M5 are the toughest in the plan.

## Decision

**The files folder is a replica that syncs through the op log**, exactly as a second device would.
Nothing in it bypasses the op log, so history, undo and provenance work for outside edits too.

- **Layout.** `files/<Space>/<Parent>/<Title>.md`; an object's children live in a folder named after
  it. The folder's location is configurable and can be limited to chosen spaces.
- **File shape.** YAML frontmatter holds `id` (the authoritative identity), types, fields, and
  relations as IDs plus wikilinks. The body holds the blocks as Markdown with `^block-id` anchors.
  Wikilinks, `#tags` and properties follow Obsidian's conventions, so the folder opens as an
  Obsidian vault. FORMAT.md specifies the details.
- **Writing out.** Debounced (about 1 s), atomic (write a temporary file, then rename), and the
  content hash is recorded in `mirror_state` so the watcher ignores the app's own writes.
- **Reading in.** A file watcher, debounced; parse; diff against `base_text` (the last synced
  text); apply as ordinary ops with provenance `file:external`.
- **Identity.** Renaming or moving a file renames or reparents its object; moving a folder reparents
  its subtree. A file whose `id` was removed is matched by content hash within the same batch, and
  otherwise becomes a new object.
- **Both sides changed.** Three-way merge against `base_text`: frontmatter field by field, the body
  with a diff3-style merge. On a true conflict the app's version stays, the file's version becomes
  a **conflict-copy object**, and both appear in the Conflicts inbox. No edit is ever silently
  dropped.
- **New and dropped files.** A new `.md` file becomes a new object (type from frontmatter, else
  Note). Any other file goes through the ingestion pipeline, so the folder is also a drop zone.
- **Deletes.** A deleted file sends its object to the Trash, from which it can be restored.
- **Bulk-change guard.** If one batch deletes or rewrites more than 50 files or more than 10% of the
  folder, the engine pauses and asks the user.
- **Bad files.** Unparseable frontmatter is never applied; the file is left alone and flagged in the
  Conflicts inbox. Conflict files from Dropbox, iCloud, OneDrive and Syncthing are recognised and
  routed to the inbox.
- **Database-only data.** Canvas layouts, view configurations, the op log and the AI audit log stay
  in the database and are exported as read-only JSON.
- **Encryption.** On an encrypted vault the files folder is off by default; turning it on warns that
  the files are plain text.
- **Mobile.** File watching sits behind a trait. Platforms without a reliable watcher rescan on
  resume instead.

**Library choices are deferred to M5**, each with its own ADR: the Markdown parser (pulldown-cmark
or comrak), a _maintained_ YAML crate (not `serde_yaml`, which is unmaintained; candidates include
`serde_yaml_ng`, `serde-saphyr` and `saphyr`), the watcher (`notify` with a debouncer), and the
three-way merge (`diffy` or our own diff3 over `similar`).

## Consequences

- M5 must pass: app→file and file→app each within 2 s; a property test of 10,000 random
  interleavings of app and file edits that loses no content; no echo loops; the bulk guard firing on
  50 deletions; rename and move mapping to rename and reparent; the Obsidian fixture opening with
  working links; a vault rebuilt from `files/` alone reproducing every object, field, relation and
  block; and fuzzing of the Markdown and frontmatter parser.
- Because `mirror_state.base_text` stores the last synced text, the database grows by roughly the
  size of the Markdown corpus. That is the price of correct three-way merges.

## Alternatives considered

- **One-way export only.** Safe and simple, but it fails the decision that outside edits flow back.
- **Files as the canonical store.** See ADR-0002.
- **Last writer wins.** Simple, and silently loses edits; it violates "never silently lose data".
