// SPDX-License-Identifier: AGPL-3.0-or-later
import { describe, expect, it } from "vitest";
import {
  movedTile,
  tilesOf,
  widenedTile,
  withoutTile,
  withTile,
  type DashboardOptions,
} from "./tiles";

const tile = (view: string, wide = false) => ({ view, wide });
const views = (tiles: readonly { view: string }[]) => tiles.map((one) => one.view);

describe("the tiles in saved settings", () => {
  it("are what is there, in order", () => {
    expect(tilesOf({ tiles: [tile("a"), tile("b", true)] })).toEqual([tile("a"), tile("b", true)]);
  });

  it("drop what is not a tile, since settings come from files too", () => {
    const junk = {
      tiles: [tile("a"), null, 3, { wide: true }, { view: 7 }, "b"],
    } as unknown as DashboardOptions;
    expect(tilesOf(junk)).toEqual([tile("a")]);
    expect(tilesOf({ tiles: "nope" } as unknown as DashboardOptions)).toEqual([]);
    expect(tilesOf(undefined)).toEqual([]);
    expect(tilesOf({})).toEqual([]);
  });

  it("show a view once", () => {
    expect(views(tilesOf({ tiles: [tile("a"), tile("a")] }))).toEqual(["a"]);
  });

  it("are only wide when they say so", () => {
    const odd = { tiles: [{ view: "a", wide: "yes" }] } as unknown as DashboardOptions;
    expect(tilesOf(odd)).toEqual([tile("a")]);
  });
});

describe("changing the tiles", () => {
  const three = [tile("a"), tile("b"), tile("c")];

  it("adds at the end, and not twice", () => {
    expect(views(withTile(three, "d"))).toEqual(["a", "b", "c", "d"]);
    expect(views(withTile(three, "b"))).toEqual(["a", "b", "c"]);
  });

  it("removes", () => {
    expect(views(withoutTile(three, "b"))).toEqual(["a", "c"]);
    expect(views(withoutTile(three, "z"))).toEqual(["a", "b", "c"]);
  });

  it("moves along, stopping at either end", () => {
    expect(views(movedTile(three, "a", 1))).toEqual(["b", "a", "c"]);
    expect(views(movedTile(three, "c", -1))).toEqual(["a", "c", "b"]);
    expect(views(movedTile(three, "a", -1))).toEqual(["a", "b", "c"]);
    expect(views(movedTile(three, "c", 1))).toEqual(["a", "b", "c"]);
    expect(views(movedTile(three, "z", 1))).toEqual(["a", "b", "c"]);
  });

  it("widens and narrows one tile", () => {
    const wide = widenedTile(three, "b");
    expect(wide[1]).toEqual(tile("b", true));
    expect(wide[0]).toEqual(tile("a"));
    expect(widenedTile(wide, "b")[1]).toEqual(tile("b"));
  });
});
