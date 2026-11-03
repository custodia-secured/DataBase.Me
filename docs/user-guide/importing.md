# Importing

**Import** in the header, `> Import files` in the palette, or files dropped on the window, opens
the Import dialog. It brings files into the vault as pages and leaves the originals where they
are.

## What comes in

| Source                                  | What it becomes                                                                                                                                                                                                                               |
| --------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| A folder of Markdown, an Obsidian vault | One page per note, in the folder's tree; frontmatter fields, tags, wikilinks between the notes and embedded pictures kept. The notes are read by the same code as the files folder, so what Obsidian wrote is what the app reads.             |
| CSV, Excel (`.xlsx`)                    | One page per row, under a Dataset page that holds the file; a type made from the columns, with each field's kind inferred from the values (number, date, yes/no, text) and a value that does not fit kept as text. A sheet is a dataset each. |
| PDF                                     | A page with the document's title, author and page count, and the text as a heading and paragraphs per page.                                                                                                                                   |
| Pictures (JPEG, PNG, GIF, WebP)         | A page with the picture attached and a thumbnail; the date, place and camera from the picture's EXIF when it carries them.                                                                                                                    |
| Anything else                           | A page with the file attached.                                                                                                                                                                                                                |

## Preview first

Type the files or folders, one per line, or drop them on the window. **Preview** reads them and
writes nothing: it lists each source and what it would become, how many pages in all, which files
are already in the vault (found by their content, so a file imported twice is skipped, or linked
to the page that has it), which could not be read and why, and any warnings (a spreadsheet's
formula cells, for one, are imported as their values, never run). Changing the paths or the
destination drops the preview; **Preview** again.

The pages land under the open page when you tick **Put them under**, or under a new page named
"Import" otherwise.

## Import, and undo

**Import _n_ objects** writes them. The receipt says what was written and offers **Open the
receipt**, a page that lists the sources and what came of each, and **Undo the import**, which
removes the import whole: one undo step per ten thousand pages, so a large import is a few steps, not one per
page.

## Where a page came from

Every imported page remembers its origin: the Document view lists it under the text ("Imported
from _file_", with the date) beside the page's files. The record travels with the page: it
exports, mirrors to the files folder and is undone with the page.

## Hostile files

Files from outside are treated as untrusted. A malformed PDF, a picture that claims an absurd
size, a spreadsheet with formulas or a sheet named like a path: each is contained, reported in the
preview, and never runs anything. An archive (zip, tar) is not opened at all; it is attached as a
file like any other. The parsers are fuzzed nightly in the project's CI.

## From the command line

The `dbme` tool imports without the app: `dbme import <source>… <vault>`, with `--dry-run` for the
preview and `--into <page id>` to put the pages under an existing page. With a vault folder that
does not exist yet, it imports an export folder, which rebuilds a vault from a full export
([the vault](the-vault.md)).
