// SPDX-License-Identifier: AGPL-3.0-or-later
import type { DataProfile_Serialize, FieldKind, FieldRow } from "@databaseme/client";
import { describe, expect, test } from "vitest";
import { profileKeys, shapeOf } from "./suggested";

const field = (key: string, kind: FieldKind): FieldRow =>
  ({ id: key, type_id: "t", key, label: key, kind, options_json: null }) as unknown as FieldRow;

const profile = (change: Partial<DataProfile_Serialize>): DataProfile_Serialize => ({
  objects: 10,
  sampled: 10,
  with_event: 0,
  with_span: 0,
  event_first: null,
  event_last: null,
  with_geo: 0,
  with_children: 0,
  with_relations: 0,
  fields: [],
  ...change,
});

const held = (
  key: string,
  change: Partial<DataProfile_Serialize["fields"][number]> = {},
): DataProfile_Serialize["fields"][number] => ({
  key,
  non_empty: 10,
  distinct: 3,
  numeric: 0,
  lists: 0,
  min: null,
  max: null,
  ...change,
});

describe("shapeOf", () => {
  test("counts become shares of what the profile looked at, not of everything", () => {
    const shape = shapeOf(
      profile({
        objects: 9000,
        sampled: 5000,
        with_event: 2500,
        with_span: 500,
        with_geo: 5000,
        with_children: 1000,
        with_relations: 250,
      }),
      [],
    );
    expect(shape).toMatchObject({
      objects: 9000,
      withEvent: 0.5,
      withSpan: 0.1,
      withGeo: 1,
      withChildren: 0.2,
      withRelations: 0.05,
    });
  });

  test("nothing under a page is no shares at all, and not a division by zero", () => {
    const shape = shapeOf(profile({ objects: 0, sampled: 0, fields: [held("a")] }), [
      field("a", "text"),
    ]);
    expect(shape.withEvent).toBe(0);
    expect(shape.fields[0]?.filled).toBe(0);
  });

  test("the range of events is in days", () => {
    const day = 86_400_000;
    const shape = shapeOf(profile({ event_first: 5 * day, event_last: 25 * day }), []);
    expect(shape.eventRangeDays).toBe(20);
    expect(shapeOf(profile({}), []).eventRangeDays).toBe(0);
  });

  test.each<[FieldKind, string]>([
    ["text", "text"],
    ["rich_text", "text"],
    ["number", "number"],
    ["currency", "number"],
    ["rating", "number"],
    ["duration", "number"],
    ["boolean", "boolean"],
    ["date", "date"],
    ["datetime", "date"],
    ["select", "select"],
    ["multi_select", "multi_select"],
    ["file", "file"],
    ["url", "url"],
    ["email", "contact"],
    ["phone", "contact"],
    ["relation", "other"],
    ["geo", "other"],
    ["json", "other"],
  ])("a %s field reads as %s", (kind, expected) => {
    const shape = shapeOf(profile({ fields: [held("a")] }), [field("a", kind)]);
    expect(shape.fields[0]?.kind).toBe(expected);
  });

  test("a formula is whatever it works out to", () => {
    const of = (numeric: number) =>
      shapeOf(profile({ fields: [held("a", { numeric })] }), [field("a", "formula")]).fields[0]
        ?.kind;
    expect(of(10)).toBe("number");
    expect(of(5)).toBe("number");
    expect(of(4)).toBe("other");
    expect(
      shapeOf(profile({ fields: [held("a", { non_empty: 0 })] }), [field("a", "rollup")]).fields[0]
        ?.kind,
    ).toBe("other");
  });

  test("a date's range comes from its values, and a value that is no date gives none", () => {
    const of = (min: unknown, max: unknown) =>
      shapeOf(profile({ fields: [held("a", { min, max } as never)] }), [field("a", "date")])
        .fields[0]?.rangeDays;
    expect(of("2026-01-01", "2026-01-31")).toBe(30);
    expect(of(86_400_000, 3 * 86_400_000)).toBe(2);
    expect(of("soon", "later")).toBeUndefined();
    expect(of(null, null)).toBeUndefined();
  });

  test("only a date has a range", () => {
    const shape = shapeOf(profile({ fields: [held("a", { min: 1, max: 9 })] }), [
      field("a", "number"),
    ]);
    expect(shape.fields[0]).not.toHaveProperty("rangeDays");
  });

  test("a field the profile was told about and nothing else knows is left out", () => {
    expect(shapeOf(profile({ fields: [held("ghost")] }), []).fields).toEqual([]);
  });
});

describe("profileKeys", () => {
  test("each key once, however many types share it", () => {
    expect(profileKeys([field("a", "text"), field("b", "date"), field("a", "number")])).toEqual([
      "a",
      "b",
    ]);
  });

  test("no more than the store takes", () => {
    const many = Array.from({ length: 80 }, (_, at) => field(`f${String(at)}`, "text"));
    expect(profileKeys(many)).toHaveLength(64);
  });
});
