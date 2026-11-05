// SPDX-License-Identifier: AGPL-3.0-or-later
import type { AggregateResult, FieldRow } from "@databaseme/client";
import { describe, expect, it } from "vitest";
import {
  chartAggregate,
  chartData,
  chartSpec,
  chartSummary,
  chartTitle,
  DEFAULT_CHART,
  groupChoices,
  marksFor,
  nextPeriod,
  numberChoices,
  periodLabel,
  resolveChart,
  seriesChoices,
  type ChartLook,
  type ChartSettings,
  type ChartSpec,
} from "./chart";

const field = (key: string, kind: string, options?: { id: string; label: string }[]) =>
  ({
    key,
    label: `${key.charAt(0).toUpperCase()}${key.slice(1)}`,
    kind,
    type_id: "t",
    options_json: options ? { options } : {},
  }) as unknown as FieldRow;

const status = field("status", "select", [
  { id: "todo", label: "To do" },
  { id: "doing", label: "Doing" },
  { id: "done", label: "Done" },
]);
const due = field("due", "date");
const estimate = field("estimate", "number");
const shipped = field("shipped", "boolean");
const notes = field("notes", "rich_text");
const fields = [notes, estimate, due, status, shipped];

const look: ChartLook = {
  series: ["#1", "#2", "#3", "#4", "#5", "#6", "#7", "#8"],
  neutral: "#n",
  surface: "#s",
  text: "#t",
  muted: "#m",
  grid: "#g",
  font: "sans",
};
const size = { width: 800, height: 400 };

const settings = (change: Partial<ChartSettings>): ChartSettings => ({
  ...DEFAULT_CHART,
  ...change,
});
const resolved = (change: Partial<ChartSettings>) => resolveChart(settings(change), fields);
const result = (rows: [unknown[], number | null][], truncated = false) =>
  ({
    rows: rows.map(([keys, value]) => ({ keys, values: [value] })),
    truncated,
  }) as unknown as AggregateResult;
const listing = { source: { kind: "type" as const, type_id: "t" }, conditions: [] };

describe("what a chart can go by", () => {
  it("groups by what has kinds worth grouping, and the event date", () => {
    expect(groupChoices(fields).map((one) => one.key)).toEqual([
      "estimate",
      "due",
      "status",
      "shipped",
      "$event_start",
    ]);
  });

  it("splits by names and measures numbers", () => {
    expect(seriesChoices(fields).map((one) => one.key)).toEqual(["status", "shipped"]);
    expect(numberChoices(fields).map((one) => one.key)).toEqual(["estimate"]);
  });

  it("offers a line only where its values run in an order", () => {
    expect(marksFor(resolved({ x: "due" }).x)).toEqual(["bar", "line", "area", "point"]);
    expect(marksFor(resolved({ x: "estimate" }).x)).toEqual(["bar", "line", "area", "point"]);
    expect(marksFor(resolved({ x: "status" }).x)).toEqual(["bar", "point"]);
  });
});

describe("settings made to fit the fields", () => {
  it("start from the first choices, then the first date", () => {
    expect(resolved({}).x.key).toBe("status");
    expect(resolveChart(DEFAULT_CHART, [notes, due]).x.key).toBe("due");
    expect(resolveChart(DEFAULT_CHART, []).x.key).toBe("$event_start");
  });

  it("fall back when what was saved is not there", () => {
    expect(resolved({ x: "gone" }).x.key).toBe("status");
    expect(resolved({ series: "gone" }).series).toBeNull();
  });

  it("do not draw a line along names", () => {
    expect(resolved({ x: "status", mark: "line" }).mark).toBe("bar");
    expect(resolved({ x: "due", mark: "line" }).mark).toBe("line");
  });

  it("count when there is no number to work on", () => {
    const chart = resolveChart(settings({ measure: "sum" }), [status]);
    expect(chart.measure).toBe("count");
    expect(chart.of).toBeNull();
  });

  it("do not split a field by itself", () => {
    expect(resolved({ x: "status", series: "status" }).series).toBeNull();
  });
});

