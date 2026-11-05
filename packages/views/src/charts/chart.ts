// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * The Charts' rules (M8, Task 7, ADR-0032), apart from drawing them: what a chart can go by, the
 * aggregate it asks the store for, the rows that come back as a chart and a table, and the Vega-Lite
 * spec that draws them. Nothing here loads Vega, so all of it is tested as plain data.
 *
 * The numbers come from the store (build plan §2.7): a spec never aggregates, and is drawn from rows
 * already counted. The colours follow what a series is, not where it ranks, so a filter does not
 * repaint what it leaves.
 */
import type {
  AggregateQuery_Deserialize,
  AggregateResult,
  FieldKind,
  FieldRow,
  MeasureOp,
  TimeBin,
  Value,
} from "@databaseme/client";
import { addDays, msOfDay } from "../calendar/dates";
import { listingQuery, narrowed, type Listing } from "../framework/listing";
import { optionsOf } from "../values";

export type ChartMark = "bar" | "line" | "area" | "point";

/** What a person chooses. A saved chart keeps these, and draws from them. */
export interface ChartSettings {
  mark: ChartMark;
  /** What each bar or point is, by field key; the first that fits until someone chooses. */
  x: string | null;
  /** The period, when `x` is a date. */
  bin: TimeBin;
  measure: MeasureOp;
  /** The number a measure other than a count works on. */
  of: string | null;
  /** What splits it into series; nothing for one series. */
  series: string | null;
}

export const DEFAULT_CHART: ChartSettings = {
  mark: "bar",
  x: null,
  bin: "month",
  measure: "count",
  of: null,
  series: null,
};

/** How many buckets a chart asks for. More would be neither readable nor quick. */
export const CHART_LIMIT = 500;

export const MARKS: readonly { id: ChartMark; label: string }[] = [
  { id: "bar", label: "Bars" },
  { id: "line", label: "Line" },
  { id: "area", label: "Area" },
  { id: "point", label: "Points" },
];

export const MEASURES: readonly { id: MeasureOp; label: string }[] = [
  { id: "count", label: "Count" },
  { id: "sum", label: "Sum" },
  { id: "avg", label: "Average" },
  { id: "min", label: "Smallest" },
  { id: "max", label: "Largest" },
];

export const BINS: readonly { id: TimeBin; label: string }[] = [
  { id: "day", label: "Days" },
  { id: "week", label: "Weeks" },
  { id: "month", label: "Months" },
  { id: "quarter", label: "Quarters" },
  { id: "year", label: "Years" },
];

export interface ChartField {
  key: string;
  label: string;
  kind: FieldKind | "event";
  /** The field behind it; none for a built-in date. */
  field: FieldRow | null;
}

const EVENT_FIELD: ChartField = {
  key: "$event_start",
  label: "Event date",
  kind: "event",
  field: null,
};

const NUMBERS: readonly ChartField["kind"][] = ["number", "currency", "duration", "rating"];
const CATEGORIES: readonly ChartField["kind"][] = ["select", "multi_select", "boolean", "text"];
const DATES: readonly ChartField["kind"][] = ["date", "datetime", "event"];

const isDate = (field: ChartField) => DATES.includes(field.kind);
const isNumber = (field: ChartField) => NUMBERS.includes(field.kind);

/** Whether a field's values run in an order, so that a line between them means something. */
export const isOrdered = (field: ChartField) => isDate(field) || isNumber(field);

function own(fields: readonly FieldRow[]): ChartField[] {
  const seen = new Set<string>();
  const result: ChartField[] = [];
  for (const field of fields) {
    if (seen.has(field.key)) continue;
    seen.add(field.key);
    result.push({ key: field.key, label: field.label, kind: field.kind, field });
  }
  return result;
}

/** What a chart can be grouped by: the fields that have kinds worth grouping, and the event date. */
export function groupChoices(fields: readonly FieldRow[]): ChartField[] {
  const usable = own(fields).filter((field) => CATEGORIES.includes(field.kind) || isOrdered(field));
  return [...usable, EVENT_FIELD];
}

