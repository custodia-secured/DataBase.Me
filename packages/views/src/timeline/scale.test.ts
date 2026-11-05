// SPDX-License-Identifier: AGPL-3.0-or-later
import { describe, expect, it } from "vitest";
import {
  axisLabel,
  bucketAt,
  bucketCount,
  bucketName,
  domainOf,
  indexOfDay,
  laneKey,
  layOut,
  LEVELS,
  levelOf,
  offsetOf,
  readWindow,
  zoomed,
  type LevelId,
} from "./scale";

const domain = { firstYear: 2024, lastYear: 2026 };
const level = (id: LevelId) => levelOf(id);

describe("zooming", () => {
  it("goes from days to decades and stops at either end", () => {
    expect(LEVELS.map((one) => one.id)).toEqual(["day", "week", "month", "year", "decade"]);
    expect(zoomed("month", -1).id).toBe("week");
    expect(zoomed("month", 1).id).toBe("year");
    expect(zoomed("day", -1).id).toBe("day");
    expect(zoomed("decade", 1).id).toBe("decade");
  });

  it("draws objects close up and counts further out", () => {
    expect(LEVELS.filter((one) => one.items).map((one) => one.id)).toEqual(["day", "week"]);
  });

  it("falls back to months for a level it does not know", () => {
    expect(levelOf("century").id).toBe("month");
  });
});

describe("the lane's span", () => {
  const years = (...keys: (string | null)[]) => ({
    rows: keys.map((key) => ({ keys: [key], values: [1] })),
    truncated: false,
  });

  it("runs a year past the first and the last", () => {
    expect(domainOf(years("2019", "2021", "2030"), "2026-11-04")).toEqual({
      firstYear: 2018,
      lastYear: 2031,
    });
  });

  it("always takes in this year", () => {
    expect(domainOf(years("2001"), "2026-11-04")).toEqual({ firstYear: 2000, lastYear: 2027 });
    expect(domainOf(years("2031"), "2026-11-04")).toEqual({ firstYear: 2025, lastYear: 2032 });
    expect(domainOf(undefined, "2026-11-04")).toEqual({ firstYear: 2025, lastYear: 2027 });
  });

  it("does not go before the year 0", () => {
    expect(domainOf(years("0000"), "2026-11-04").firstYear).toBe(0);
  });

  it("skips what is not a year", () => {
    expect(domainOf(years(null, "soon"), "2026-11-04")).toEqual({
      firstYear: 2025,
      lastYear: 2027,
    });
  });
});

describe("buckets", () => {
  it("are counted for each level", () => {
    // 2024 is a leap year.
    expect(bucketCount(level("day"), domain)).toBe(366 + 365 + 365);
    // 2024 starts on a Monday, so the weeks start with it; the last one runs into 2027.
    expect(bucketCount(level("week"), domain)).toBe(157);
    expect(bucketCount(level("month"), domain)).toBe(36);
    expect(bucketCount(level("year"), domain)).toBe(3);
    expect(bucketCount(level("decade"), domain)).toBe(3);
  });

  it("carry the key the store groups by", () => {
    expect(bucketAt(level("day"), domain, 0)).toEqual({
      key: "2024-01-01",
      start: "2024-01-01",
      end: "2024-01-02",
    });
    expect(bucketAt(level("week"), domain, 1)).toEqual({
      key: "2024-01-08",
      start: "2024-01-08",
      end: "2024-01-15",
    });
    expect(bucketAt(level("month"), domain, 11)).toEqual({
      key: "2024-12",
      start: "2024-12-01",
      end: "2025-01-01",
    });
    expect(bucketAt(level("year"), domain, 2)).toEqual({
      key: "2026",
      start: "2026-01-01",
      end: "2027-01-01",
    });
  });

  it("start a week on the Monday before the lane when the year starts midweek", () => {
    const later = { firstYear: 2026, lastYear: 2026 };
    expect(bucketAt(level("week"), later, 0).start).toBe("2025-12-29");
    expect(indexOfDay(level("week"), later, "2026-01-01")).toBe(0);
    expect(indexOfDay(level("week"), later, "2026-01-05")).toBe(1);
  });

  it("are found again from any day in them", () => {
    for (const id of ["day", "week", "month", "year", "decade"] as const) {
      const one = level(id);
      const index = indexOfDay(one, domain, "2025-07-16");
      const bucket = bucketAt(one, domain, index);
      expect(bucket.start <= "2025-07-16").toBe(true);
      expect(bucket.end > "2025-07-16").toBe(true);
    }
  });

  it("are named for the axis and in words", () => {
    const november = bucketAt(level("month"), domain, 34);
    expect(axisLabel(level("month"), november)).toBe("Nov 2026");
    expect(bucketName(level("month"), november)).toBe("November 2026");
    expect(bucketName(level("week"), bucketAt(level("week"), domain, 0))).toBe(
      "The week of Monday, January 1, 2024",
    );
  });

  it("name a decade only as it starts", () => {
    const decades = { firstYear: 2019, lastYear: 2021 };
    const labels = [0, 1, 2].map((index) =>
      axisLabel(level("decade"), bucketAt(level("decade"), decades, index)),
    );
    expect(labels).toEqual(["", "2020", ""]);
  });
});

