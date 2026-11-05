// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * Which views suit a set of objects (M8, Task 9). Hard constraints rule a view out, soft scores rank what
 * is left, and every result carries one sentence saying why, as Draco separates the two (Moritz et al.,
 * 2019). It never decides anything by itself: the app offers what comes back as "Suggested".
 *
 * The scores are not measurements. They are this file's way of saying which reason outranks which, written
 * against `fixtures.ts`, and only the order they produce means anything.
 */
import type { FieldShape, Goal, Shape, ViewName } from "./shape";

export interface Recommendation {
  view: ViewName;
  score: number;
  /** One sentence for the person, in plain words. */
  why: string;
}

/** A share at which something is true of the objects as a whole. */
const MOSTLY = 0.5;
/** Past this many days, the distances between dates are what is worth seeing. */
const LONG_RANGE_DAYS = 3 * 365;
/** How many states fit as columns on a board. */
const MOST_COLUMNS = 8;
/** How many different values can still group a chart. */
const MOST_GROUPS = 20;
/** How many items a radial map holds before an indented list reads better. */
const MOST_ON_A_MAP = 40;
/** How many objects are few enough to be one page. */
const A_FEW = 2;
/** At and past this many attributes, a row of them is what someone reads. */
const MANY_FIELDS = 4;

const filledFields = (shape: Shape, kind: FieldShape["kind"]) =>
  shape.fields.filter((field) => field.kind === kind && field.filled >= MOSTLY);

/** Whether most objects have a date, as an event or in a field, and how far the widest dates run. */
function datesOf(shape: Shape): { hasDates: boolean; rangeDays: number } {
  const fields = filledFields(shape, "date");
  const events = shape.withEvent >= MOSTLY;
  return {
    hasDates: events || fields.length > 0,
    rangeDays: Math.max(
      events ? shape.eventRangeDays : 0,
      ...fields.map((field) => field.rangeDays ?? 0),
    ),
  };
}

/** A set of states: choices few enough for columns, that most objects have chosen from. */
function statesOf(shape: Shape): FieldShape | undefined {
  return filledFields(shape, "select").find(
    (field) => field.distinct >= 2 && field.distinct <= MOST_COLUMNS,
  );
}

function hasGroup(shape: Shape): boolean {
  return shape.fields.some(
    (field) =>
      (field.kind === "select" || field.kind === "multi_select" || field.kind === "boolean") &&
      field.filled >= MOSTLY &&
      field.distinct >= 2 &&
      field.distinct <= MOST_GROUPS,
  );
}

function scoreOf(view: ViewName, shape: Shape): Recommendation | null {
  const { objects } = shape;
  const { hasDates, rangeDays } = datesOf(shape);
  const numbers = filledFields(shape, "number");
  const measured = numbers.filter((field) => field.distinct > 1);
  const pictures = filledFields(shape, "file").length > 0;
  const states = statesOf(shape);
  const fieldCount = shape.fields.filter((field) => field.filled >= MOSTLY).length;
  const nested = shape.withChildren >= 0.2;
  const linked = shape.withRelations >= MOSTLY;
  const placed = shape.withGeo >= MOSTLY;
  const bare = fieldCount === 0;

  const result = (score: number, why: string): Recommendation => ({ view, score, why });

  switch (view) {
    case "map":
      if (!placed) return null;
      return result(90, "Most of these have a location, so a map shows where they are.");

    case "gallery":
      if (!pictures) return null;
      return result(85, "Most of these have a picture, and pictures are found by looking at them.");

    case "graph":
      if (!linked || objects <= 2) return null;
      return result(80, "Most of these link to one another, and a graph shows those links.");

    case "timeline":
      if (!hasDates) return null;
      if (shape.withSpan >= MOSTLY) {
        return result(82, "These have a start and an end, so a timeline shows how long each runs.");
      }
      if (rangeDays >= LONG_RANGE_DAYS) {
        return result(75, "These dates run over years, which a timeline shows to scale.");
      }
      return result(30, "A timeline shows how far apart these dates are.");

    case "charts": {
      if (measured.length === 0) return null;
      if (objects < 2) return null;
      const by = hasDates || hasGroup(shape);
      if (!by) return result(25, "A chart can show how these figures are spread.");
      // A number among a good many other attributes is one more thing to read in a row.
      if (fieldCount >= MANY_FIELDS) return result(45, "A chart shows how these figures compare.");
      return result(70, "These have figures to compare, and position on a scale reads best.");
    }

    case "kanban": {
      if (!states) return null;
      if (objects < 2) return null;
      // With a handful of attributes, the columns would leave most of what matters out.
      if (fieldCount >= MANY_FIELDS) {
        return result(40, "A board shows these by the state each is in.");
      }
      return result(65, "These have a few states to be in, which a board shows as columns.");
    }

    case "calendar":
      if (!hasDates) return null;
      if (rangeDays >= LONG_RANGE_DAYS) return result(35, "A calendar shows these on their days.");
      return result(60, "These fall on days within a year or so, which a calendar lays out.");

    case "table": {
      if (bare || objects === 0) return null;
      if (fieldCount >= MANY_FIELDS) {
        return result(
          55,
          "These have several attributes each, which a table lets you read across.",
        );
      }
      return result(35, "A table lists these with their values.");
    }

    case "mindmap":
      if (!nested) return null;
      if (objects > MOST_ON_A_MAP) return result(30, "A mind map shows how these branch.");
      return result(58, "These are few and branch from one another, which a mind map shows.");

    case "outline":
      if (!nested) return result(10, "An outline lists these in order.");
      return result(
        objects > MOST_ON_A_MAP ? 58 : 45,
        "These nest, which an outline reads through.",
      );

    case "document":
      if (objects <= A_FEW && bare) {
        return result(50, "There is little under this page, so it is best read as a page.");
      }
      return result(5, "This page's own text is here.");

    case "canvas":
      // A free surface answers someone wanting to arrange things, which nothing in the data can say.
      if (objects === 0) return null;
      return result(8, "A canvas lets you place these by hand.");
  }
}

/** What each goal asks for, added to a view that can already do it. */
const GOAL_FAVOURS: Record<Goal, readonly ViewName[]> = {
  schedule: ["calendar"],
  compare: ["charts"],
  organize: ["kanban"],
  lookup: ["table"],
  arrange: ["canvas"],
};

const GOAL_WEIGHT = 100;

/** Under this, a view could show these objects, and nothing says it should. */
const A_REASON = 20;

const ALL_VIEWS: readonly ViewName[] = [
  "document",
  "outline",
  "table",
  "gallery",
  "mindmap",
  "kanban",
  "calendar",
  "timeline",
  "charts",
  "map",
  "graph",
  "canvas",
];

/** What is worth offering: the views there is a reason to pick, best first. */
export function suggestions(shape: Shape, goal?: Goal): Recommendation[] {
  return recommend(shape, goal).filter((result) => result.score >= A_REASON);
}

/** The views that can show `shape`, best first. What cannot show it at all is not there. */
export function recommend(shape: Shape, goal?: Goal): Recommendation[] {
  const results: Recommendation[] = [];
  for (const view of ALL_VIEWS) {
    const scored = scoreOf(view, shape);
    if (!scored) continue;
    const favoured = goal !== undefined && GOAL_FAVOURS[goal].includes(view);
    results.push(favoured ? { ...scored, score: scored.score + GOAL_WEIGHT } : scored);
  }
  return results.sort(
    (a, b) => b.score - a.score || ALL_VIEWS.indexOf(a.view) - ALL_VIEWS.indexOf(b.view),
  );
}
