// SPDX-License-Identifier: AGPL-3.0-or-later
import type { GraphData } from "@databaseme/client";
import { describe, expect, it } from "vitest";
import { listingOf } from "../framework/listing";
import {
  colourSlot,
  graphQuery,
  graphSummary,
  indexLinks,
  layoutJob,
  linksOf,
  namesOf,
  passesFor,
  search,
  spreadOf,
  startOf,
  waitAfter,
} from "./graph";

function graph(parts: Partial<GraphData>): GraphData {
  return {
    ids: [],
    titles: [],
    types: [],
    node_types: [],
    edges: [],
    rel_types: [],
    edge_types: [],
    parent_edges: [],
    truncated: false,
    ...parts,
  };
}

describe("graphQuery", () => {
  const conditions = [{ field: "status", op: "eq" as const, value: "todo" }];

  it("reaches all the way below the page, and keeps its filter", () => {
    const query = graphQuery({ ...listingOf("page"), conditions }, "here");
    expect(query.from).toEqual({ under: "page" });
    expect(query.where).toEqual(conditions);
    // The store bounds a query's own limit far lower, and takes the graph's beside it.
    expect(query.limit).toBeUndefined();
  });

  it("asks for everything when the scope is the vault, and keeps the filter there too", () => {
    const query = graphQuery({ ...listingOf("page"), conditions }, "vault");
    expect(query.from).toEqual({});
    expect(query.where).toEqual(conditions);
  });

  it("has no parent to be under at the top of the vault", () => {
    expect(
      graphQuery({ source: { kind: "children", parent: null }, conditions: [] }, "here").from,
    ).toEqual({});
  });

  it("goes by the type when the source is one, whatever the scope", () => {
    const source = { kind: "type" as const, type_id: "t" };
    expect(graphQuery({ source, conditions: [] }, "here").from).toEqual({ types: ["t"] });
    expect(graphQuery({ source, conditions: [] }, "vault").from).toEqual({ types: ["t"] });
  });
});

describe("startOf", () => {
  const ids = Array.from({ length: 2000 }, (_, at) => `01KA${String(at).padStart(22, "0")}`);

  it("gives an ID the same place every time, in the disc", () => {
    for (const id of ids) {
      const place = startOf(id);
      expect(startOf(id)).toEqual(place);
      expect(Math.hypot(place.x, place.y)).toBeLessThanOrEqual(1);
    }
  });

  it("spreads IDs that differ by one character over the disc", () => {
    const places = ids.map(startOf);
    // Every quadrant gets its share, and the outer half of the area gets half.
    for (const [sx, sy] of [
      [1, 1],
      [1, -1],
      [-1, 1],
      [-1, -1],
    ] as const) {
      const share = places.filter(({ x, y }) => x * sx > 0 && y * sy > 0).length / places.length;
      expect(share).toBeGreaterThan(0.2);
      expect(share).toBeLessThan(0.3);
    }
    const outer = places.filter(({ x, y }) => x * x + y * y > 0.5).length / places.length;
    expect(outer).toBeGreaterThan(0.45);
    expect(outer).toBeLessThan(0.55);
  });

  it("does not depend on the other nodes", () => {
    expect(startOf("a")).toEqual(startOf("a"));
    expect(startOf("a")).not.toEqual(startOf("b"));
  });

  it("starts wider the more nodes there are, and never collapses", () => {
    expect(spreadOf(0)).toBe(1);
    expect(spreadOf(10_000)).toBe(100);
  });
});

describe("indexLinks", () => {
  const data = graph({
    ids: ["a", "b", "c", "d"],
    titles: ["A", "B", "C", "D"],
    edges: [0, 1, 2, 0, 3, 3],
    edge_types: [0, 0, 0],
    rel_types: ["relates"],
    parent_edges: [0, 2, 0, 3],
  });
  const index = indexLinks(data);

  it("finds a relation from either end", () => {
    expect(linksOf(index, 0).related).toEqual([1, 2]);
    expect(linksOf(index, 1).related).toEqual([0]);
    expect(linksOf(index, 2).related).toEqual([0]);
  });

  it("counts a relation to itself once", () => {
    expect(linksOf(index, 3).related).toEqual([3]);
  });

  it("tells parents from children", () => {
    expect(linksOf(index, 0).children).toEqual([2, 3]);
    expect(linksOf(index, 0).parent).toBeNull();
    expect(linksOf(index, 2).parent).toBe(0);
    expect(linksOf(index, 2).children).toEqual([]);
  });

  it("holds up on no nodes at all", () => {
    expect(indexLinks(graph({})).parents).toHaveLength(0);
  });
});

describe("what the list says", () => {
  const titles = ["Alpha", "", "Gamma", "Delta", "Echo"];

  it("names the first few and counts the rest", () => {
    expect(namesOf(titles, [])).toBe("");
    expect(namesOf(titles, [0])).toBe("Alpha");
    expect(namesOf(titles, [0, 1, 2])).toBe("Alpha, Untitled, Gamma");
    expect(namesOf(titles, [0, 2, 3, 4])).toBe("Alpha, Gamma, Delta and 1 more");
  });

  it("searches titles whatever their case, and tells nothing from no matches", () => {
    expect(search(titles, "  ")).toBeNull();
    expect(search(titles, "A")).toEqual([0, 2, 3]);
    expect(search(titles, "zzz")).toEqual([]);
  });

  it("says what the graph holds, in words", () => {
    expect(graphSummary(graph({ ids: ["a"] }))).toBe("1 item and no links.");
    expect(graphSummary(graph({ ids: ["a", "b", "c"], edges: [0, 1], parent_edges: [0, 2] }))).toBe(
      "3 items and 2 links.",
    );
    expect(graphSummary(graph({ ids: Array.from({ length: 1234 }, String), edges: [0, 1] }))).toBe(
      "1,234 items and 1 link.",
    );
  });
});

describe("the layout", () => {
  it("takes the positions it is given, and every link but a node's to itself", () => {
    const data = graph({
      ids: ["a", "b"],
      edges: [0, 1, 1, 1],
      parent_edges: [1, 0],
    });
    const job = layoutJob(data, (id) => (id === "a" ? { x: 1, y: 2 } : { x: 3, y: 4 }));
    expect([...job.positions]).toEqual([1, 2, 3, 4]);
    expect([...job.edges]).toEqual([0, 1, 1, 0]);
  });

  it("makes fewer passes the bigger the graph, within bounds", () => {
    expect(passesFor(10)).toBe(1000);
    expect(passesFor(2000)).toBe(1000);
    expect(passesFor(10_000)).toBe(200);
    expect(passesFor(100_000)).toBe(100);
    expect(passesFor(0)).toBe(1000);
  });

  it("leaves the page nine tenths of its time, and never spins", () => {
    expect(waitAfter(1000)).toBe(9000);
    expect(waitAfter(0)).toBe(16);
  });
});

describe("colourSlot", () => {
  it("gives the first eight types a colour, and the rest and none share one", () => {
    expect(colourSlot(0)).toBe(0);
    expect(colourSlot(7)).toBe(7);
    expect(colourSlot(8)).toBe(-1);
    expect(colourSlot(-1)).toBe(-1);
  });
});
