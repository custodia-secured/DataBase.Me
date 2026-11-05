# ADR-0031: The view framework

- **Status:** Proposed
- **Date:** 2026-11-05
- **Milestone:** M8

## Context

M8 takes the app from five views to thirteen, and four of the new ones bring libraries of 0.4 to
1.8 MB. M4 left the filter editor and saved views in the Table and Gallery for this milestone
(Decision 8). The brief asks every visual view for an accessible alternative and a phone layout.
Without a shared shape, each view would answer these separately, and differently.

## Decision

- **Views load when they are opened.** A new view is a `React.lazy` entry in the registry, in a chunk
  of its own, under a boundary (`ViewBoundary`) in the pane. If it cannot load or start, the pane says
  so and the rest of the app keeps working. The five views of 0.1 stay in the first bundle, since
  they are what the app opens on.
- **Five tabs and a menu.** The registry marks the five views of 0.1 as pinned, and they stay tabs. The
  rest are behind **More views** until one is chosen, when it gets a tab of its own. The plan said the
  menu alone on a phone; the five tabs already fit 360 px (the unselected ones show icons only), so
  the phone keeps them and every existing path through the app stays as it was.
- **A listing is a source and conditions.** With no conditions a view reads the source's own pages
  (`objects_page`), as the Table and Gallery always did, so their budgets do not move. With conditions
  it reads the same rows through the DSL. The DSL gains `$parent_id`, so a parent's children can be a
  query, which is what lets a saved view hold any listing.
- **Saved views are explicit.** Settings last as long as the view is open on the object, until the
  person presses **Save view**. A saved view belongs to one object and one renderer: `query_json` holds
  what it lists and its order, and `config_json` the view's own options and whether it shows as a
  list. Saving goes through the op log, so undo works.
- **The filter editor writes conditions.** One row per condition, its comparisons chosen by the field's
  kind, in a dialog, so it works the same by keyboard and by touch. A value that does not fit stops
  the filter and says why. Changing the source clears the filter, since another source has other
  fields.
- **Simple mode keeps none of it.** It has no types or fields to filter by, so the bar stays as it was
  in 0.1.
- **The alternative is a real grid.** `ListAlternative` shows the same objects as the Table's
  virtualised grid does, one tab stop, with what the picture encodes written out in columns.

## Consequences

- A filtered list reads through `LIMIT`/`OFFSET` in the DSL, so a deep page of a large filtered list
  is slower than the same page unfiltered. Nothing here measures that yet; the budgets cover the
  unfiltered path.
- A condition on a field that a later change removes stays in a saved view and matches nothing. The
  editor still shows it, by its key, so it can be removed.
- `listingFromQuery` reads back only the shape `listingQuery` writes. A saved view written by something
  else (a plugin, later) with several types or `under` loses that part when opened in these views.

## Alternatives considered

- **Saving settings automatically, per object.** Simpler to use, but looking at something would write
  to the vault and the op log (M8 Decision 8).
- **Moving the Table and Gallery wholly onto the DSL.** One path instead of two, but the 100k-row budget
  rests on the paged read, and nothing has shown the DSL holds it at high offsets.
- **A second column in the query for the source.** The DSL would need a concept only views use, where a
  condition on `$parent_id` is one more column.
