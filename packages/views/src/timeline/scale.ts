// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * The Timeline's rules (M8, Task 6), apart from drawing it: the zoom levels, which bucket sits where
 * along the lane, how bars share it, and what a key does. A bucket's place is arithmetic, so the lane
 * holds a century of days and draws only what is on screen.
 */
import type { AggregateResult, TimeBin } from "@databaseme/client";
import { addDays, longDay, msOfDay, startOfWeek, type Day } from "../calendar/dates";
import type { Bucket } from "../framework/useBuckets";

export type LevelId = "day" | "week" | "month" | "year" | "decade";

export interface Level {
  id: LevelId;
  label: string;
  /** The period the store groups by. */
  bin: TimeBin;
  /** How long a bucket is along the lane, in pixels, across a screen and down a phone. */
  size: number;
  sizeVertical: number;
  /** Whether individual objects are drawn; further out, only how many. */
  items: boolean;
}

/** From days to decades (M8's design). A decade's buckets are still years, drawn narrow. */
export const LEVELS: readonly Level[] = [
  { id: "day", label: "Days", bin: "day", size: 96, sizeVertical: 72, items: true },
  { id: "week", label: "Weeks", bin: "week", size: 120, sizeVertical: 88, items: true },
  { id: "month", label: "Months", bin: "month", size: 96, sizeVertical: 56, items: false },
  { id: "year", label: "Years", bin: "year", size: 72, sizeVertical: 48, items: false },
  { id: "decade", label: "Decades", bin: "year", size: 24, sizeVertical: 28, items: false },
];

const MONTHS_LEVEL: Level = {
  id: "month",
  label: "Months",
  bin: "month",
  size: 96,
  sizeVertical: 56,
  items: false,
};

export function levelOf(id: string): Level {
  return LEVELS.find((level) => level.id === id) ?? MONTHS_LEVEL;
}

/** The level `by` steps along from `id`: in is negative. It stops at either end. */
export function zoomed(id: LevelId, by: number): Level {
  const at = LEVELS.findIndex((level) => level.id === id);
  return LEVELS[Math.min(LEVELS.length - 1, Math.max(0, at + by))] ?? MONTHS_LEVEL;
}

/** The whole years the lane spans. */
export interface Domain {
  firstYear: number;
  lastYear: number;
}

/**
 * The years an aggregate by year found and this one, so Today always has somewhere to go, and a year
 * beyond each end, so the first and last things can sit in the middle of the lane and there is room to
 * look ahead. A count per year is the cheapest way to find the ends, and it works for columns and
 * fields alike.
 */
export function domainOf(result: AggregateResult | undefined, today: Day): Domain {
  const years = [Number(today.slice(0, 4))];
  for (const row of result?.rows ?? []) {
    const [key] = row.keys;
    const year = typeof key === "string" ? Number(key) : Number.NaN;
    if (Number.isInteger(year)) years.push(year);
  }
  return { firstYear: Math.max(0, Math.min(...years) - 1), lastYear: Math.max(...years) + 1 };
}

const yearText = (year: number) => String(year).padStart(4, "0");
const pad = (value: number) => String(value).padStart(2, "0");
const firstDay = (domain: Domain): Day => `${yearText(domain.firstYear)}-01-01`;
const endDay = (domain: Domain): Day => `${yearText(domain.lastYear + 1)}-01-01`;

const DAY_MS = 86_400_000;

/** Where days are counted from: the lane's first day, or the Monday of its first week. */
function origin(level: Level, domain: Domain): Day {
  return level.id === "week" ? startOfWeek(firstDay(domain)) : firstDay(domain);
}

const daysBetween = (from: Day, to: Day) => Math.round((msOfDay(to) - msOfDay(from)) / DAY_MS);

export function bucketCount(level: Level, domain: Domain): number {
  const years = domain.lastYear - domain.firstYear + 1;
  switch (level.id) {
    case "day":
      return daysBetween(firstDay(domain), endDay(domain));
    case "week":
      return Math.ceil(daysBetween(origin(level, domain), endDay(domain)) / 7);
    case "month":
      return years * 12;
    case "year":
    case "decade":
      return years;
  }
}

/** The bucket at a place along the lane. */
export function bucketAt(level: Level, domain: Domain, index: number): Bucket {
  switch (level.id) {
    case "day": {
      const start = addDays(origin(level, domain), index);
      return { key: start, start, end: addDays(start, 1) };
    }
    case "week": {
      const start = addDays(origin(level, domain), index * 7);
      return { key: start, start, end: addDays(start, 7) };
    }
    case "month": {
      const months = domain.firstYear * 12 + index;
      const key = `${yearText(Math.floor(months / 12))}-${pad((months % 12) + 1)}`;
      const after = months + 1;
      return {
        key,
        start: `${key}-01`,
        end: `${yearText(Math.floor(after / 12))}-${pad((after % 12) + 1)}-01`,
      };
    }
    case "year":
    case "decade": {
      const key = yearText(domain.firstYear + index);
      return { key, start: `${key}-01-01`, end: `${yearText(domain.firstYear + index + 1)}-01-01` };
    }
  }
}

