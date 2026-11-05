# ADR-0032: Charts and aggregates

- **Status:** Proposed
- **Date:** 2026-11-05
- **Milestone:** M8

## Context

The brief asks for charts and dashboards, and the build plan (§2.7) says numbers are aggregated in SQL
before anything is drawn: a chart over a large vault must not pull its objects across IPC to count them.
The CSP has no `unsafe-eval`, which Vega's expressions normally need; gate 1a showed `vega-interpreter`
draws them without it. A chart also has to be right in both themes and for people who do not see colour
the same way, and has to have a real alternative, which for a picture of numbers is the numbers.

## Decision

- **Vega-Lite, drawn by the interpreter.** It compiles a spec to Vega, parsed with `{ ast: true }` and run
  with `expressionInterpreter`, to SVG. All of it arrives in the Charts view's lazy chunk, so nothing else
  in the app pays for it. ECharts stays out (M8 Decision 3), so Sankey, treemap and sunburst wait.
- **The store counts.** A chart asks `aggregate` for at most 500 buckets: one or two groups, a time bin, a
  count, sum, average, smallest or largest. Its spec holds no `aggregate`, `bin` or `timeUnit`, so Vega draws
  rows and computes nothing. When the store had more, the view says so.
- **Settings are what a chart is.** A saved chart keeps `mark`, `x`, `bin`, `measure`, `of` and `series`,
  and draws from them.
- **The stored spec is written and never read (a change from the plan).** The plan had Vega-Lite as "the
  stored spec". A saved chart does keep one, in `config_json.spec`, naming its rows and holding none of the
  vault's numbers, so what reads a vault from outside can draw it. The app does not draw from it. A spec is a
  program of a kind: it can name a URL for its data, an image or a link, and saved views arrive through the
  files folder and through imports. Drawing only from settings means a chart in a shared vault cannot make a
  request or run anything, whatever is in its file, and leaves no second source of truth to fall out of step.
- **Time is laid out by the app.** The axis is ordinal over labels `chart.ts` writes, with the empty periods
  put in, as zeros where a count or sum has nothing and gaps where an average has nothing to compute from.
  A period's key is the store's, so a day here is the same UTC day as in the Calendar and the Timeline.
- **Form follows what the axis is.** Names rank along the side, largest first, with the value at each bar's
  tip when there is one series and at most 12 bars. What runs in an order, dates and numbers, runs along the
  bottom, and only that offers a line or an area. A sum or count stacks; what cannot be added up sits side by
  side.
- **Colour has rules, and they are checked.** Eight slots, `--dbme-chart-1` to `-8`, the same hues chosen
  separately for each theme and validated on this app's own surfaces, `#ffffff` and `#14171b`: worst adjacent
  CVD ΔE 9.1 light and 8.4 dark, normal-vision 19.6 and 19.3. Three light slots fall below 3:1 against white,
  which the table answers. A series keeps the slot its field gives it, by choice order for a select and yes
  then no for a boolean, so a filter repaints nothing. No value and Other are a neutral, since neither is an
  identity. Past eight series the tail folds into Other where it can be added up, and is left out and said so
  where it cannot; points stop at three, where every pair has to differ and not only neighbours.
- **Marks stay thin.** A bar is at most 24 px with a rounded data end, a line 2 px, a marker 8 px with a ring
  in the surface's colour, an area a 10% wash under its line, and a stack keeps a 2 px gap. The Timeline's
  counts follow the same bar.
- **The alternative is the table of the rows.** "Show as list" is a real table with every row the store
  returned, including what the picture folded into Other or left out. The figure itself is one `role="img"`
  with a sentence saying what it shows and its highest; Vega's own marks are hidden from the accessibility
  tree, since a mark-by-mark reading of an SVG is worse than the table one switch away.

## Consequences

- A chart costs one bounded read however many objects it covers, and scrolling or resizing reads nothing.
- Nothing can hand-edit a spec to reach what the settings do not offer. If that is wanted later, it needs
  its own decision about sanitising what it draws, and this record says why that is not free.
- A text field's series take their slots in name order, as a text field offers no fixed set, so a filter can
  shift them. Select and boolean fields, the usual ones to split by, do not have this.
- Tooltips follow a pointer. Touch and the keyboard have the table instead.

## Alternatives considered

- **Draw from the stored spec.** Simplest to describe, and it would make a chart in someone else's vault a
  way to send requests. The CSP would stop most of it; a chart should not depend on that.
- **Aggregate in Vega.** Fewer store calls, but it pulls the objects across IPC, against build plan §2.7.
- **Canvas rendering.** Quicker for many marks, but a chart here has at most 500 buckets, and SVG stays sharp
  when zoomed and can be inspected and tested.
