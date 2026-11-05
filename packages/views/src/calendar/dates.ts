// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * The Calendar's rules (M8, Task 5), apart from drawing it: days, what a month or week shows, the
 * conditions that read them, what a key does and what a move writes.
 *
 * A day is its text, YYYY-MM-DD, and a UTC day, as in the store's periods and the filter editor. A date
 * field holds no time zone, so a date is the same day for anyone who opens the vault. A week starts on
 * Monday, as the store's week does.
 */
import type {
  AggregateResult,
  Condition_Deserialize,
  FieldRow,
  Id,
  ObjectSummary_Serialize,
} from "@databaseme/client";

export type Day = string;
export type CalendarMode = "month" | "week";

/** The column that holds an object's event date. */
export const EVENT = "$event_start";

const DAY_MS = 86_400_000;

function msOf(day: Day): number {
  return Date.parse(`${day}T00:00:00Z`);
}

function dayOfMs(ms: number): Day {
  return new Date(ms).toISOString().slice(0, 10);
}

/** Whether text is a day that exists: 2026-02-30 is not. */
export function isDay(text: string): boolean {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(text)) return false;
  const ms = msOf(text);
  return !Number.isNaN(ms) && dayOfMs(ms) === text;
}

export function addDays(day: Day, count: number): Day {
  return dayOfMs(msOf(day) + count * DAY_MS);
}

/** The same day `count` months on, or the last day of a month too short to have it. */
export function addMonths(day: Day, count: number): Day {
  const from = new Date(msOf(day));
  const year = from.getUTCFullYear();
  const month = from.getUTCMonth() + count;
  const last = new Date(Date.UTC(year, month + 1, 0)).getUTCDate();
  return dayOfMs(Date.UTC(year, month, Math.min(from.getUTCDate(), last)));
}

/** The Monday of a day's week. */
export function startOfWeek(day: Day): Day {
  return addDays(day, -((new Date(msOf(day)).getUTCDay() + 6) % 7));
}

/** What today is where the person is, which is the day they mean by it. */
export function todayOf(now: Date = new Date()): Day {
  const month = String(now.getMonth() + 1).padStart(2, "0");
  const date = String(now.getDate()).padStart(2, "0");
  return `${String(now.getFullYear()).padStart(4, "0")}-${month}-${date}`;
}

/** The days of the month a day is in. */
export function monthDays(day: Day): Day[] {
  const first = `${day.slice(0, 7)}-01`;
  const length = new Date(
    Date.UTC(Number(day.slice(0, 4)), Number(day.slice(5, 7)), 0),
  ).getUTCDate();
  return Array.from({ length }, (_, index) => addDays(first, index));
}

/** The rows of the grid: one week, or every week that touches the month. */
export function weeksOf(mode: CalendarMode, anchor: Day): Day[][] {
  const week = (start: Day) => Array.from({ length: 7 }, (_, index) => addDays(start, index));
  if (mode === "week") return [week(startOfWeek(anchor))];
  const days = monthDays(anchor);
  const last = days.at(-1) ?? anchor;
  const weeks: Day[][] = [];
  for (
    let start = startOfWeek(anchor.slice(0, 7) + "-01");
    start <= last;
    start = addDays(start, 7)
  ) {
    weeks.push(week(start));
  }
  return weeks;
}

/** Where the previous and next buttons go. */
export function stepped(mode: CalendarMode, anchor: Day, count: number): Day {
  return mode === "month" ? addMonths(anchor, count) : addDays(anchor, count * 7);
}

/** The conditions for `first` up to but not including `end`. */
export function rangeConditions(key: string, first: Day, end: Day): Condition_Deserialize[] {
  const value = (day: Day) => (key === EVENT ? msOf(day) : day);
  return [
    { field: key, op: "ge", value: value(first) },
    { field: key, op: "lt", value: value(end) },
  ];
}

/** The day an object is on, by the event date or a date field. */
export function dayOf(summary: ObjectSummary_Serialize, key: string): Day | null {
  if (key === EVENT) {
    const start = summary.object.event_start;
    return start === null ? null : dayOfMs(start);
  }
  const value = summary.object.props_json[key];
  if (typeof value !== "string") return null;
  const day = value.slice(0, 10);
  return isDay(day) ? day : null;
}