/** What can split a chart into series: a set of names, not a run of values. */
export function seriesChoices(fields: readonly FieldRow[]): ChartField[] {
  return own(fields).filter((field) => CATEGORIES.includes(field.kind));
}

/** What a sum or an average can work on. */
export function numberChoices(fields: readonly FieldRow[]): ChartField[] {
  return own(fields).filter(isNumber);
}

export function marksFor(x: ChartField): readonly ChartMark[] {
  return isOrdered(x) ? ["bar", "line", "area", "point"] : ["bar", "point"];
}

export interface ResolvedChart {
  mark: ChartMark;
  x: ChartField;
  bin: TimeBin;
  measure: MeasureOp;
  of: ChartField | null;
  series: ChartField | null;
}

/** Settings made to fit the fields there are: a chart saved on others still draws something true. */
export function resolveChart(settings: ChartSettings, fields: readonly FieldRow[]): ResolvedChart {
  const groups = groupChoices(fields);
  const x =
    groups.find((field) => field.key === settings.x) ??
    groups.find((field) => field.kind === "select") ??
    groups.find((field) => DATES.includes(field.kind) && field.kind !== "event") ??
    groups[0] ??
    EVENT_FIELD;
  const numbers = numberChoices(fields);
  const of = numbers.find((field) => field.key === settings.of) ?? numbers[0] ?? null;
  const measure = of === null ? "count" : settings.measure;
  const series =
    seriesChoices(fields).find((field) => field.key === settings.series && field.key !== x.key) ??
    null;
  return {
    mark: marksFor(x).includes(settings.mark) ? settings.mark : "bar",
    x,
    bin: settings.bin,
    measure,
    of: measure === "count" ? null : of,
    series,
  };
}

/** The read a chart needs. A chart along time leaves out what has no date to place it by. */
export function chartAggregate(chart: ResolvedChart, listing: Listing): AggregateQuery_Deserialize {
  const placed = isDate(chart.x)
    ? narrowed(listing, { field: chart.x.key, op: "is_not_empty", value: null })
    : listing;
  return {
    query: listingQuery(placed),
    group_by: [
      { field: chart.x.key, bin: isDate(chart.x) ? chart.bin : null },
      ...(chart.series ? [{ field: chart.series.key, bin: null }] : []),
    ],
    measures: [
      chart.measure === "count"
        ? { op: "count", field: null }
        : { op: chart.measure, field: chart.of?.key ?? null },
    ],
    limit: CHART_LIMIT,
  };
}

const NO_VALUE = "No value";
const OTHER = "Other";

const MONTH = new Intl.DateTimeFormat(undefined, {
  timeZone: "UTC",
  year: "numeric",
  month: "short",
});
const DAY = new Intl.DateTimeFormat(undefined, {
  timeZone: "UTC",
  year: "numeric",
  month: "short",
  day: "numeric",
});

/** A period's key as the store groups it, in words. */
export function periodLabel(bin: TimeBin, key: string): string {
  switch (bin) {
    case "year":
      return key;
    case "quarter":
      return `${key.slice(5)} ${key.slice(0, 4)}`;
    case "month":
      return MONTH.format(msOfDay(`${key}-01`));
    case "hour":
      return key.replace("T", " ");
    case "day":
    case "week":
      return DAY.format(msOfDay(key));
  }
}

/** The key of the period after this one. */
export function nextPeriod(bin: TimeBin, key: string): string | null {
  const pad = (value: number) => String(value).padStart(2, "0");
  const year = Number(key.slice(0, 4));
  switch (bin) {
    case "year":
      return String(year + 1).padStart(4, "0");
    case "quarter": {
      const quarter = Number(key.slice(6));
      return quarter === 4
        ? `${String(year + 1).padStart(4, "0")}-Q1`
        : `${key.slice(0, 4)}-Q${String(quarter + 1)}`;
    }
    case "month": {
      const month = Number(key.slice(5, 7));
      return month === 12
        ? `${String(year + 1).padStart(4, "0")}-01`
        : `${key.slice(0, 4)}-${pad(month + 1)}`;
    }
    case "week":
      return addDays(key, 7);
    case "day":
      return addDays(key, 1);
    case "hour":
      return null;
  }
}

