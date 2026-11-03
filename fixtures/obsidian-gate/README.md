# Gate 1h: open this folder in Obsidian

This folder is a small Obsidian vault. It exists so a person can look at the file shape M5 proposes
(docs/milestones/M5.md, Design "Frontmatter" and "Body") in real Obsidian before any format code
is written. Nothing here is real data; every name, ID and date is made up.

How to run it: in Obsidian choose "Open folder as vault" and pick `fixtures/obsidian-gate/`. The
`.obsidian/app.json` is an empty object, so Obsidian starts with its defaults. Do not commit any
file Obsidian adds under `.obsidian/` or any note it rewrites; `git status` shows them. Work through
the checklist in order and write the result after each item.

## Record

- Obsidian version (Settings → General → "Current version"):
- Operating system:
- Date:
- Checked by:

## Checklist

Write "pass", "fail" or what you saw after each item. "Pass" is what the plan assumes; a "fail" is
a finding, not a mistake.

1. `Blocks/Paragraph.md`: open in reading view. Pass: two paragraphs, with bold, italic, strike
   and code shown; no ` ^01hx3e01` text is visible at the end of the lines (Obsidian hides block
   identifiers in reading view).
2. `Blocks/Headings.md`: reading view. Pass: six heading levels; the lone `^01hx3h0n` lines are not
   visible, or are visible only as faint identifiers. Then, in `Links.md`, type
   `[[Blocks/Headings#^` and record which identifiers the suggestions offer. Pass for Decision 8:
   `01hx3h02` is offered and `![[Blocks/Headings#^01hx3h02]]` embeds "Heading two". Also record
   what the last heading, which carries ` ^01hx3h08` on the heading line itself, shows.
3. `Blocks/Bullet list.md`: reading view and live preview. Pass: three nested levels (Decision
   11b); the outline pane (Outline core plugin) is not needed.
4. `Blocks/Numbered list.md`: reading view. Record for the first list (two-space nesting) whether
   the items are nested or shown as one flat list or as continuation text. The second list
   (three-space nesting) is the control and should nest. Pass for Decision 11b: both nest the same
   way.
5. `Blocks/Todo list.md`: reading view. Pass: checkboxes render, nested two deep. Click the first
   checkbox, then open source mode and copy the first line here verbatim. Pass: the line is
   `- [x] An open task ^01hx3t01` with the anchor intact.
