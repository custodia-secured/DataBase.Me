# Pages and views

Everything in the vault is a **page**: a title, text made of blocks, and, in Organizer mode, types
with their fields. Pages nest: a page can hold pages, without limit, and the pages list on the left
is that tree. A page is one thing shown many ways; the views are the ways.

## The pages list

The tree on the left (in a drawer behind the **Pages** button on a phone). Choose a page to open it
in the main pane. **New page** makes a page at the top; the **⋯** button above the list holds the
actions for the open page: **New page inside**, **Move up** and **Move down** among its siblings,
**Move to…**, which finds the new parent by title, and **Move to Trash**. Pages can also be
dragged to a new place, and **Today** opens the day's page under Journal.

The breadcrumbs at the top name the open page and the pages above it; each is a link.

## The views

The tabs above the pane switch the view of the open page, and the **More views** menu at the end of
them holds the rest. A view from the menu gets a tab while it is chosen. Each view is live: a change
made in one shows at once in every other, including a second pane in **Split view**, and in the pages
list.

- **Document.** The title, the types and their fields (Organizer mode), and the text: paragraphs,
  headings, bulleted and numbered lists, to-do boxes, quotes, code and dividers, with bold, italic
  and the rest from the toolbar or the usual shortcuts. `[[` followed by a name links to another
  page; the pages that link here are listed under the text as **Linked from**, each with the line
  that mentions this page. Where a page came from, when it was imported, is listed with its files.
- **Outline.** The pages inside this one as an outline: a line each, nested. Enter makes a sibling,
  Tab and Shift+Tab indent and outdent, Alt with the arrows reorders, Ctrl+Enter opens the line's
  page. Writing an outline is making pages.
- **Mind map.** The same tree drawn as a map, this page at the centre, laid out automatically.
  Add, rename, reorder, move under another node, fold and relate (drag between the handles) by
  mouse, keyboard or touch. Far away, the map draws as plain boxes so a large tree stays quick.
- **Table.** (Organizer mode.) The pages of a type as a grid, one row each, one column per field,
  edited in place. **Show** chooses the type, or the pages inside this one. **New row** makes a
  page like the others in the table, **New column** adds a column and **Edit columns** renames or
  deletes them. Thousands of rows scroll smoothly; the grid loads what is on screen.
- **Gallery.** The pages inside this one, or of a type, as cards with a cover picture when the
  page has one.

In **More views** (Organizer mode):

- **Kanban.** The pages inside this one as cards in columns, one column for each choice of a list
  field and one for no value. **Group by** chooses the field when there is more than one. Drag a card
  to move it, or use its **Move to** control. A first press selects a card, and a second opens it. On
  a phone, one column at a time, with the others a swipe away.
- **Calendar.** Month or week, by the event date or any date field. A day shows the first few things
  on it and **+N more** for the rest. Drag an item to another day, or use **Move to**. An event keeps
  its time of day and its length. **New item** adds one, and the list of what has no date holds the
  rest. On a phone, an agenda of the days that have something on them. A day is the same day for
  anyone who opens the vault, so a timed event from a file can land on the next day far from UTC.
- **Timeline.** One lane by a date field or the event date, from days to decades. Close in, it draws
  each thing, an event as long as it runs. Far out, it draws how many fall in each month, year or
  decade. It shows and opens; the Calendar is where a date moves. Down the screen on a phone.
- **Charts.** Bars, a line, an area or points, by any field, with dates grouped by day up to year, as
  a count, sum, average, smallest or largest, split into series if you like. **Save view** keeps a
  chart.
- **Dashboard.** Saved charts and views side by side. **Add tile** puts one here, and each tile's
  menu moves it, widens it or removes it. A tile that fails says so and leaves the others alone. One
  column on a phone.
- **Map.** The pages inside this one that have a location, grouped where they crowd. Choose a group to
  go in, or a place to open, move or remove it. **Place on the map** sets a location by a click on the
  map or by typing it. There is no address search, since that would have to ask someone. The map is
  coastlines that come with the app; a map file of your own can replace them, in
  [Settings](settings.md).