describe("the read", () => {
  it("groups in the store and asks for a bounded number of buckets", () => {
    const query = chartAggregate(resolved({}), listing);
    expect(query.group_by).toEqual([{ field: "status", bin: null }]);
    expect(query.measures).toEqual([{ op: "count", field: null }]);
    expect(query.limit).toBe(500);
    expect(query.query.where).toEqual([]);
  });

  it("bins a date, and leaves out what has none to place it by", () => {
    const query = chartAggregate(resolved({ x: "due", bin: "week" }), listing);
    expect(query.group_by).toEqual([{ field: "due", bin: "week" }]);
    expect(query.query.where).toEqual([{ field: "due", op: "is_not_empty", value: null }]);
  });

  it("names the number a measure works on, and the series as a second group", () => {
    const query = chartAggregate(
      resolved({ measure: "avg", of: "estimate", series: "shipped" }),
      listing,
    );
    expect(query.measures).toEqual([{ op: "avg", field: "estimate" }]);
    expect(query.group_by).toEqual([
      { field: "status", bin: null },
      { field: "shipped", bin: null },
    ]);
  });
});

describe("periods", () => {
  it("are named in words", () => {
    expect(periodLabel("year", "2026")).toBe("2026");
    expect(periodLabel("quarter", "2026-Q4")).toBe("Q4 2026");
    expect(periodLabel("month", "2026-11")).toBe("Nov 2026");
    expect(periodLabel("day", "2026-11-04")).toBe("Nov 4, 2026");
  });

  it("run on across their ends", () => {
    expect(nextPeriod("year", "2026")).toBe("2027");
    expect(nextPeriod("quarter", "2026-Q3")).toBe("2026-Q4");
    expect(nextPeriod("quarter", "2026-Q4")).toBe("2027-Q1");
    expect(nextPeriod("month", "2026-09")).toBe("2026-10");
    expect(nextPeriod("month", "2026-12")).toBe("2027-01");
    expect(nextPeriod("week", "2026-11-02")).toBe("2026-11-09");
    expect(nextPeriod("day", "2026-12-31")).toBe("2027-01-01");
  });
});

describe("the rows", () => {
  it("name their categories and rank them, with no value last", () => {
    const data = chartData(
      resolved({}),
      result([
        [[null], 9],
        [["todo"], 1],
        [["doing"], 5],
        [["done"], 3],
      ]),
    );
    expect(data.categories).toEqual(["Doing", "Done", "To do", "No value"]);
    expect(data.series).toEqual([]);
  });

  it("say yes and no for a boolean, however the store returns it", () => {
    const data = chartData(
      resolved({ x: "shipped" }),
      result([
        [[1], 2],
        [[0], 1],
      ]),
    );
    expect(data.categories).toEqual(["Yes", "No"]);
  });

  it("keep numbers in the order of their values, not their counts", () => {
    const data = chartData(
      resolved({ x: "estimate" }),
      result([
        [[8], 1],
        [[1], 3],
        [[3], 2],
      ]),
    );
    expect(data.categories).toEqual(["1", "3", "8"]);
  });

  it("put the empty periods between on the axis, as zeros for a count", () => {
    const data = chartData(
      resolved({ x: "due" }),
      result([
        [["2026-11"], 2],
        [["2026-09"], 1],
      ]),
    );
    expect(data.categories).toEqual(["Sep 2026", "Oct 2026", "Nov 2026"]);
    expect(data.rows.map((row) => row.value)).toEqual([1, 0, 2]);
    // The table has only what the store returned.
    expect(data.table).toHaveLength(2);
  });

  it("leave a gap for a period with nothing to average", () => {
    const data = chartData(
      resolved({ x: "due", measure: "avg", of: "estimate" }),
      result([
        [["2026-11"], 2],
        [["2026-09"], 4],
      ]),
    );
    expect(data.rows.map((row) => row.value)).toEqual([4, null, 2]);
  });

  it("do not invent periods further apart than a chart can show", () => {
    const data = chartData(
      resolved({ x: "due", bin: "day" }),
      result([
        [["2000-01-01"], 1],
        [["2026-11-04"], 1],
      ]),
    );
    expect(data.categories).toHaveLength(2);
  });

  it("say when the store had more than a chart asks for", () => {
    expect(chartData(resolved({}), result([[["todo"], 1]], true)).truncated).toBe(true);
    expect(chartData(resolved({}), result([[["todo"], 1]])).truncated).toBe(false);
  });
});