export type DateWrite =
  | { kind: "event"; start: number; end: number | null }
  | { kind: "prop"; key: string; value: string };

/**
 * What moving an object to `target` writes. An event keeps its time of day and its length; a field
 * keeps whatever follows the date.
 */
export function dateWrite(summary: ObjectSummary_Serialize, key: string, target: Day): DateWrite {
  if (key === EVENT) {
    const { event_start: start, event_end: end } = summary.object;
    if (start === null) return { kind: "event", start: msOf(target), end: null };
    const by = msOf(target) - msOf(dayOfMs(start));
    return { kind: "event", start: start + by, end: end === null ? null : end + by };
  }
  const value = summary.object.props_json[key];
  return {
    kind: "prop",
    key,
    value: `${target}${typeof value === "string" ? value.slice(10) : ""}`,
  };
}

/** The day a key goes to from `day` in the grid; null for a key that is not the grid's. */
export function dayKey(key: string, day: Day, mode: CalendarMode): Day | null {
  switch (key) {
    case "ArrowLeft":
      return addDays(day, -1);
    case "ArrowRight":
      return addDays(day, 1);
    case "ArrowUp":
      return addDays(day, -7);
    case "ArrowDown":
      return addDays(day, 7);
    case "Home":
      return startOfWeek(day);
    case "End":
      return addDays(startOfWeek(day), 6);
    case "PageUp":
      return stepped(mode, day, -1);
    case "PageDown":
      return stepped(mode, day, 1);
    default:
      return null;
  }
}

/** How many days Alt and a key carry an item; null for a key that does not. */
export function carryKey(key: string): number | null {
  switch (key) {
    case "ArrowLeft":
      return -1;
    case "ArrowRight":
      return 1;
    case "ArrowUp":
      return -7;
    case "ArrowDown":
      return 7;
    default:
      return null;
  }
}

export interface DateChoice {
  key: string;
  label: string;
  /** The type a new object needs for this date to mean anything; none for the event date. */
  typeId: Id | null;
}

const EVENT_DATE: DateChoice = { key: EVENT, label: "Event date", typeId: null };

/** What a calendar can go by: the event date, and every date field its objects have. */
export function dateChoices(fields: readonly FieldRow[]): DateChoice[] {
  const seen = new Set<string>();
  const own: DateChoice[] = [];
  for (const field of fields) {
    if ((field.kind !== "date" && field.kind !== "datetime") || seen.has(field.key)) continue;
    seen.add(field.key);
    own.push({ key: field.key, label: field.label, typeId: field.type_id });
  }
  return [EVENT_DATE, ...own];
}

/** The date in use: the one chosen, else the first field, since a field someone added says more. */
export function chosenDate(choices: readonly DateChoice[], chosen: string | null): DateChoice {
  return choices.find((choice) => choice.key === chosen) ?? choices[1] ?? choices[0] ?? EVENT_DATE;
}

/** An aggregate grouped by day, as how many are on each. */
export function countsByDay(result: AggregateResult | undefined): Map<Day, number> {
  const counts = new Map<Day, number>();
  for (const row of result?.rows ?? []) {
    const [day] = row.keys;
    const [count] = row.values;
    if (typeof day === "string" && typeof count === "number") counts.set(day, count);
  }
  return counts;
}

function formatter(options: Intl.DateTimeFormatOptions): Intl.DateTimeFormat {
  return new Intl.DateTimeFormat(undefined, { timeZone: "UTC", ...options });
}

const LONG = formatter({ weekday: "long", year: "numeric", month: "long", day: "numeric" });
const MONTH = formatter({ year: "numeric", month: "long" });
const SPAN = formatter({ year: "numeric", month: "short", day: "numeric" });
const WEEKDAY = formatter({ weekday: "short" });

export const longDay = (day: Day): string => LONG.format(msOf(day));
export const weekdayShort = (day: Day): string => WEEKDAY.format(msOf(day));

/** What a view of the calendar is called. */
export function titleOf(mode: CalendarMode, anchor: Day): string {
  if (mode === "month") return MONTH.format(msOf(anchor));
  const first = startOfWeek(anchor);
  return SPAN.formatRange(msOf(first), msOf(addDays(first, 6)));
}