describe("a moment along the lane", () => {
  it("is measured from the first bucket", () => {
    const start = Date.parse("2024-01-03T00:00:00Z");
    expect(offsetOf(level("day"), domain, start, 96)).toBe(192);
    expect(offsetOf(level("week"), domain, start, 120)).toBeCloseTo((2 / 7) * 120);
  });

  it("falls within a day by its time", () => {
    const noon = Date.parse("2024-01-01T12:00:00Z");
    expect(offsetOf(level("day"), domain, noon, 96)).toBe(48);
  });
});

describe("what is read", () => {
  it("moves in chunks, so a small scroll asks for nothing new", () => {
    expect(readWindow(3, 9, 8, 100)).toEqual({ from: 0, to: 16 });
    expect(readWindow(5, 11, 8, 100)).toEqual({ from: 0, to: 16 });
    expect(readWindow(9, 15, 8, 100)).toEqual({ from: 8, to: 16 });
    expect(readWindow(9, 17, 8, 100)).toEqual({ from: 8, to: 24 });
  });

  it("stops at the end of the lane", () => {
    expect(readWindow(95, 99, 8, 100)).toEqual({ from: 88, to: 100 });
  });
});

describe("laying out bars", () => {
  const span = (id: string, start: number, length: number) => ({ id, start, length });

  it("shares a lane when bars do not touch", () => {
    const { placed, hidden } = layOut([span("a", 0, 10), span("b", 10, 10), span("c", 30, 5)], 3);
    expect(placed.map((one) => [one.span.id, one.lane])).toEqual([
      ["a", 0],
      ["b", 0],
      ["c", 0],
    ]);
    expect(hidden).toEqual([]);
  });

  it("opens a lane when they overlap, and goes back to a free one", () => {
    const { placed } = layOut(
      [span("a", 0, 100), span("b", 10, 10), span("c", 15, 10), span("d", 30, 10)],
      3,
    );
    expect(placed.map((one) => [one.span.id, one.lane])).toEqual([
      ["a", 0],
      ["b", 1],
      ["c", 2],
      ["d", 1],
    ]);
  });

  it("orders by time whatever order they arrive in", () => {
    const { placed } = layOut([span("late", 50, 5), span("early", 0, 5)], 1);
    expect(placed.map((one) => one.span.id)).toEqual(["early", "late"]);
  });

  it("leaves out and counts what finds no lane", () => {
    const { placed, hidden } = layOut([span("a", 0, 10), span("b", 0, 10), span("c", 0, 10)], 2);
    expect(placed).toHaveLength(2);
    expect(hidden.map((one) => one.id)).toEqual(["c"]);
  });
});

describe("the keys in the lane", () => {
  it("step, page, reach the ends, zoom and act", () => {
    expect(laneKey("ArrowRight")).toEqual({ kind: "step", by: 1 });
    expect(laneKey("ArrowDown")).toEqual({ kind: "step", by: 1 });
    expect(laneKey("ArrowLeft")).toEqual({ kind: "step", by: -1 });
    expect(laneKey("ArrowUp")).toEqual({ kind: "step", by: -1 });
    expect(laneKey("PageDown")).toEqual({ kind: "page", by: 1 });
    expect(laneKey("Home")).toEqual({ kind: "edge", to: "first" });
    expect(laneKey("End")).toEqual({ kind: "edge", to: "last" });
    expect(laneKey("+")).toEqual({ kind: "zoom", by: -1 });
    expect(laneKey("=")).toEqual({ kind: "zoom", by: -1 });
    expect(laneKey("-")).toEqual({ kind: "zoom", by: 1 });
    expect(laneKey("Enter")).toEqual({ kind: "act" });
    expect(laneKey("a")).toBeNull();
  });
});