function fillPeriods(bin: TimeBin, keys: readonly string[]): string[] {
  const sorted = [...keys].sort();
  const [first] = sorted;
  const last = sorted.at(-1);
  if (first === undefined || last === undefined) return sorted;
  const filled: string[] = [];
  for (let key: string | null = first; key !== null && key <= last; key = nextPeriod(bin, key)) {
    // Further apart than a chart can show: the periods that have something are what it has.
    if (filled.length >= CHART_LIMIT) return sorted;
    filled.push(key);
  }
  return bin === "hour" ? sorted : filled;
}

/** A group's key as text. Lists are grouped item by item, so this is a word or a number. */
const keyText = (key: Value): string => (typeof key === "string" ? key : JSON.stringify(key));

function labelOf(field: ChartField, key: Value, bin: TimeBin): string {
  if (key === null) return NO_VALUE;
  if (isDate(field)) return periodLabel(bin, keyText(key));
  if (field.kind === "boolean") return key === true || key === 1 || key === "true" ? "Yes" : "No";
  if (field.field && (field.kind === "select" || field.kind === "multi_select")) {
    const option = optionsOf(field.field).find((one) => one.id === key);
    return option?.label ?? keyText(key);
  }
  return typeof key === "number" ? key.toLocaleString() : keyText(key);
}

export interface ChartRow {
  x: string;
  /** Empty for a chart of one series. */
  series: string;
  /** Where the series sits in a stack, so it does not move as values change. */
  order: number;
  /** No number to compute from leaves a gap, not a zero. */
  value: number | null;
}

export interface ChartData {
  rows: ChartRow[];
  /** Every row, whatever was folded for the picture: what the table shows. */
  table: ChartRow[];
  /** What the axis runs through, in order. */
  categories: string[];
  /** The series, in order, with the slot each takes; empty for one series. */
  series: { name: string; slot: number | null }[];
  /** Whether the store had more buckets than a chart asks for. */
  truncated: boolean;
  /** How many series a measure that cannot be added up left out of the picture. */
  leftOut: number;
}

/** How many series a chart can tell apart. Points overlap, so every pair must differ, not neighbours. */
const seriesCap = (mark: ChartMark) => (mark === "point" ? 3 : 8);

/** A count or a sum can be added up, so a tail folds into Other and a stack means a total. */
const isAdditive = (measure: MeasureOp) => measure === "count" || measure === "sum";

/**
 * The slots series take: what a field fixes (its choices in order; yes, then no) so that a filter
 * repaints nothing, or the order of the names. Past the cap, the first free one.
 */
function slotsOf(chart: ResolvedChart, names: readonly string[]): (number | null)[] {
  const cap = seriesCap(chart.mark);
  const fixed = (name: string): number | null => {
    const { series } = chart;
    if (series?.field && series.kind === "select") {
      const at = optionsOf(series.field).findIndex((option) => option.label === name);
      return at === -1 ? null : at;
    }
    if (series?.kind === "boolean") return name === "Yes" ? 0 : 1;
    return null;
  };
  const used = new Set<number>();
  const wanted = names.map((name) => {
    if (name === NO_VALUE || name === OTHER) return null;
    const slot = fixed(name);
    if (slot === null || slot >= cap || used.has(slot)) return -1;
    used.add(slot);
    return slot;
  });
  return wanted.map((slot) => {
    if (slot !== -1) return slot;
    for (let free = 0; free < cap; free++) {
      if (!used.has(free)) {
        used.add(free);
        return free;
      }
    }
    return null;
  });
}

