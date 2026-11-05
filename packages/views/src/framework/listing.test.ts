// SPDX-License-Identifier: AGPL-3.0-or-later
import { MANUAL, type Query_Serialize, type Sort } from "@databaseme/client";
import { describe, expect, it } from "vitest";
import { listingFromQuery, listingOf, listingQuery, type Listing } from "./listing";

/** What a query looks like read back from the store, with every field the DSL fills in. */
function stored(query: ReturnType<typeof listingQuery>): Query_Serialize {
  return {
    from: { types: query.from?.types ?? [], under: query.from?.under ?? null },
    where: (query.where ?? []).map((condition) => ({
      field: condition.field,
      op: condition.op,
      value: condition.value ?? null,
    })),
    search: null,
    related_to: null,
    sort: query.sort ?? MANUAL,
    limit: query.limit ?? 50,
    offset: query.offset ?? 0,
  } as Query_Serialize;
}

const byTitle: Sort = { by: "title", field: null, descending: true };
const done = { field: "status", op: "eq" as const, value: "done" };

describe("a listing as a query", () => {
  it("names a parent's children with a condition", () => {
    expect(listingQuery(listingOf("p1"))).toMatchObject({
      from: {},
      where: [{ field: "$parent_id", op: "eq", value: "p1" }],
      limit: 200,
      offset: 0,
    });
  });

  it("names the top level by an empty parent", () => {
    const query = listingQuery({ source: { kind: "children", parent: null }, conditions: [] });
    expect(query.where).toEqual([{ field: "$parent_id", op: "is_empty", value: null }]);
  });

  it("names a type by where it looks, and adds the conditions", () => {
    const query = listingQuery(
      { source: { kind: "type", type_id: "t1" }, conditions: [done] },
      byTitle,
      400,
      200,
    );
    expect(query).toEqual({
      from: { types: ["t1"] },
      where: [done],
      sort: byTitle,
      limit: 200,
      offset: 400,
    });
  });
});

describe("a saved query as a listing", () => {
  const listings: Listing[] = [
    { source: { kind: "children", parent: "p1" }, conditions: [] },
    { source: { kind: "children", parent: "p1" }, conditions: [done] },
    { source: { kind: "children", parent: null }, conditions: [done] },
    { source: { kind: "type", type_id: "t1" }, conditions: [done] },
  ];

  it("comes back as it was saved", () => {
    for (const listing of listings) {
      const { listing: back, sort } = listingFromQuery(
        stored(listingQuery(listing, byTitle)),
        "other",
      );
      expect(back).toEqual(listing);
      expect(sort).toEqual(byTitle);
    }
  });

  it("falls back on the object when the query names no source", () => {
    const { listing } = listingFromQuery(
      stored({ from: {}, where: [done], sort: MANUAL }),
      "object",
    );
    expect(listing).toEqual({ source: { kind: "children", parent: "object" }, conditions: [done] });
  });
});
