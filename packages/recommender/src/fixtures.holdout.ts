// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * The held-out set (M8, Task 9): written with the others, before any rule, and never used to tune one.
 * The rules are written and adjusted against `fixtures.ts` alone, and this one is scored once they
 * are done, so its figure says something the other cannot about how they do on what they were not fitted
 * to. The labels are still the author's, as `fixtures.ts` says.
 *
 * Changing a rule after seeing a failure here makes this set one more that was tuned against; M8.md says
 * if that ever happened.
 */
import { contact, date, file, number, select, shape, text, type Fixture } from "./fixtures";

export const HOLDOUT: readonly Fixture[] = [
  {
    name: "Job applications",
    shape: shape({ objects: 35, fields: [select(5), date(60, 0.3), text()] }),
    expected: "kanban",
    basis: "kanban",
  },
  {
    name: "A sprint board",
    shape: shape({ objects: 18, fields: [select(3), number(0.8, 6)] }),
    expected: "kanban",
    basis: "kanban",
  },
  {
    name: "A class timetable",
    shape: shape({ objects: 60, withEvent: 1, eventRangeDays: 120 }),
    expected: "calendar",
    basis: "time",
  },
  {
    name: "Dates of a short course",
    shape: shape({ objects: 14, fields: [date(100)] }),
    expected: "calendar",
    basis: "time",
  },
  {
    name: "A company's history",
    shape: shape({ objects: 40, fields: [date(18_000), text()] }),
    expected: "timeline",
    basis: "time",
  },
  {
    name: "Stages of a renovation",
    shape: shape({ objects: 8, withEvent: 1, eventRangeDays: 200, withSpan: 1 }),
    expected: "timeline",
    basis: "time",
  },
  {
    name: "Invoices",
    shape: shape({ objects: 800, fields: [number(), date(700), select(3)] }),
    expected: "charts",
    basis: "cleveland-mcgill",
  },
  {
    name: "A weight log",
    shape: shape({ objects: 400, fields: [number(), date(800)] }),
    expected: "charts",
    basis: "cleveland-mcgill",
  },
  {
    name: "Library members",
    shape: shape({
      objects: 300,
      fields: [text(), text(), text(), contact(), contact(), select(4)],
    }),
    expected: "table",
    basis: "few-table",
  },
  {
    name: "A wine cellar",
    shape: shape({
      objects: 120,
      fields: [text(), select(30), number(1, 30), number(1, 60), text(0.6), text(0.3)],
    }),
    expected: "table",
    basis: "few-table",
  },
  {
    name: "Dates on hardly anything",
    shape: shape({ objects: 100, fields: [date(50, 0.05), text(), text(), text()] }),
    expected: "table",
    basis: "few-table",
  },
  {
    name: "An album of scans",
    shape: shape({ objects: 500, fields: [file(), date(3000)] }),
    expected: "gallery",
    basis: "images",
  },
  {
    name: "Art references",
    shape: shape({ objects: 200, fields: [file(0.95), select(6)] }),
    expected: "gallery",
    basis: "images",
  },
  {
    name: "A hiking log",
    shape: shape({ objects: 70, withGeo: 1, fields: [number(), date(500)] }),
    expected: "map",
    basis: "geo",
  },
  {
    name: "Cafes to visit",
    shape: shape({ objects: 25, withGeo: 1, fields: [select(3)] }),
    expected: "map",
    basis: "geo",
  },
  {
    name: "A slip box of notes",
    shape: shape({ objects: 900, withRelations: 0.75 }),
    expected: "graph",
    basis: "network",
  },
  {
    name: "The chapters of a thesis",
    shape: shape({ objects: 45, withChildren: 0.35 }),
    expected: "outline",
    basis: "tree",
  },
  {
    name: "Plot ideas",
    shape: shape({ objects: 9, withChildren: 0.6 }),
    expected: "mindmap",
    basis: "tree",
  },
  {
    name: "An empty page",
    shape: shape({ objects: 0, fields: [text(0, 0)] }),
    expected: "document",
    basis: "plain",
  },
  {
    name: "Someone arranging a handful of things",
    shape: shape({ objects: 6 }),
    goal: "arrange",
    expected: "canvas",
    basis: "plain",
  },
];
