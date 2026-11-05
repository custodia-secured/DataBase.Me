// SPDX-License-Identifier: AGPL-3.0-or-later
import type { FieldRow, ObjectSummary_Serialize } from "@databaseme/client";
import { describe, expect, it } from "vitest";
import {
  addDays,
  addMonths,
  carryKey,
  chosenDate,
  countsByDay,
  dateChoices,
  dateWrite,
  dayKey,
  dayOf,
  EVENT,
  isDay,
  monthDays,
  rangeConditions,
  startOfWeek,
  stepped,
  todayOf,
  weeksOf,
} from "./dates";

function summary(object: {
  event_start?: number | null;
  event_end?: number | null;
  props_json?: Record<string, unknown>;
}): ObjectSummary_Serialize {
  return {
    object: { event_start: null, event_end: null, props_json: {}, ...object },
  } as unknown as ObjectSummary_Serialize;
}

const at = (text: string) => Date.parse(text);

describe("days", () => {
  it("know which text is a day that exists", () => {
    expect(isDay("2026-11-04")).toBe(true);
    expect(isDay("2028-02-29")).toBe(true);
    expect(isDay("2026-02-30")).toBe(false);
    expect(isDay("2026-11-4")).toBe(false);
    expect(isDay("")).toBe(false);
  });

  it("add across months and years", () => {
    expect(addDays("2026-12-31", 1)).toBe("2027-01-01");
    expect(addDays("2026-03-01", -1)).toBe("2026-02-28");
  });

  it("add months, stopping at the end of a short one", () => {
    expect(addMonths("2026-01-31", 1)).toBe("2026-02-28");
    expect(addMonths("2026-12-15", 1)).toBe("2027-01-15");
    expect(addMonths("2026-01-15", -1)).toBe("2025-12-15");
  });

  it("start their week on Monday", () => {
    expect(startOfWeek("2026-11-04")).toBe("2026-11-02");
    expect(startOfWeek("2026-11-02")).toBe("2026-11-02");
    expect(startOfWeek("2026-11-01")).toBe("2026-10-26");
  });

  it("say today where the person is, not in UTC", () => {
    expect(todayOf(new Date(2026, 10, 4, 23, 30))).toBe("2026-11-04");
    expect(todayOf(new Date(2026, 0, 1, 0, 30))).toBe("2026-01-01");
  });
});

describe("what a view shows", () => {
  it("is every week that touches the month", () => {
    const november = weeksOf("month", "2026-11-15");
    expect(november).toHaveLength(6);
    expect(november[0]?.[0]).toBe("2026-10-26");
    expect(november.at(-1)?.at(-1)).toBe("2026-12-06");
  });

  it("is four weeks for a February that starts on a Monday", () => {
    const february = weeksOf("month", "2027-02-10");
    expect(february).toHaveLength(4);
    expect(february[0]?.[0]).toBe("2027-02-01");
    expect(february.at(-1)?.at(-1)).toBe("2027-02-28");
  });

  it("is seven days for a week", () => {
    expect(weeksOf("week", "2026-11-04")).toEqual([
      [
        "2026-11-02",
        "2026-11-03",
        "2026-11-04",
        "2026-11-05",
        "2026-11-06",
        "2026-11-07",
        "2026-11-08",
      ],
    ]);
  });

  it("lists a month's own days for the agenda", () => {
    const days = monthDays("2026-02-10");
    expect(days).toHaveLength(28);
    expect(days[0]).toBe("2026-02-01");
    expect(days.at(-1)).toBe("2026-02-28");
  });

  it("steps by its own length", () => {
    expect(stepped("month", "2026-11-04", 1)).toBe("2026-12-04");
    expect(stepped("week", "2026-11-04", -1)).toBe("2026-10-28");
  });
});

describe("reading a range", () => {
  it("compares an event date in milliseconds and a field in text", () => {
    expect(rangeConditions(EVENT, "2026-11-01", "2026-12-01")).toEqual([
      { field: EVENT, op: "ge", value: at("2026-11-01T00:00:00Z") },
      { field: EVENT, op: "lt", value: at("2026-12-01T00:00:00Z") },
    ]);
    expect(rangeConditions("due", "2026-11-01", "2026-12-01")).toEqual([
      { field: "due", op: "ge", value: "2026-11-01" },
      { field: "due", op: "lt", value: "2026-12-01" },
    ]);
  });

  it("counts by the day of each bucket and skips one with no date", () => {
    const counts = countsByDay({
      rows: [
        { keys: ["2026-11-02"], values: [3] },
        { keys: [null], values: [9] },
      ],
      truncated: false,
    });
    expect([...counts]).toEqual([["2026-11-02", 3]]);
    expect(countsByDay(undefined).size).toBe(0);
  });
});

