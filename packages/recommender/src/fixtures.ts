// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * The labelled sets the recommender is measured against (M8, Task 9; Decision 7).
 *
 * THE LABELS ARE THE AUTHOR'S. The brief asks for agreement with labelled fixtures and the plan for an
 * expert-labelled set. Nobody independent labelled these: the owner chose, on 2026-11-05, that the author
 * writes them, from published guidance and before any rule existed. The commit history shows that order.
 * A figure against them says the rules do what their author meant, and is no evidence that an independent
 * expert would agree.
 *
 * Where a label comes from, by `basis`:
 * - `cleveland-mcgill`: quantities read best as position on a common scale, so numbers want a chart
 *   (Cleveland & McGill 1984).
 * - `few-table`: a table is for looking up exact values across many attributes; a graph is for the shape
 *   of them (Few, Show Me the Numbers, 2004).
 * - `voyager`: a data type and its cardinality limit which encodings are effective at all, and a nominal
 *   field of few values groups well (Wongsuphasawat et al., Voyager, 2016).
 * - `draco`: a hard constraint rules a design out, and soft ones rank what remains (Moritz et al., Draco,
 *   2019).
 * - `kanban`: columns are for a small set of states work moves through (Anderson, Kanban, 2010).
 * - `time`: a day grid is for what falls on days people plan around, and a scaled line for durations and
 *   long spans, where the distance means something (Aigner et al., Visualization of Time-Oriented Data,
 *   2011).
 * - `geo`: what is about places belongs where the places are, if most of it has one.
 * - `network`: links between many items are best seen as a node-link diagram when the links are what
 *   matters (Munzner, Visualization Analysis and Design, 2014).
 * - `tree`: nesting wants indentation for reading through many items, and a radial map for a few ideas
 *   (Munzner 2014).
 * - `images`: pictures are recognised by looking at them, so what is mostly images wants a grid of them.
 * - `plain`: a page with little under it is a page, and a free surface is for someone arranging by hand.
 */
import type { FieldShape, Goal, Shape, ViewName } from "./shape";

export interface Fixture {
  name: string;
  shape: Shape;
  goal?: Goal;
  expected: ViewName;
  basis: string;
}

const base: Shape = {
  objects: 0,
  withEvent: 0,
  eventRangeDays: 0,
  withSpan: 0,
  withGeo: 0,
  withChildren: 0,
  withRelations: 0,
  fields: [],
};

export const shape = (change: Partial<Shape>): Shape => ({ ...base, ...change });

export const select = (distinct: number, filled = 1): FieldShape => ({
  kind: "select",
  filled,
  distinct,
});
export const date = (rangeDays: number, filled = 1, distinct = 50): FieldShape => ({
  kind: "date",
  filled,
  distinct,
  rangeDays,
});
export const number = (filled = 1, distinct = 40): FieldShape => ({
  kind: "number",
  filled,
  distinct,
});
export const text = (filled = 1, distinct = 40): FieldShape => ({ kind: "text", filled, distinct });
export const file = (filled = 1): FieldShape => ({ kind: "file", filled, distinct: 40 });
export const url = (filled = 1): FieldShape => ({ kind: "url", filled, distinct: 40 });
export const contact = (filled = 1): FieldShape => ({ kind: "contact", filled, distinct: 40 });