/** What the store returned, as what a chart and a table show. */
export function chartData(chart: ResolvedChart, result: AggregateResult | undefined): ChartData {
  const { x, series, bin, measure } = chart;
  const raw = (result?.rows ?? []).map((row) => ({
    xKey: row.keys[0] ?? null,
    seriesKey: series ? (row.keys[1] ?? null) : null,
    value: row.values[0] ?? null,
  }));

  // The axis: periods in order with the empty ones between, values in order, else largest first.
  const totals = new Map<string, number>();
  const keys = new Map<string, Value>();
  for (const { xKey, value } of raw) {
    const label = labelOf(x, xKey, bin);
    keys.set(label, xKey);
    totals.set(label, (totals.get(label) ?? 0) + (value ?? 0));
  }
  let categories: string[];
  if (isDate(x)) {
    categories = fillPeriods(bin, [...keys.values()].map(keyText)).map((key) =>
      periodLabel(bin, key),
    );
  } else if (isNumber(x)) {
    categories = [...keys.entries()]
      .sort(([, a], [, b]) => Number(a) - Number(b))
      .map(([label]) => label);
  } else {
    categories = [...totals.entries()]
      .sort(([a, one], [b, two]) => {
        if ((a === NO_VALUE) !== (b === NO_VALUE)) return a === NO_VALUE ? 1 : -1;
        return two - one || a.localeCompare(b);
      })
      .map(([label]) => label);
  }

  // The series: what fits, in the order the field gives, with the rest folded or left out.
  const seriesTotals = new Map<string, number>();
  for (const { seriesKey, value } of raw) {
    if (!series) continue;
    const name = labelOf(series, seriesKey, bin);
    seriesTotals.set(name, (seriesTotals.get(name) ?? 0) + (value ?? 0));
  }
  const cap = seriesCap(chart.mark);
  const named = [...seriesTotals.keys()];
  const additive = isAdditive(measure);
  const kept = new Set<string>(
    named.length <= cap
      ? named
      : [...seriesTotals.entries()]
          .sort(([a, one], [b, two]) => two - one || a.localeCompare(b))
          .map(([name]) => name)
          .filter((name) => name !== NO_VALUE)
          .slice(0, additive ? cap - 1 : cap),
  );
  const folded = additive && named.length > cap;
  const rank = (name: string): number => {
    if (name === OTHER) return 1e9;
    if (name === NO_VALUE) return 1e9 - 1;
    const options = series?.field ? optionsOf(series.field) : [];
    const at = options.findIndex((option) => option.label === name);
    return at === -1 ? 1e6 : at;
  };
  const order = [...kept]
    .sort((a, b) => rank(a) - rank(b) || a.localeCompare(b))
    .concat(folded ? [OTHER] : []);
  const slots = slotsOf(chart, order);

  const table: ChartRow[] = raw.map(({ xKey, seriesKey, value }) => ({
    x: labelOf(x, xKey, bin),
    series: series ? labelOf(series, seriesKey, bin) : "",
    order: 0,
    value,
  }));
  table.sort(
    (a, b) =>
      categories.indexOf(a.x) - categories.indexOf(b.x) ||
      rank(a.series) - rank(b.series) ||
      a.series.localeCompare(b.series),
  );

  const combined = new Map<string, ChartRow>();
  for (const row of table) {
    let name = row.series;
    if (series && !kept.has(name)) {
      if (!folded) continue;
      name = OTHER;
    }
    const key = `${row.x}\u0000${name}`;
    const before = combined.get(key);
    if (before) before.value = (before.value ?? 0) + (row.value ?? 0);
    else
      combined.set(key, { x: row.x, series: name, order: order.indexOf(name), value: row.value });
  }

  // A period with nothing in it is on the axis too: nothing to count is a zero, nothing to average a gap.
  const rows = [...combined.values()];
  if (isDate(x)) {
    const names = series ? order : [""];
    for (const category of categories) {
      for (const name of names) {
        if (!combined.has(`${category}\u0000${name}`)) {
          rows.push({
            x: category,
            series: name,
            order: series ? order.indexOf(name) : 0,
            value: additive ? 0 : null,
          });
        }
      }
    }
    rows.sort((a, b) => categories.indexOf(a.x) - categories.indexOf(b.x) || a.order - b.order);
  }

  return {
    rows,
    table,
    categories,
    series: series ? order.map((name, at) => ({ name, slot: slots[at] ?? null })) : [],
    truncated: result?.truncated ?? false,
    leftOut: folded ? 0 : Math.max(0, named.length - kept.size),
  };
}

