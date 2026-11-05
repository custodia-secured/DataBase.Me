// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * The acceptance line (M8): agreement with labelled fixtures of at least 80%. The labels are the author's
 * and not an independent expert's, as `fixtures.ts` says, so a pass says the rules do what their author
 * meant. M8.md records the figures, with what they can and cannot show.
 */
import { describe, expect, test } from "vitest";
import { date, file, FIXTURES, number, select, shape, text } from "./fixtures";
import { HOLDOUT } from "./fixtures.holdout";
import { recommend, suggestions } from "./recommend";
import { agreement } from "./score";
import type { ViewName } from "./shape";

const FLOOR = 0.8;
const viewsOf = (results: readonly { view: ViewName }[]) => results.map((result) => result.view);

describe("agreement", () => {
  test("the first recommendation agrees with the labels at least 80% of the time", () => {
    const scored = agreement(FIXTURES);
    expect(scored.share, JSON.stringify(scored.misses)).toBeGreaterThanOrEqual(FLOOR);
  });

  // A guard from here on, and no licence to tune against it: M8.md has what it scored the one time it told
  // us anything, with the rules already frozen.
  test("so it does on the set the rules were not adjusted against", () => {
    const scored = agreement(HOLDOUT);
    expect(scored.share, JSON.stringify(scored.misses)).toBeGreaterThanOrEqual(FLOOR);
  });

  test("the score counts what disagrees", () => {
    const fixture = FIXTURES[0];
    if (!fixture) throw new Error("no fixtures");
    const wrong = { ...fixture, expected: "canvas" as const };
    expect(agreement([fixture, wrong])).toMatchObject({ agreed: 1, total: 2, share: 0.5 });
    expect(agreement([]).share).toBe(0);
  });
});

describe("hard constraints", () => {
  const plain = shape({ objects: 50, fields: [text()] });

  test.each<[ViewName, string]>([
    ["calendar", "a date"],
    ["timeline", "a date"],
    ["map", "a location"],
    ["graph", "relations"],
    ["gallery", "files"],
    ["charts", "a number"],
    ["kanban", "a few states"],
    ["mindmap", "children"],
  ])("%s needs %s", (view) => {
    expect(viewsOf(recommend(plain))).not.toContain(view);
  });

  test("a value only some objects have does not count", () => {
    const seldom = shape({
      objects: 50,
      withEvent: 0.1,
      withGeo: 0.1,
      withRelations: 0.1,
      fields: [date(50, 0.1), select(4, 0.1), number(0.1), file(0.1)],
    });
    const views = viewsOf(recommend(seldom));
    for (const view of ["calendar", "timeline", "map", "graph", "kanban", "charts", "gallery"]) {
      expect(views).not.toContain(view);
    }
  });

  test("a number with one value has nothing to compare", () => {
    expect(
      viewsOf(recommend(shape({ objects: 30, fields: [number(1, 1), select(3)] }))),
    ).not.toContain("charts");
  });

  test("too many values are not columns", () => {
    expect(viewsOf(recommend(shape({ objects: 90, fields: [select(40)] })))).not.toContain(
      "kanban",
    );
  });

  test("a goal cannot bring back what the data rules out", () => {
    expect(viewsOf(recommend(plain, "schedule"))).not.toContain("calendar");
    expect(viewsOf(recommend(plain, "compare"))).not.toContain("charts");
  });
});

describe("what comes back", () => {
  const rich = shape({
    objects: 100,
    withEvent: 1,
    eventRangeDays: 200,
    withSpan: 1,
    withGeo: 1,
    withChildren: 0.5,
    withRelations: 1,
    fields: [select(4), number(), date(100), file(), text()],
  });

  test("every result has one sentence saying why", () => {
    const results = recommend(rich);
    expect(results.length).toBeGreaterThan(6);
    for (const result of results) {
      expect(result.why).toMatch(/^[A-Z].*\.$/);
      expect(result.why.match(/\. /g)).toBeNull();
    }
  });

  test("best first, and the same every time", () => {
    const results = recommend(rich);
    const scores = results.map((result) => result.score);
    expect(scores).toEqual([...scores].sort((a, b) => b - a));
    expect(recommend(rich)).toEqual(results);
  });

  test("a goal puts what it asks for first, if the data allows it", () => {
    expect(recommend(rich, "schedule")[0]?.view).toBe("calendar");
    expect(recommend(rich, "organize")[0]?.view).toBe("kanban");
  });

  test("what is left is not offered as a suggestion", () => {
    const nothing = shape({ objects: 0 });
    expect(viewsOf(suggestions(nothing))).toEqual(["document"]);
    const few = shape({ objects: 12 });
    expect(viewsOf(recommend(few)).length).toBeGreaterThan(0);
    expect(suggestions(few)).toEqual([]);
  });
});
