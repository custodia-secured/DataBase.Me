// SPDX-License-Identifier: AGPL-3.0-or-later
import type { FieldKind, FieldRow } from "@databaseme/client";
import { describe, expect, it } from "vitest";
import {
  comparisonsFor,
  conditionToRow,
  fieldNamed,
  filterFields,
  newRow,
  rowToCondition,
  takesValue,
  type FilterRow,
} from "./filters";

function row(key: string, kind: FieldKind, options_json: unknown = {}): FieldRow {
  return { key, label: key, kind, options_json } as unknown as FieldRow;
}

const fields = filterFields([
  row("status", "select", {
    options: [
      { id: "todo", label: "To do" },
      { id: "done", label: "Done" },
    ],
  }),
  row("tags", "multi_select", { options: [{ id: "red", label: "Red" }] }),
  row("amount", "number"),
  row("due", "date"),
  row("shipped", "boolean"),
  row("link", "url"),
  row("owner", "relation"),
  row("empty", "select", { options: [] }),
]);

function apply(field: string, op: FilterRow["op"], text: string) {
  return rowToCondition({ id: 1, field, op, text }, fieldNamed(fields, field));
}

describe("the fields a listing is filtered by", () => {
  it("are the title, the event date and its own, once each", () => {
    const keys = filterFields([row("a", "text"), row("a", "text"), row("b", "number")]).map(
      (field) => field.key,
    );
    expect(keys).toEqual(["$title", "$event_start", "a", "b"]);
  });

  it("keep a name that is no longer on the listing", () => {
    expect(fieldNamed(fields, "gone")).toMatchObject({ key: "gone", label: "gone", kind: "text" });
  });
});

describe("comparisons", () => {
  it("fit the kind", () => {
    const ops = (kind: Parameters<typeof comparisonsFor>[0]) =>
      comparisonsFor(kind).map((comparison) => comparison.op);
    expect(ops("text")).toContain("contains");
    expect(ops("number")).toContain("ge");
    expect(ops("number")).not.toContain("contains");
    expect(ops("date")).toContain("lt");
    expect(ops("event")).not.toContain("eq");
    expect(ops("select")).toEqual(["eq", "ne", "is_empty", "is_not_empty"]);
    expect(ops("multi_select")).toContain("has");
    expect(ops("boolean")).toEqual(["eq"]);
    expect(ops("relation")).toEqual(["is_empty", "is_not_empty"]);
  });

  it("say which take a value", () => {
    expect(takesValue("eq")).toBe(true);
    expect(takesValue("is_empty")).toBe(false);
  });
});

describe("a new row", () => {
  it("starts on the first comparison and, for a choice, the first choice", () => {
    expect(newRow(1, fieldNamed(fields, "status"))).toEqual({
      id: 1,
      field: "status",
      op: "eq",
      text: "todo",
    });
    expect(newRow(2, fieldNamed(fields, "shipped")).text).toBe("true");
    expect(newRow(3, fieldNamed(fields, "$title"))).toMatchObject({ op: "contains", text: "" });
  });
});

describe("a row as a condition", () => {
  it("reads numbers as numbers and refuses what is not one", () => {
    expect(apply("amount", "gt", " 2.5 ")).toEqual({
      condition: { field: "amount", op: "gt", value: 2.5 },
    });
    expect(apply("amount", "gt", "lots")).toEqual({ error: "Enter a number" });
    expect(apply("amount", "gt", "")).toEqual({ error: "Enter a number" });
  });

  it("keeps a date field's date as text and refuses one that does not exist", () => {
    expect(apply("due", "lt", "2026-11-04")).toEqual({
      condition: { field: "due", op: "lt", value: "2026-11-04" },
    });
    expect(apply("due", "lt", "2026-02-30")).toEqual({ error: "Enter a date as YYYY-MM-DD" });
    expect(apply("due", "lt", "soon")).toEqual({ error: "Enter a date as YYYY-MM-DD" });
  });

  it("turns an event date into the column's milliseconds", () => {
    expect(apply("$event_start", "ge", "2026-01-01")).toEqual({
      condition: { field: "$event_start", op: "ge", value: Date.UTC(2026, 0, 1) },
    });
  });

  it("reads yes or no as a boolean", () => {
    expect(apply("shipped", "eq", "true")).toEqual({
      condition: { field: "shipped", op: "eq", value: true },
    });
    expect(apply("shipped", "eq", "false")).toEqual({
      condition: { field: "shipped", op: "eq", value: false },
    });
  });

  it("needs a choice and text where they are the value", () => {
    expect(apply("empty", "eq", "")).toEqual({ error: "This field has no choices yet" });
    expect(apply("$title", "contains", "  ")).toEqual({ error: "Enter some text" });
    expect(apply("tags", "has", "red")).toEqual({
      condition: { field: "tags", op: "has", value: "red" },
    });
  });

  it("gives no value to a comparison that takes none", () => {
    expect(apply("owner", "is_empty", "whatever")).toEqual({
      condition: { field: "owner", op: "is_empty", value: null },
    });
  });
});

describe("a condition as a row", () => {
  it("comes back as it was applied", () => {
    const cases: [string, FilterRow["op"], string][] = [
      ["amount", "ge", "3"],
      ["due", "lt", "2026-11-04"],
      ["$event_start", "ge", "2026-01-01"],
      ["shipped", "eq", "false"],
      ["status", "ne", "done"],
      ["owner", "is_not_empty", ""],
    ];
    for (const [field, op, text] of cases) {
      const applied = apply(field, op, text);
      if (!("condition" in applied)) throw new Error(`${field}: ${applied.error}`);
      expect(conditionToRow(9, applied.condition, fields)).toEqual({ id: 9, field, op, text });
    }
  });
});