describe("the day an object is on", () => {
  it("comes from the event date, in UTC", () => {
    const row = summary({ event_start: at("2026-11-04T23:30:00Z") });
    expect(dayOf(row, EVENT)).toBe("2026-11-04");
    expect(dayOf(summary({}), EVENT)).toBeNull();
  });

  it("comes from a field, and a value that is no date is on no day", () => {
    expect(dayOf(summary({ props_json: { due: "2026-11-04" } }), "due")).toBe("2026-11-04");
    expect(dayOf(summary({ props_json: { due: "2026-11-04T10:00:00Z" } }), "due")).toBe(
      "2026-11-04",
    );
    expect(dayOf(summary({ props_json: { due: "soon" } }), "due")).toBeNull();
    expect(dayOf(summary({ props_json: { due: 3 } }), "due")).toBeNull();
    expect(dayOf(summary({}), "due")).toBeNull();
  });
});

describe("moving an object", () => {
  it("keeps an event's time of day and its length", () => {
    const row = summary({
      event_start: at("2026-11-04T09:30:00Z"),
      event_end: at("2026-11-06T11:00:00Z"),
    });
    expect(dateWrite(row, EVENT, "2026-11-10")).toEqual({
      kind: "event",
      start: at("2026-11-10T09:30:00Z"),
      end: at("2026-11-12T11:00:00Z"),
    });
  });

  it("starts an event with no date at the start of the day", () => {
    expect(dateWrite(summary({}), EVENT, "2026-11-10")).toEqual({
      kind: "event",
      start: at("2026-11-10T00:00:00Z"),
      end: null,
    });
  });

  it("writes a field's date and keeps its time", () => {
    expect(dateWrite(summary({ props_json: { due: "2026-11-04" } }), "due", "2026-11-10")).toEqual({
      kind: "prop",
      key: "due",
      value: "2026-11-10",
    });
    expect(
      dateWrite(summary({ props_json: { due: "2026-11-04T10:00:00Z" } }), "due", "2026-11-10"),
    ).toEqual({ kind: "prop", key: "due", value: "2026-11-10T10:00:00Z" });
    expect(dateWrite(summary({}), "due", "2026-11-10")).toEqual({
      kind: "prop",
      key: "due",
      value: "2026-11-10",
    });
  });
});

describe("the keys in the grid", () => {
  it("move by a day, a week, along the week and by the view's length", () => {
    expect(dayKey("ArrowLeft", "2026-11-04", "month")).toBe("2026-11-03");
    expect(dayKey("ArrowRight", "2026-11-04", "month")).toBe("2026-11-05");
    expect(dayKey("ArrowUp", "2026-11-04", "month")).toBe("2026-10-28");
    expect(dayKey("ArrowDown", "2026-11-04", "month")).toBe("2026-11-11");
    expect(dayKey("Home", "2026-11-04", "month")).toBe("2026-11-02");
    expect(dayKey("End", "2026-11-04", "month")).toBe("2026-11-08");
    expect(dayKey("PageDown", "2026-11-04", "month")).toBe("2026-12-04");
    expect(dayKey("PageUp", "2026-11-04", "week")).toBe("2026-10-28");
    expect(dayKey("a", "2026-11-04", "month")).toBeNull();
  });

  it("carry an item by a day or a week", () => {
    expect(carryKey("ArrowLeft")).toBe(-1);
    expect(carryKey("ArrowRight")).toBe(1);
    expect(carryKey("ArrowUp")).toBe(-7);
    expect(carryKey("ArrowDown")).toBe(7);
    expect(carryKey("Enter")).toBeNull();
  });
});

describe("what a calendar goes by", () => {
  const field = (key: string, kind: string) =>
    ({ key, label: key, kind, type_id: "t" }) as unknown as FieldRow;

  it("offers the event date and each date field once", () => {
    const choices = dateChoices([
      field("due", "date"),
      field("status", "select"),
      field("at", "datetime"),
      field("due", "date"),
    ]);
    expect(choices.map((choice) => choice.key)).toEqual([EVENT, "due", "at"]);
  });

  it("starts from the first field, or the event date when there is none", () => {
    const both = dateChoices([field("due", "date")]);
    expect(chosenDate(both, null).key).toBe("due");
    expect(chosenDate(both, EVENT).key).toBe(EVENT);
    expect(chosenDate(both, "gone").key).toBe("due");
    expect(chosenDate(dateChoices([]), null).key).toBe(EVENT);
  });
});