/** What the theme gives a chart: read from the tokens, so light and dark are each chosen, not flipped. */
export interface ChartLook {
  series: readonly string[];
  /** For No value and Other, which are not an identity. */
  neutral: string;
  surface: string;
  text: string;
  muted: string;
  grid: string;
  font: string;
}

/** What a measure is called on an axis. */
export function measureTitle(chart: ResolvedChart): string {
  const name = MEASURES.find((one) => one.id === chart.measure)?.label ?? "Count";
  return chart.of ? `${name} of ${chart.of.label}` : name;
}

/** The chart in words: its heading, and most of what a screen reader gets. */
export function chartTitle(chart: ResolvedChart): string {
  const by = chart.series ? `, split by ${chart.series.label}` : "";
  return `${measureTitle(chart)} by ${chart.x.label}${by}`;
}

/** What the picture shows, for anyone who is not looking at it. The table has every value. */
export function chartSummary(chart: ResolvedChart, data: ChartData): string {
  const kind = MARKS.find((one) => one.id === chart.mark)?.label ?? "Bars";
  const totals = new Map<string, number>();
  for (const row of data.rows) totals.set(row.x, (totals.get(row.x) ?? 0) + (row.value ?? 0));
  const top = [...totals.entries()].sort(([, a], [, b]) => b - a)[0];
  const highest = top ? ` The highest is ${top[0]}, at ${top[1].toLocaleString()}.` : "";
  return `${chartTitle(chart)}, as ${kind.toLowerCase()}. ${String(data.categories.length)} groups.${highest}`;
}

/** A Vega-Lite spec, as data: this file does not load the library that reads it. */
export type ChartSpec = Record<string, unknown>;

export interface ChartSize {
  width: number;
  height: number;
}

/** How far a mark may run across its band. Thick marks read loud, so the bar has a ceiling too. */
function barThickness(
  chart: ResolvedChart,
  data: ChartData,
  size: ChartSize,
  sideways: boolean,
): number {
  const room = Math.max(40, sideways ? size.height - 40 : size.width - 60);
  const band = room / Math.max(1, data.categories.length);
  const grouped = !isAdditive(chart.measure) && data.series.length > 1;
  const share = grouped ? (band * 0.8) / data.series.length : band * 0.7;
  return Math.max(1, Math.min(24, Math.floor(share)));
}

/**
 * The spec that draws a chart: its marks and scales and no data. It names its rows, so what a saved view
 * keeps is a chart and not a copy of the vault's numbers, and the rows go in when it is drawn.
 */