/** The place along the lane of the bucket a day is in. */
export function indexOfDay(level: Level, domain: Domain, day: Day): number {
  const year = Number(day.slice(0, 4));
  switch (level.id) {
    case "day":
      return daysBetween(origin(level, domain), day);
    case "week":
      return Math.floor(daysBetween(origin(level, domain), day) / 7);
    case "month":
      return (year - domain.firstYear) * 12 + Number(day.slice(5, 7)) - 1;
    case "year":
    case "decade":
      return year - domain.firstYear;
  }
}

/** How far along the lane a moment is, where a level draws objects: days and weeks are all one length. */
export function offsetOf(level: Level, domain: Domain, ms: number, size: number): number {
  const length = level.id === "week" ? 7 * DAY_MS : DAY_MS;
  return ((ms - msOfDay(origin(level, domain))) / length) * size;
}

/**
 * The buckets to read while `first` to `last` are on screen. Reads move in whole chunks, so scrolling
 * asks for nothing new until the screen leaves what has been read.
 */
export function readWindow(
  first: number,
  last: number,
  chunk: number,
  count: number,
): { from: number; to: number } {
  const from = Math.floor(first / chunk) * chunk;
  const to = (Math.floor(last / chunk) + 1) * chunk;
  return { from: Math.max(0, from), to: Math.min(count, to) };
}

export interface Span {
  id: string;
  start: number;
  length: number;
}

/**
 * Bars in lanes, each in the first lane free when it starts, in order of time. What finds no lane is
 * left out and counted, so a crowded stretch says so rather than drawing over itself.
 */
export function layOut<T extends Span>(
  spans: readonly T[],
  maxLanes: number,
): { placed: { span: T; lane: number }[]; hidden: T[] } {
  const sorted = [...spans].sort((a, b) => a.start - b.start || a.id.localeCompare(b.id));
  const ends: number[] = [];
  const placed: { span: T; lane: number }[] = [];
  const hidden: T[] = [];
  for (const span of sorted) {
    let lane = ends.findIndex((end) => end <= span.start);
    if (lane === -1) {
      if (ends.length >= maxLanes) {
        hidden.push(span);
        continue;
      }
      lane = ends.length;
    }
    ends[lane] = span.start + span.length;
    placed.push({ span, lane });
  }
  return { placed, hidden };
}

export type LaneKey =
  | { kind: "step"; by: 1 | -1 }
  | { kind: "page"; by: 1 | -1 }
  | { kind: "edge"; to: "first" | "last" }
  | { kind: "zoom"; by: 1 | -1 }
  | { kind: "act" }
  | null;

/** What a key does in the lane. */
export function laneKey(key: string): LaneKey {
  switch (key) {
    case "ArrowRight":
    case "ArrowDown":
      return { kind: "step", by: 1 };
    case "ArrowLeft":
    case "ArrowUp":
      return { kind: "step", by: -1 };
    case "PageDown":
      return { kind: "page", by: 1 };
    case "PageUp":
      return { kind: "page", by: -1 };
    case "Home":
      return { kind: "edge", to: "first" };
    case "End":
      return { kind: "edge", to: "last" };
    case "+":
    case "=":
      return { kind: "zoom", by: -1 };
    case "-":
      return { kind: "zoom", by: 1 };
    case "Enter":
      return { kind: "act" };
    default:
      return null;
  }
}

const MONTH_LONG = new Intl.DateTimeFormat(undefined, {
  timeZone: "UTC",
  year: "numeric",
  month: "long",
});
const MONTH_SHORT = new Intl.DateTimeFormat(undefined, {
  timeZone: "UTC",
  year: "numeric",
  month: "short",
});
const DAY_SHORT = new Intl.DateTimeFormat(undefined, {
  timeZone: "UTC",
  month: "short",
  day: "numeric",
});

/** What the axis prints. A decade's narrow years are named only as a decade starts. */
export function axisLabel(level: Level, bucket: Bucket): string {
  const ms = msOfDay(bucket.start);
  switch (level.id) {
    case "day":
    case "week":
      return DAY_SHORT.format(ms);
    case "month":
      return MONTH_SHORT.format(ms);
    case "year":
      return bucket.key;
    case "decade":
      return Number(bucket.key) % 10 === 0 ? bucket.key : "";
  }
}

/** What a bucket is called in words. */
export function bucketName(level: Level, bucket: Bucket): string {
  const ms = msOfDay(bucket.start);
  switch (level.id) {
    case "day":
      return longDay(bucket.start);
    case "week":
      return `The week of ${longDay(bucket.start)}`;
    case "month":
      return MONTH_LONG.format(ms);
    case "year":
    case "decade":
      return bucket.key;
  }
}