- **Graph.** How what is below this page joins up, by relations and by what is inside what, or the
  whole vault. Up to 200,000 pages. A page starts where its ID puts it, then the layout spreads them,
  and **Pause layout** stops it. Choose a page to see what it joins. A phone starts on the list.
- **Canvas.** This page's own drawing: shapes, lines, text and freehand, and **Add card**, which puts
  a page from inside this one on the canvas as a card that follows its name. A card's corner icon, or
  **Open** when it is chosen, opens its page. If its page goes to the Trash, the card says so. Each
  stroke or move is one undo with the app's Undo, as anywhere else. **Pictures are not kept yet:** one
  dropped on a canvas lasts until you leave the page, and the view says so.

The view chosen for a pane is kept while the app runs. Simple mode offers Document, Outline, Mind
map and Gallery. Everything else is hidden there and comes back with Organizer, with nothing lost.

### Show as list

Every view in **More views** has **Show as list**, in the same place: what the picture shows as a
keyboard grid, with what it encodes written out in columns. It is the same pages, not a summary. The
Map's has each place's coordinates, the Graph's each page's neighbours and what lies within a few
relations of it, and the Canvas's each thing's position, which it can also move. A device that cannot
draw a map or a graph gets the list and is told so. See
[keyboard and accessibility](keyboard-and-accessibility.md).

### Filters and saved views

**Filter** narrows what a view shows, one row per condition: a field, a comparison and a value. Filters
change nothing in the vault, and last while the view is open on this page. **Save view** keeps the
filter, the order and the view's settings under a name. A saved view belongs to its page and its kind of
view, and is listed in the bar above the view. Nothing is saved by looking at something.

### Suggested

**More views** ends with up to three views worth trying on what this page holds, each with its reason:
a calendar if things have dates, a map if they have locations. It reads what is here only when the menu
opens, and never switches a view by itself. The rules are in the app and nothing is sent anywhere.

## Types and fields (Organizer mode)

A **type** is a named set of fields: a Milestone with a status, a due date and an estimate, a
Person with an email. A page may have several types, added from **Add a type** under its title
and removed from the chip. Each field is a button under the types line that edits in place:
select from a list, type a number or a date, tick a box.

The built-in types name the common things (notes, tasks, people, places, events, files, pictures,
web pages, datasets and more), and they start with no fields, so adding one to a page adds no
columns until you give it some. Fields come from two places:

- **New column** in the Table view. A column is a field on a type, and the dialog says which one
  before you add it. With **Show** set to a type, it goes on that type. With **Show** set to the
  pages inside this one, it goes on a type those pages all share; if that is only a built-in type,
  or there is none, they get a new type named after this page. A table with no rows also gets a
  first row for the column to show on.
- **Importing a table**, which makes a type from its columns, with new pages for its rows.

A column shows on every page of its type, in every table. **Edit columns** renames a column or
deletes it; a deleted column's values stay on the pages, and Undo brings it back. A column's kind,
and the choices in a list, cannot be changed after it is made.

Types and fields live in the vault with the pages; in Simple mode they are hidden, not removed.

## Undo

**Ctrl+Z** undoes the last change and **Ctrl+Shift+Z** (or Ctrl+Y) redoes it, from anywhere
outside a text field; the **Undo** and **Redo** buttons in the header do the same. Every change
counts: a typed word, a move, a field, an import (one step per ten thousand pages). A change made
in the files folder by another tool is not a step of this undo stack; it is undone by editing the
file again.

## Search

**Ctrl+K** opens the palette (the **Search** button on a phone). Type to search titles, text and
field values; results show the line the word is in, with the word marked. A line that starts with
`>` lists commands instead: the views, Today, the mode, the theme, Settings, Import. **Show all
results** (or Ctrl+Enter) puts every match on a surface with facets (by type and by field value)
that narrow it, and **Save as view…** there keeps the query in the pages list to run again.