describe("the series", () => {
  const split = resolved({ x: "due", series: "status" });

  it("keep the slot their field gives them, so a filter repaints nothing", () => {
    const all = chartData(
      split,
      result([
        [["2026-11", "todo"], 1],
        [["2026-11", "doing"], 1],
        [["2026-11", "done"], 1],
      ]),
    );
    expect(all.series).toEqual([
      { name: "To do", slot: 0 },
      { name: "Doing", slot: 1 },
      { name: "Done", slot: 2 },
    ]);
    const filtered = chartData(split, result([[["2026-11", "done"], 1]]));
    expect(filtered.series).toEqual([{ name: "Done", slot: 2 }]);
  });

  it("give no value a neutral, as it is not an identity", () => {
    const data = chartData(
      split,
      result([
        [["2026-11", null], 1],
        [["2026-11", "todo"], 1],
      ]),
    );
    expect(data.series).toEqual([
      { name: "To do", slot: 0 },
      { name: "No value", slot: null },
    ]);
  });

  const tag = field("tag", "text");
  const many = (count: number) => ({
    chart: resolveChart(settings({ x: "due", series: "tag" }), [due, tag]),
    rows: Array.from({ length: count }, (_, at): [unknown[], number] => [
      ["2026-11", `Tag ${String(at + 10)}`],
      count - at,
    ]),
  });

  it("fold the tail into Other past eight, adding it up, and the table keeps them all", () => {
    const { chart, rows } = many(10);
    const data = chartData(chart, result(rows));
    expect(data.series).toHaveLength(8);
    expect(data.series.at(-1)).toEqual({ name: "Other", slot: null });
    const other = data.rows.find((row) => row.series === "Other");
    expect(other?.value).toBe(3 + 2 + 1);
    expect(data.table).toHaveLength(10);
    expect(data.leftOut).toBe(0);
  });

  it("leave out rather than add up what cannot be added", () => {
    const base = many(10);
    const chart = resolveChart(
      settings({ x: "due", series: "tag", measure: "avg", of: "estimate" }),
      [due, estimate, field("tag", "text")],
    );
    const data = chartData(chart, result(base.rows));
    expect(data.series.map((one) => one.name)).not.toContain("Other");
    expect(data.series).toHaveLength(8);
    expect(data.leftOut).toBe(2);
    expect(data.table).toHaveLength(10);
  });

  it("are fewer for points, where every pair must differ", () => {
    const base = many(5);
    const chart = resolveChart(settings({ x: "due", series: "tag", mark: "point" }), [
      due,
      field("tag", "text"),
    ]);
    const data = chartData(chart, result(base.rows));
    expect(data.series).toHaveLength(3);
    expect(data.series.at(-1)?.name).toBe("Other");
    expect(data.series.slice(0, 2).map((one) => one.slot)).toEqual([0, 1]);
  });
});