export function chartSpec(
  chart: ResolvedChart,
  data: ChartData,
  look: ChartLook,
  size: ChartSize,
): ChartSpec {
  const ordered = isOrdered(chart.x);
  const sideways = !ordered;
  const additive = isAdditive(chart.measure);
  const split = data.series.length > 0;
  const stacked = split && additive && (chart.mark === "bar" || chart.mark === "area");
  const grouped = split && !additive && chart.mark === "bar";
  const maximum = Math.max(0, ...data.rows.map((row) => row.value ?? 0));
  const format = maximum >= 10_000 ? "~s" : ",~r";
  const categoryChannel = sideways ? "y" : "x";
  const valueChannel = sideways ? "x" : "y";

  const encoding: ChartSpec = {
    [categoryChannel]: {
      field: "x",
      type: ordered ? "ordinal" : "nominal",
      sort: data.categories,
      title: chart.x.label,
      axis: { labelOverlap: true, labelLimit: 160, labelAngle: 0, ticks: false },
    },
    [valueChannel]: {
      field: "value",
      type: "quantitative",
      title: measureTitle(chart),
      stack: stacked ? "zero" : null,
      axis: {
        format,
        grid: true,
        tickCount: 5,
        ticks: false,
        domain: false,
        // There is no half of a thing to count.
        ...(chart.measure === "count" ? { tickMinStep: 1 } : {}),
      },
    },
    tooltip: [
      { field: "x", type: "nominal", title: chart.x.label },
      ...(split ? [{ field: "series", type: "nominal", title: chart.series?.label ?? "" }] : []),
      { field: "value", type: "quantitative", title: measureTitle(chart), format: ",~r" },
    ],
  };
  if (split) {
    encoding.color = {
      field: "series",
      type: "nominal",
      sort: data.series.map((one) => one.name),
      title: chart.series?.label ?? "",
      scale: {
        domain: data.series.map((one) => one.name),
        range: data.series.map((one) =>
          one.slot === null ? look.neutral : (look.series[one.slot] ?? look.neutral),
        ),
      },
      legend: { orient: "top", symbolType: chart.mark === "line" ? "stroke" : "circle" },
    };
    encoding.order = { field: "order", type: "quantitative" };
  }
  if (grouped) {
    encoding[sideways ? "yOffset" : "xOffset"] = {
      field: "series",
      type: "nominal",
      sort: data.series.map((one) => one.name),
    };
  }

  const color = split ? {} : { color: look.series[0] ?? look.neutral };
  // Past this, markers crowd the line; they stay, unseen, as what a pointer finds.
  const pointSize = 64;
  const marker =
    data.categories.length <= 40
      ? { filled: true, size: pointSize, stroke: look.surface, strokeWidth: 2 }
      : { filled: true, size: pointSize * 2, opacity: 0 };

  let mark: ChartSpec;
  switch (chart.mark) {
    case "bar":
      mark = {
        type: "bar",
        size: barThickness(chart, data, size, sideways),
        // A stack is one bar: its segments keep a gap in the surface's colour, not their own corners.
        ...(stacked ? { stroke: look.surface, strokeWidth: 2 } : { cornerRadiusEnd: 4 }),
        ...color,
      };
      break;
    case "line":
      mark = {
        type: "line",
        strokeWidth: 2,
        strokeCap: "round",
        strokeJoin: "round",
        point: marker,
        ...color,
      };
      break;
    case "area":
      mark = {
        type: "area",
        // A wash under its line, never a block.
        fillOpacity: 0.1,
        line: { strokeWidth: 2, strokeCap: "round", strokeJoin: "round" },
        point: marker,
        ...color,
      };
      break;
    case "point":
      mark = {
        type: "point",
        filled: true,
        size: pointSize,
        stroke: look.surface,
        strokeWidth: 2,
        opacity: 1,
        ...color,
      };
      break;
  }

  const base: ChartSpec = {
    $schema: "https://vega.github.io/schema/vega-lite/v6.json",
    description: chartTitle(chart),
    data: { name: "rows" },
    config: {
      background: null,
      view: { stroke: null },
      aria: false,
      font: look.font,
      axis: {
        labelColor: look.muted,
        titleColor: look.muted,
        gridColor: look.grid,
        domainColor: look.grid,
        labelFontSize: 12,
        titleFontSize: 12,
        titleFontWeight: "normal",
        grid: false,
      },
      legend: {
        labelColor: look.text,
        titleColor: look.muted,
        labelFontSize: 12,
        titleFontSize: 12,
        titleFontWeight: "normal",
      },
    },
  };

  // Ranked bars of one series carry their value at the tip, where the eye already is.
  if (chart.mark === "bar" && sideways && !split && data.categories.length <= 12) {
    return {
      ...base,
      encoding,
      layer: [
        { mark },
        {
          mark: { type: "text", align: "left", baseline: "middle", dx: 4, color: look.text },
          encoding: { text: { field: "value", type: "quantitative", format: ",~r" } },
        },
      ],
    };
  }
  return { ...base, mark, encoding };
}
