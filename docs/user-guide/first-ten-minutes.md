# The first ten minutes

Five things, in the order most people meet them: write something down, give it a little
structure, find it again, change it from outside the app, and take a copy of everything. The same
five steps are the app's acceptance test (`apps/desktop/e2e/scenario.spec.ts` runs them on every
change, on a desktop window and a phone-sized one), so what this page describes is what the app
is held to.

If you have not installed the app yet, [getting started](getting-started.md) comes first. The
first launch asks where the vault goes, whether you want the files folder, Simple or Organizer
mode, the theme and whether to check for updates. The defaults are fine; everything here can be
changed later in Settings.

## 1. Capture

Press **Ctrl+K** (⌘K on a Mac) and type `> Today`, or press **Today** at the top of the pages
list. That opens today's page, dated, under a page called **Journal**, making both on the first
day. Type a line:

> Call the plumber about the kitchen tap

There is no save. Each change is written to the vault as you make it and can be undone with
**Ctrl+Z** from anywhere outside a text field.

In Simple mode that is all a page is: a title and text, with headings, lists, to-do boxes, quotes
and links to other pages (`[[` and the page's name).

## 2. Organize

Open the palette and run `> Mode: Organizer`. The page now shows a **types** line under its title.
Choose a type from **Add a type**; a type brings its fields with it, and each field is a button
under the types line that edits in place. Pick a value.

To put the page somewhere, it needs somewhere to go: press **New page** above the pages list and
name it **Inbox**. Open today's page again, press the **⋯** button above the list (the actions for
the open page) and choose **Move to…**. Type `Inbox` and choose it. The breadcrumbs at the top
change at once.

Switching back to Simple hides the types and fields again. It changes what is shown, never what is
stored: a vault made in Organizer opens the same in Simple.

## 3. Find

Open the palette from any page and type a word from the text, `plumber`. The result shows the
page's name and the line the word is in, with the word marked. Choose it to open the page. The
palette also finds pages by title, and field values when you are in Organizer mode; a line that
starts with `>` runs a command instead.

## 4. Edit a file outside the app

Press **Files folder** in the header and turn the folder on. The app writes every page as a
Markdown file, with its fields in a frontmatter block at the top: one folder per page that has
pages inside it, so `Inbox/2026-11-03.md` is the page you moved. The folder is inside the vault
unless you chose another place during onboarding.

Open that file in any editor, add a line at the end, and save. The page in the app shows the new
line within two seconds, and a change made in the app reaches the file as fast. The dialog has a
**Sync now** button for when you want to see it happen, and an inbox for the rare case where the
file and the app changed the same block at the same time.

The folder is the vault's second copy, in a form every other tool can read. Obsidian can open it
as a vault; the [files folder](files-folder.md) page says how.

## 5. Export

Open **Settings** (the header button, or `> Settings` in the palette). Under **Vault**, type or
pick a new or empty folder next to **Export this vault to a folder** and press **Export**. The app
writes the vault's types as `schema/types.json`, every saved view under `views/` and one CSV per
type under `data/`, which any spreadsheet opens. With **Full export** checked, as it is by
default, it adds the change log (`ops.jsonl`), the vault's facts (`vault_meta.json`) and every
attached file under `blobs/`; from those, the command-line tool rebuilds the vault to the byte:

```sh
dbme import <the export folder> <a new vault folder>
```

The status line under the button says what was written. The format of every file is documented
in [FORMAT.md](../FORMAT.md), which the app also writes into each vault it makes, so a vault
explains itself to whoever finds it.

## Where to go next

- [Pages and views](pages-and-views.md): the Outline, Table, Gallery and Mind map views of the
  same pages.
- [Importing](importing.md): a Markdown folder, CSV, spreadsheets, PDFs and photos.
- [Settings, modes and themes](settings.md).
- [Keyboard and accessibility](keyboard-and-accessibility.md): every action from the keyboard,
  and with a screen reader.
- [Privacy](privacy.md): what the app promises about the network, and how to check it yourself.