/** What the rules were written against. */
export const FIXTURES: readonly Fixture[] = [
  // Work moving through a few states.
  {
    name: "A task list with a status",
    shape: shape({ objects: 40, fields: [select(4), date(120, 0.5), text()] }),
    expected: "kanban",
    basis: "kanban",
  },
  {
    name: "A bug tracker",
    shape: shape({ objects: 200, fields: [select(5), select(3), number(0.2), text()] }),
    expected: "kanban",
    basis: "kanban",
  },
  {
    name: "A hiring pipeline",
    shape: shape({ objects: 25, fields: [select(6), text()] }),
    expected: "kanban",
    basis: "kanban",
  },
  {
    name: "A reading list by whether it is read",
    shape: shape({ objects: 60, fields: [select(3), number(0.4), text()] }),
    expected: "kanban",
    basis: "kanban",
  },
  {
    name: "Someone organizing things that have a state and a date",
    shape: shape({ objects: 30, fields: [select(4), date(90)] }),
    goal: "organize",
    expected: "kanban",
    basis: "kanban",
  },

  // What falls on days.
  {
    name: "Appointments over a year",
    shape: shape({ objects: 120, withEvent: 1, eventRangeDays: 300 }),
    expected: "calendar",
    basis: "time",
  },
  {
    name: "A content schedule, to plan",
    shape: shape({ objects: 50, fields: [date(90), select(3)] }),
    goal: "schedule",
    expected: "calendar",
    basis: "time",
  },
  {
    name: "Meeting notes by their date",
    shape: shape({ objects: 80, fields: [date(200), text()] }),
    expected: "calendar",
    basis: "time",
  },
  {
    name: "Birthdays",
    shape: shape({ objects: 30, fields: [date(360)] }),
    expected: "calendar",
    basis: "time",
  },
  {
    name: "A few deadlines",
    shape: shape({ objects: 15, fields: [date(60), text()] }),
    expected: "calendar",
    basis: "time",
  },

  // What spans a long time, or has a length of its own.
  {
    name: "Events across two centuries",
    shape: shape({ objects: 300, fields: [date(73_000), text()] }),
    expected: "timeline",
    basis: "time",
  },
  {
    name: "A family history",
    shape: shape({ objects: 80, withEvent: 1, eventRangeDays: 36_500 }),
    expected: "timeline",
    basis: "time",
  },
  {
    name: "Project phases, each with an end",
    shape: shape({ objects: 12, withEvent: 1, eventRangeDays: 500, withSpan: 1 }),
    expected: "timeline",
    basis: "time",
  },
  {
    name: "A career, job by job",
    shape: shape({
      objects: 10,
      withEvent: 1,
      eventRangeDays: 7000,
      withSpan: 0.9,
      fields: [text()],
    }),
    expected: "timeline",
    basis: "time",
  },

  // Quantities.
  {
    name: "Expenses by month and category",
    shape: shape({ objects: 500, fields: [number(), date(365), select(8)] }),
    expected: "charts",
    basis: "cleveland-mcgill",
  },
  {
    name: "Sales by region",
    shape: shape({ objects: 1000, fields: [number(), select(6)] }),
    expected: "charts",
    basis: "cleveland-mcgill",
  },
  {
    name: "Survey scores by group",
    shape: shape({ objects: 300, fields: [number(1, 5), select(4)] }),
    expected: "charts",
    basis: "cleveland-mcgill",
  },
  {
    name: "Workouts over time",
    shape: shape({ objects: 150, fields: [number(), number(), date(200)] }),
    expected: "charts",
    basis: "cleveland-mcgill",
  },
  {
    name: "Sensor readings",
    shape: shape({ objects: 5000, fields: [number(), date(30, 1, 720)] }),
    expected: "charts",
    basis: "cleveland-mcgill",
  },
  {
    name: "Someone comparing a few figures",
    shape: shape({ objects: 40, fields: [number(), select(5)] }),
    goal: "compare",
    expected: "charts",
    basis: "cleveland-mcgill",
  },

  // Looking things up.
  {
    name: "Contacts",
    shape: shape({ objects: 200, fields: [text(), contact(), contact(), url(0.5), select(80)] }),
    expected: "table",
    basis: "few-table",
  },
  {
    name: "An inventory with many columns",
    shape: shape({
      objects: 400,
      fields: [text(), number(), number(), select(12), text(), text()],
    }),
    expected: "table",
    basis: "few-table",
  },
  {
    name: "A catalogue of books",
    shape: shape({
      objects: 150,
      fields: [text(), text(), text(), number(1, 90), select(20), url(0.4)],
    }),
    expected: "table",
    basis: "few-table",
  },
  {
    name: "A list of vendors",
    shape: shape({ objects: 30, fields: [text(), url(), contact(), contact()] }),
    expected: "table",
    basis: "few-table",
  },
  {
    name: "A small sheet of mixed values",
    shape: shape({ objects: 12, fields: [text(), number(1, 12), text(), select(9)] }),
    expected: "table",
    basis: "few-table",
  },
  {
    name: "Someone looking things up where a board would also do",
    shape: shape({ objects: 90, fields: [select(4), date(100), text()] }),
    goal: "lookup",
    expected: "table",
    basis: "few-table",
  },

  // Pictures.
  {
    name: "A set of photos",
    shape: shape({ objects: 300, fields: [file()] }),
    expected: "gallery",
    basis: "images",
  },
  {
    name: "A mood board's material",
    shape: shape({ objects: 40, fields: [file(), url(0.6)] }),
    expected: "gallery",
    basis: "images",
  },
  {
    name: "Recipes with pictures",
    shape: shape({ objects: 60, fields: [file(0.9), text(), select(5)] }),
    expected: "gallery",
    basis: "images",
  },
  {
    name: "Products with a picture and a price",
    shape: shape({ objects: 80, fields: [file(), number(), text()] }),
    expected: "gallery",
    basis: "images",
  },

  // Places.
  {
    name: "Places on a trip",
    shape: shape({ objects: 40, withGeo: 1 }),
    expected: "map",
    basis: "geo",
  },
  {
    name: "Customers with their locations",
    shape: shape({ objects: 300, withGeo: 0.9, fields: [select(6), text()] }),
    expected: "map",
    basis: "geo",
  },
  {
    name: "Field observations with places and dates",
    shape: shape({ objects: 1000, withGeo: 1, fields: [date(400), text()] }),
    expected: "map",
    basis: "geo",
  },
  {
    name: "Places, not all of them pinned yet",
    shape: shape({ objects: 20, withGeo: 0.6, fields: [text()] }),
    expected: "map",
    basis: "geo",
  },

  // Links.
  {
    name: "Research notes that link to each other",
    shape: shape({ objects: 500, withRelations: 0.8, fields: [text()] }),
    expected: "graph",
    basis: "network",
  },
  {
    name: "People and who knows whom",
    shape: shape({ objects: 100, withRelations: 0.9, fields: [text(), contact(0.5)] }),
    expected: "graph",
    basis: "network",
  },
  {
    name: "A wiki",
    shape: shape({ objects: 2000, withRelations: 0.6, withChildren: 0.1 }),
    expected: "graph",
    basis: "network",
  },

  // Nesting.
  {
    name: "The outline of a book",
    shape: shape({ objects: 60, withChildren: 0.4 }),
    expected: "outline",
    basis: "tree",
  },
  {
    name: "A brainstorm",
    shape: shape({ objects: 15, withChildren: 0.5 }),
    expected: "mindmap",
    basis: "tree",
  },
  {
    name: "A course of many lessons",
    shape: shape({ objects: 200, withChildren: 0.3 }),
    expected: "outline",
    basis: "tree",
  },
  {
    name: "Ideas branching out",
    shape: shape({ objects: 30, withChildren: 0.6 }),
    expected: "mindmap",
    basis: "tree",
  },

  // A page, and a free surface.
  {
    name: "A page with nothing under it",
    shape: shape({ objects: 0 }),
    expected: "document",
    basis: "plain",
  },
  {
    name: "A page with a couple of pages under it",
    shape: shape({ objects: 2 }),
    expected: "document",
    basis: "plain",
  },
  {
    name: "Someone arranging a few things by hand",
    shape: shape({ objects: 12 }),
    goal: "arrange",
    expected: "canvas",
    basis: "plain",
  },
  {
    name: "Someone arranging linked things by hand",
    shape: shape({ objects: 30, withRelations: 0.5 }),
    goal: "arrange",
    expected: "canvas",
    basis: "plain",
  },
];
