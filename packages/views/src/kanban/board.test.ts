// SPDX-License-Identifier: AGPL-3.0-or-later
import { describe, expect, it } from "vitest";
import { boardKey, columnListing, columnsOf } from "./board";

const field = {
  options_json: {
    options: [{ id: "todo", label: "To do" }, { id: "done" }],
  },
};

describe("a board's columns", () => {
  it("are the field's choices in order, then no value", () => {
    expect(columnsOf(field)).toEqual([
      { id: "option:todo", value: "todo", label: "To do" },
      { id: "option:done", value: "done", label: "done" },
      { id: "none", value: null, label: "No value" },
    ]);
  });

  it("still have a place for every card when the field offers no choices", () => {
    expect(columnsOf({ options_json: {} })).toEqual([
      { id: "none", value: null, label: "No value" },
    ]);
  });

  it("narrow what the board lists, and keep its filter", () => {
    const listing = {
      source: { kind: "type" as const, type_id: "t" },
      conditions: [{ field: "n", op: "gt" as const, value: 1 }],
    };
    const [todo, , none] = columnsOf(field);
    if (!todo || !none) throw new Error("columns");
    expect(columnListing(listing, "status", todo).conditions).toEqual([
      { field: "n", op: "gt", value: 1 },
      { field: "status", op: "eq", value: "todo" },
    ]);
    expect(columnListing(listing, "status", none).conditions).toEqual([
      { field: "n", op: "gt", value: 1 },
      { field: "status", op: "is_empty", value: null },
    ]);
    expect(columnListing(listing, "status", todo).source).toBe(listing.source);
  });
});

describe("the keys on a board", () => {
  const at = (key: string, alt = false, row = 2, total = 5) => boardKey(key, alt, row, total, 3);

  it("move the selection along a column and stop at its ends", () => {
    expect(at("ArrowUp")).toEqual({ kind: "row", row: 1 });
    expect(at("ArrowDown")).toEqual({ kind: "row", row: 3 });
    expect(at("ArrowUp", false, 0)).toEqual({ kind: "row", row: 0 });
    expect(at("ArrowDown", false, 4)).toEqual({ kind: "row", row: 4 });
    expect(at("Home")).toEqual({ kind: "row", row: 0 });
    expect(at("End")).toEqual({ kind: "row", row: 4 });
    expect(at("PageDown")).toEqual({ kind: "row", row: 4 });
    expect(at("PageUp")).toEqual({ kind: "row", row: 0 });
  });

  it("stay at the top of an empty column", () => {
    expect(at("ArrowDown", false, 0, 0)).toEqual({ kind: "row", row: 0 });
    expect(at("End", false, 0, 0)).toEqual({ kind: "row", row: 0 });
  });

  it("move between columns, and carry the card with Alt", () => {
    expect(at("ArrowRight")).toEqual({ kind: "column", step: 1 });
    expect(at("ArrowLeft")).toEqual({ kind: "column", step: -1 });
    expect(at("ArrowRight", true)).toEqual({ kind: "card-to-column", step: 1 });
    expect(at("ArrowLeft", true)).toEqual({ kind: "card-to-column", step: -1 });
    expect(at("ArrowUp", true)).toEqual({ kind: "card-in-column", step: -1 });
    expect(at("ArrowDown", true)).toEqual({ kind: "card-in-column", step: 1 });
  });

  it("open the card with Enter and leave other keys alone", () => {
    expect(at("Enter")).toEqual({ kind: "open" });
    expect(at("a")).toBeNull();
    expect(at("Tab")).toBeNull();
  });
});
