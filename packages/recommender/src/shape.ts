// SPDX-License-Identifier: AGPL-3.0-or-later
/** What the recommender is told about a set of objects (M8, Task 9). Shares run from 0 to 1. */

/** The views it can recommend, by their IDs in the registry. */
export type ViewName =
  | "document"
  | "outline"
  | "table"
  | "gallery"
  | "mindmap"
  | "kanban"
  | "calendar"
  | "timeline"
  | "charts"
  | "map"
  | "graph"
  | "canvas";

/** What someone says they are after. Nothing says they have to. */
export type Goal = "schedule" | "compare" | "organize" | "lookup" | "arrange";

export type FieldShapeKind =
  | "text"
  | "number"
  | "date"
  | "select"
  | "multi_select"
  | "boolean"
  | "file"
  | "url"
  | "contact"
  | "other";

export interface FieldShape {
  kind: FieldShapeKind;
  /** The share of objects with a value. */
  filled: number;
  /** How many different values there are. */
  distinct: number;
  /** For a date, how many days lie between the earliest and the latest. */
  rangeDays?: number;
}

export interface Shape {
  objects: number;
  /** The share with an event date, and how many days those run over. */
  withEvent: number;
  eventRangeDays: number;
  /** The share with an event date that also ends: a span and not a moment. */
  withSpan: number;
  /** The share with a location. */
  withGeo: number;
  /** The share with children of their own. */
  withChildren: number;
  /** The share with a live relation. */
  withRelations: number;
  fields: readonly FieldShape[];
}