describe("the spec", () => {
  const one = resolved({});
  const data = chartData(
    one,
    result([
      [["todo"], 1],
      [["done"], 3],
    ]),
  );
  const markOf = (spec: ChartSpec) =>
    (spec.mark ?? (spec.layer as ChartSpec[])[0]?.mark) as ChartSpec;
  const enc = (spec: ChartSpec) => spec.encoding as Record<string, ChartSpec>;

  it("names its rows and holds none of them", () => {
    const spec = chartSpec(one, data, look, size);
    expect(spec.data).toEqual({ name: "rows" });
    expect(JSON.stringify(spec)).not.toContain('"values"');
    expect(spec.$schema).toContain("vega-lite");
  });

  it("does no aggregating of its own", () => {
    const spec = JSON.stringify(chartSpec(one, data, look, size));
    expect(spec).not.toContain("aggregate");
    expect(spec).not.toContain("timeUnit");
    expect(spec).not.toContain('"url"');
  });

  it("ranks names along the side, and runs time along the bottom", () => {
    const names = enc(chartSpec(one, data, look, size));
    expect(names.y?.field).toBe("x");
    expect(names.y?.sort).toEqual(["Done", "To do"]);
    const time = resolved({ x: "due" });
    const along = enc(chartSpec(time, chartData(time, result([[["2026-11"], 1]])), look, size));
    expect(along.x?.field).toBe("x");
    expect(along.x?.type).toBe("ordinal");
  });

  it("caps a bar at 24 pixels with a rounded end, and thins it to fit", () => {
    const bar = markOf(chartSpec(one, data, look, size));
    expect(bar.size).toBe(24);
    expect(bar.cornerRadiusEnd).toBe(4);
    const time = resolved({ x: "due", bin: "day" });
    const rows: [unknown[], number][] = Array.from({ length: 200 }, (_, at) => [
      [
        `2026-${String(Math.floor(at / 28) + 1).padStart(2, "0")}-${String((at % 28) + 1).padStart(2, "0")}`,
      ],
      1,
    ]);
    const dense = markOf(chartSpec(time, chartData(time, result(rows)), look, size));
    expect(dense.size).toBeLessThan(24);
    expect(dense.size).toBeGreaterThanOrEqual(1);
  });

  it("carries one series in the first slot, with no legend", () => {
    const spec = chartSpec(one, data, look, size);
    expect(markOf(spec).color).toBe("#1");
    expect(enc(spec).color).toBeUndefined();
  });

  it("labels ranked bars at the tip, in text colour", () => {
    const spec = chartSpec(one, data, look, size);
    const layers = spec.layer as ChartSpec[];
    expect((layers[1]?.mark as ChartSpec).color).toBe("#t");
    expect(spec.mark).toBeUndefined();
  });

  const split = resolved({ x: "due", series: "status" });
  const splitData = chartData(
    split,
    result([
      [["2026-11", "todo"], 1],
      [["2026-11", "done"], 2],
    ]),
  );

  it("stacks what adds up, with a gap in the surface's colour, and a legend", () => {
    const spec = chartSpec(split, splitData, look, size);
    expect(enc(spec).y?.stack).toBe("zero");
    expect(markOf(spec).stroke).toBe("#s");
    expect(markOf(spec).strokeWidth).toBe(2);
    expect(markOf(spec).cornerRadiusEnd).toBeUndefined();
    const color = enc(spec).color as { scale: ChartSpec; legend: ChartSpec };
    expect(color.scale.domain).toEqual(["To do", "Done"]);
    expect(color.scale.range).toEqual(["#1", "#3"]);
    expect(color.legend.orient).toBe("top");
  });

  it("sets side by side what does not add up", () => {
    const chart = resolved({ x: "due", series: "status", measure: "avg", of: "estimate" });
    const spec = chartSpec(chart, chartData(chart, result([[["2026-11", "todo"], 1]])), look, size);
    expect(enc(spec).y?.stack).toBeNull();
    expect(enc(spec).xOffset?.field).toBe("series");
  });

  it("gives a line 2 pixels and an area a wash", () => {
    const time = resolved({ x: "due", mark: "line" });
    const line = markOf(chartSpec(time, chartData(time, result([[["2026-11"], 1]])), look, size));
    expect(line.strokeWidth).toBe(2);
    const under = resolved({ x: "due", mark: "area" });
    const area = markOf(chartSpec(under, chartData(under, result([[["2026-11"], 1]])), look, size));
    expect(area.fillOpacity).toBe(0.1);
  });

  it("keeps its markers as hit targets when they would crowd the line", () => {
    const time = resolved({ x: "due", mark: "line", bin: "day" });
    const few = chartData(time, result([[["2026-11-01"], 1]]));
    expect((markOf(chartSpec(time, few, look, size)).point as ChartSpec).opacity).toBeUndefined();
    const rows: [unknown[], number][] = Array.from({ length: 60 }, (_, at) => [
      [
        `2026-${String(Math.floor(at / 28) + 1).padStart(2, "0")}-${String((at % 28) + 1).padStart(2, "0")}`,
      ],
      1,
    ]);
    const dense = chartData(time, result(rows));
    expect((markOf(chartSpec(time, dense, look, size)).point as ChartSpec).opacity).toBe(0);
  });

  it("makes its ticks compact only where they reach ten thousand", () => {
    const small = enc(chartSpec(one, data, look, size)).x as { axis: ChartSpec };
    expect(small.axis.format).toBe(",~r");
    const big = chartData(one, result([[["todo"], 12_000]]));
    const large = enc(chartSpec(one, big, look, size)).x as { axis: ChartSpec };
    expect(large.axis.format).toBe("~s");
  });
});

describe("the chart in words", () => {
  it("names what it shows and its highest", () => {
    const chart = resolved({});
    const data = chartData(
      chart,
      result([
        [["todo"], 1],
        [["done"], 3],
      ]),
    );
    expect(chartTitle(chart)).toBe("Count by Status");
    expect(chartSummary(chart, data)).toBe(
      "Count by Status, as bars. 2 groups. The highest is Done, at 3.",
    );
    const split = resolved({ measure: "sum", of: "estimate", series: "shipped" });
    expect(chartTitle(split)).toBe("Sum of Estimate by Status, split by Shipped");
  });
});
