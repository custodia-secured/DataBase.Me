# The files folder and Obsidian

The files folder is a second copy of the vault as plain Markdown: one file per page, in folders
that follow the pages tree, with the page's fields in a frontmatter block at the top. It is
two-way. A change in the app reaches its file within two seconds; a change to a file, made in any
editor, reaches the app as fast and shows in every view. The folder is designed to open as an
Obsidian vault, and the app does not need to be running for the files to be read.

## Turning it on

**Files folder** in the header opens the dialog. **Turn on** starts it; leave the folder field
empty for `files/` inside the vault, or name another folder (an existing Obsidian vault, a folder a
cloud drive syncs). The first write puts every page into its file, with a progress line while it
runs; the status line then reads "On: _n_ synced." **Sync now** runs both directions at once, for
when you want to see a change land rather than wait for it; **Turn off** stops the folder and
leaves the files where they are.

The folder's location is remembered per vault on this machine, not inside the vault, so a vault
copied to another computer asks again rather than pointing at a folder that is not there.

## What a file looks like

```markdown
---
id: 01JB3R7Q8Y5K3M2N4P6Q8R0S2T
types: [Task]
tags: [travel]
created: 2026-10-25T08:21:11.000Z
updated: 2026-10-25T09:02:40.000Z
due: 2026-11-30
status: Waiting
---

Book the ferry before the price goes up. ^01JB3RB6QH1WJ2XK3YL4ZM5NA6

- Compare the two crossings ^01JB3RB6QH1WJ2XK3YL4ZM5NA7
```

- The **frontmatter** holds the page's identity (`id`), its types and tags, when it was made and
  changed, and each field as an Obsidian property. The `id` is what ties the file to the page;
  the file's name and place follow the title and the tree, and a rename or a move of the file is
  a rename or a move of the page.
- Each **block** ends with an Obsidian block anchor (`^` and the block's id), so that an edit to a
  paragraph is an edit to that paragraph and not a rewrite of the page. New lines without an
  anchor are new blocks; the app adds the anchor when it writes the file back.
- **Links** are wikilinks naming the path, `[[Projects/Alpha]]`, as Obsidian writes them.
  Attached pictures are `![[attachments/<hash>.jpg]]`.
- A page's children are in a folder with the page's name, next to its file. A trashed page's file
  moves to `.dbme/trash/`. `.dbme/schema.json` describes every type and field, so the folder can
  be read, and the vault rebuilt, without the database.

Every rule is in [FORMAT.md §6](../FORMAT.md).

## Editing outside the app

Open the folder in Obsidian (**Open folder as vault**), in VS Code, or in any editor. Save a file
and the page changes; make a new file and a page appears; delete a file and the page goes to the
Trash; move a file and the page moves. A file without a frontmatter `id` becomes a new page; the
app never trashes anything on a guess.

An edit is merged on the page's structure, not its bytes: field by field, block by block, against
the text the app last wrote. You and the app can change different blocks of one page at the same
time and both changes stay. A change to the same block by both sides is merged line by line, and
only when that fails is it a **conflict**.

## The Conflicts inbox

The dialog's lower half. An entry appears when:

- the same block changed in the app and in the file at once, and the lines could not be merged:
  the app's version stays on the page and the file's becomes a conflict copy, kept here until you
  **Dismiss** it or open the page and take what you want from it;
- a file could not be read (a broken frontmatter, a value a field rejects, a file too large): the
  file is left untouched and the page is not written over it until the file is fixed or the entry
  dismissed;
- two files carry the same `id`, or a cloud drive left a "conflicted copy" beside a file.

Nothing is lost on a conflict: both texts exist until you decide.

## The hold

When many files change or disappear at once (fifty or more, as when a folder is moved or a sync
client empties it), the app applies nothing and shows a banner under the header: "Many files in
the files folder changed at once." **Apply the changes** takes them; **Restore the folder** writes
the pages back from the vault; **Details** opens the dialog. Until you choose, the vault is as it
was.

## Obsidian

The folder opens in Obsidian as a vault with no plugin. Properties show in Obsidian's editor;
block anchors and wikilinks resolve; tags are tags. Obsidian's own folders (`.obsidian/`) and any
dot-prefixed file are left alone by the app. The app's frontmatter keys (`id`, `types`, `tags`,
`created`, `updated`, `relations`, `links`) are reserved; a field of yours with one of those names
is written under `fields:` and read back from there.

Importing an existing Obsidian vault is a different act from pointing the files folder at it:
[importing](importing.md) copies the notes in as pages, with links, frontmatter, tags and pictures
kept, and leaves the original alone; the files folder makes a folder _be_ the vault's second copy.