6. `Blocks/Quote.md`: reading view. Pass: both quotes render with two paragraphs and no visible
   anchor text. Then type `[[Blocks/Quote#^` in `Links.md` and record which of `01hx3r02` (on the
   last quote line, the plan's shape) and `01hx3r04` (own line, the documented shape) is offered,
   and what each embeds. Pass for Decision 8: `01hx3r02` embeds the whole quote.
7. `Blocks/Code rust.md` and `Blocks/Code dbme.md`: reading view. Pass: the fences render as code,
   `rust` highlighted, `dbme` plain, the JSON intact; the `^01hx3c02` / `^01hx3j02` lines are not
   visible as text. Record whether `[[Blocks/Code rust#^01hx3c02]]` is offered and embeds the
   fence.
8. `Blocks/Divider.md`: reading view. Pass: a horizontal rule; `^01hx3v02` not visible as text.
   Record whether `[[Blocks/Divider#^01hx3v02]]` is offered.
9. `Blocks/Table.md`: reading view. Pass: a three-column table with a working `[[Deep]]` link in a
   cell; `^01hx3w02` not visible. Record whether `![[Blocks/Table#^01hx3w02]]` embeds the table.
10. `Blocks/Image.md`: reading view. Pass: a tiny white square (one pixel; it may be hard to see on
    a white theme, so switch to a dark theme or zoom) and a 64-pixel one below it (Decision 9); no
    anchor text visible.
11. `Blocks/Empty paragraph.md`: reading view and source mode. Record whether `^01hx3z02` is shown
    as text, as nothing, or whether a hover over `[[Blocks/Empty paragraph#^01hx3z02]]` shows the
    paragraph above it. Pass for Decision 11: it is not attached to the paragraph above.
12. `Shape A fields.md`: open the Properties view (reading or live preview shows the property
    panel; the "Properties view" core plugin adds a sidebar). Record for `fields`, `relations` and
    `links` whether the panel shows them editable, as text, or with an "unsupported" marker; and
    for `types`, `tags`, `created`, `updated` which type icon each gets. Pass for Decision 4 as
    written: `fields` is editable field by field. The documentation says nested properties are not
    supported, so expect a fail and record the exact wording.
13. `Shape B top-level.md`: same panel. Record which of `due`, `status`, `estimate`, `done`,
    `answer`, `website` get date, text, number, checkbox icons and which are editable. Pass: all six
    editable; `due` is a date, `estimate` a number, `done` a checkbox, `answer` text with the value
    `no` intact. Then change `status` to `Done` in the panel, open source mode and record what
    Obsidian rewrote (did `types: [Task]` become a block list, did `created` keep its `.000Z`
    suffix, were quotes added anywhere). Revert the file with `git checkout -- "Shape B top-level.md"`.
14. `Links.md`, hover or ctrl-click each link: record which file `[[Projects/Alpha]]`,
    `[[Archive/Alpha]]` and the bare `[[Alpha]]` open. Pass: the two path-form links open their own
    file; for the bare link write down which `Alpha` it chose. Then rename `Projects/Alpha.md` to
    `Projects/Alpha2.md` in Obsidian, look at `Links.md` in source mode, record how Obsidian
    rewrote the links (this is the "Automatically update internal links" default), and restore
    with `git checkout -- .`.
15. `Links.md`: `[[Projects/Alpha|the project]]` shows "the project" and opens Projects/Alpha;
    `[[Projects/Alpha#^01hx3k7p]]` hovers the first paragraph of Projects/Alpha;
    `[[Projects/Alpha#Goals]]` hovers the "Goals" section; `[[Projects/Alpha#^01hx3p01]]` (the
    heading's own-line anchor): record what it shows. Pass for Decision 8: the "Goals" heading.
16. `Links.md`: `#gate` and `#gate/sub` are clickable tags; the Tags view (command "Tags: Show
    tags") lists `gate` with `sub` nested under it and counts the frontmatter `tags:` of the other
    notes too. Pass: both present.
17. `Links.md`: `[[Nowhere]]` renders as an unresolved link (dimmer colour) and does not create a
    file until clicked. Do not click it, or delete `Nowhere.md` afterwards.
18. `Links.md`: `[[Ünïcödé — dash]]` opens the note of that name; the file explorer shows the name
    with umlauts and em dash unchanged. `[[Deep/Deeper/Deepest/Leaf]]` opens the leaf; the
    explorer shows `Deep.md` next to the `Deep/` folder, and so on three levels down.
19. `Links.md`: `[[projects/alpha]]` (lower case). Record whether it resolves to `Projects/Alpha`
    or renders as unresolved. This settles whether link resolution is case-insensitive, which the
    documentation does not state.
20. `Links.md`: `![[Projects/Alpha#^01hx3k7p]]` embeds the first paragraph of Projects/Alpha;
    `![[attachments/pixel.png|32]]` shows a 32-pixel square. Pass: both.
21. Open `Links.md` in source mode and record whether any of the `^01hx3l..` anchors is visible as
    text in reading view (none should be).
22. Last: run `git status` in the repository. List every file Obsidian changed or added, then
    `git checkout -- fixtures/obsidian-gate` and delete the untracked files, so the fixture stays as
    committed.

## Rules fetched from Obsidian's documentation

Fetched on 2026-11-02 with WebFetch. `help.obsidian.md/<page>` now redirects (301) to
`obsidian.md/help/<page>`; the URLs below are the ones that answered. Each rule is paraphrased in
one line; "fetched" means the page said it, "recalled" means the page did not and the line is from
memory, to be confirmed by the person.

### Internal links: https://obsidian.md/help/links

- Path-form links start at the vault root and use forward slashes even on Windows, as in
  `[[Projects/Three laws of motion]]`. Fetched.
- A bare `[[Name]]` is written when it is unique. Settings → Files and links → "New link format",
  "Shortest path when possible", means the shortest path that is unique in the vault;
  https://obsidian.md/help/settings (the Files and links section; the page
  `obsidian.md/help/settings/files-and-links` does not exist). Fetched.
- How a bare `[[Name]]` resolves when two files share the basename is not stated on either page.
  Recalled: Obsidian resolves it to one of them (the choice is not documented) and writes the path
  form itself when it creates such a link. Item 14 records what it does.
- Case sensitivity of link resolution is not stated on the page. Recalled: resolution is
  case-insensitive. Item 19 records it.
- Heading links are `[[Note#Heading]]`; a nested heading adds more `#` segments. Fetched.
- Display text is `[[Note|shown]]`; the pipe changes one link's appearance only. Fetched.
- Block identifiers, section "Link to a block in a note": an identifier may consist only of Latin
  letters, numbers and dashes. Fetched.
- For a simple paragraph the identifier sits at the end of the line after a space. Fetched.
- For lists, quotations, callouts and tables the identifier goes on a separate line with a blank
  line before and after; for a single list item it may sit on the item's line. Fetched.
- Headings and fenced code are not mentioned in the block-identifier section; headings are linked
  with `#Heading`. Whether a lone `^id` line after a heading or fence attaches to it is not
  documented. Items 2, 7 and 8 record it.
- A warning on the page says links to parts of quotations, callouts and tables are not supported;
  a whole-table identifier is. Fetched.

### Properties: https://obsidian.md/help/properties

- Property types: Text, List, Number, Checkbox, Date, Date & time, Tags; the examples are
  `date: 2020-08-21` and `time: 2020-08-21T10:30:00`, with no time zone suffix. Fetched. Whether a
  `.000Z` suffix still reads as Date & time is item 12.
- Section "Not supported": nested properties are listed as not supported, and the page recommends
  source mode to view them. What the panel shows for them is not described. Fetched.
- Lists are YAML lists; a value must be a literal (a number may not be an expression). Fetched.
- Internal links as values must be quoted, as in `link: "[[Episode IV]]"`. Fetched.
- `tags`, `aliases` and `cssclasses` are the default properties, each of list type; the Tags type
  belongs to `tags` alone; one `tags` property per note. `tag`, `alias`, `cssclass` are deprecated
  since 1.4 and dropped as defaults in 1.9. Fetched.
- A property name must be unique within a note; order does not matter; the name is followed by a
  colon and a space. Fetched.
- Once a name has a type, every property of that name in the vault has that type. Fetched.
- JSON frontmatter is read and saved back as YAML. Fetched.

### Embeds: https://obsidian.md/help/embeds

- An embed is an internal link with `!` in front: `![[note]]`, `![[note#^id]]`,
  `![[note#Heading]]`. Fetched.
- Images: `![[image.png]]`, `![[image.png|100]]` for a width, `![[image.png|100x145]]` for both.
  Fetched.
- Lists embed by a block identifier placed on them. Fetched.

### Tags: https://obsidian.md/help/tags

- `#tag` in the body or a `tags:` list in the frontmatter. Fetched.
- Allowed characters: letters, numbers, underscore, hyphen, and `/` for nesting; at least one
  non-numeric character; no spaces. Fetched.
- Tags are case-insensitive; the Tags view shows the first casing seen. Fetched.

### Accepted file formats: https://obsidian.md/help/file-formats

- `.png` is among the accepted image formats, so `attachments/pixel.png` can be embedded. Fetched.
- The page does not list characters forbidden in file names. FORMAT.md §6.1's list stays
  recalled.

### Aliases: https://obsidian.md/help/aliases

- `aliases:` is a YAML list; Obsidian writes alias links in the `[[Name|alias]]` form so they
  stay interoperable. Fetched.
