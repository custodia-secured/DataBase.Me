// SPDX-License-Identifier: AGPL-3.0-or-later
import type { Id, ObjectSummary_Serialize } from "@databaseme/client";
import { describe, expect, it } from "vitest";
import { dropZone, expandedBelow, flattenTree, isWithin } from "./tree";

function summary(id: Id, parent: Id | null, childCount = 0): ObjectSummary_Serialize {
  return {
    object: {
      id,
      title: id,
      summary: null,
      parent_id: parent,
      sort_key: "a0",
      props_json: {},
      created_at: 0,
      updated_at: 0,
      captured_at: null,
      event_start: null,
      event_end: null,
      geo_lat: null,
      geo_lon: null,
      deleted_at: null,
      sensitivity: "normal",
    },
    type_ids: [],
    child_count: childCount,
    cover: null,
  };
}

/** Children lists by parent; the top level is under "". */
function lists(byParent: Record<string, ObjectSummary_Serialize[]>) {
  return (parent: Id | null) => byParent[parent ?? ""];
}

describe("flattenTree", () => {
  const data = lists({
    "": [summary("home", null, 2), summary("inbox", null, 1)],
    home: [summary("alpha", "home"), summary("beta", "home", 1)],
    beta: [summary("notes", "beta")],
    inbox: [summary("idea", "inbox")],
  });

  it("lists the top level and the children of expanded rows, with levels and positions", () => {
    const rows = flattenTree(data, new Set(["home", "beta"]));
    expect(rows.map((row) => [row.id, row.level, row.position, row.siblings])).toEqual([
      ["home", 1, 1, 2],
      ["alpha", 2, 1, 2],
      ["beta", 2, 2, 2],
      ["notes", 3, 1, 1],
      ["inbox", 1, 2, 2],
    ]);
    expect(rows.find((row) => row.id === "inbox")?.expanded).toBe(false);
    expect(rows.find((row) => row.id === "beta")?.parent).toBe("home");
  });

  it("skips children that are not loaded yet", () => {
    const rows = flattenTree(lists({ "": [summary("home", null, 2)] }), new Set(["home"]));
    expect(rows.map((row) => row.id)).toEqual(["home"]);
    expect(rows[0]?.expanded).toBe(true);
  });

  it("lists an object once while a move is half read", () => {
    // "beta" has moved to "inbox"; inbox's children are read again, home's not yet.
    const stale = lists({
      "": [summary("home", null, 2), summary("inbox", null, 1)],
      home: [summary("alpha", "home"), summary("beta", "home")],
      inbox: [summary("idea", "inbox"), summary("beta", "inbox")],
    });
    const ids = flattenTree(stale, new Set(["home", "inbox"])).map((row) => row.id);
    expect(ids).toEqual(["home", "alpha", "beta", "inbox", "idea"]);
  });

  it("ends when two objects each seem to hold the other", () => {
    const cycle = lists({
      "": [summary("a", null, 1)],
      a: [summary("b", "a", 1)],
      b: [summary("a", "b", 1)],
    });
    const ids = flattenTree(cycle, new Set(["a", "b"])).map((row) => row.id);
    expect(ids).toEqual(["a", "b"]);
  });
});

describe("isWithin and expandedBelow", () => {
  const rows = flattenTree(
    lists({
      "": [summary("home", null, 1)],
      home: [summary("beta", "home", 1)],
      beta: [summary("notes", "beta")],
    }),
    new Set(["home", "beta"]),
  );

  it("knows an object is within itself and its ancestors only", () => {
    expect(isWithin(rows, "notes", "home")).toBe(true);
    expect(isWithin(rows, "home", "home")).toBe(true);
    expect(isWithin(rows, "home", "notes")).toBe(false);
  });

  it("finds the expanded rows below a row", () => {
    expect(expandedBelow(rows, "home")).toEqual(["beta"]);
    expect(expandedBelow(rows, "beta")).toEqual([]);
  });
});

describe("dropZone", () => {
  it("puts the top quarter before, the bottom quarter after, the middle inside", () => {
    expect(dropZone(102, 100, 40)).toBe("before");
    expect(dropZone(120, 100, 40)).toBe("inside");
    expect(dropZone(138, 100, 40)).toBe("after");
  });
});
