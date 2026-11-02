// SPDX-License-Identifier: AGPL-3.0-or-later
import type { Id, TreeNode_Serialize } from "@databaseme/client";
import fc from "fast-check";
import { describe, expect, it } from "vitest";
import {
  dropAt,
  fitViewport,
  isWithin,
  layoutMap,
  NODE_HEIGHT,
  NODE_WIDTH,
  nodeAt,
  sibling,
  type MapLayout,
  type MapNode,
} from "./layout";

function topic(id: Id, parent: Id | null, childCount = 0, outside = 0): TreeNode_Serialize {
  return {
    id,
    parent_id: parent,
    title: `Title of ${id}`,
    type_ids: [],
    child_count: childCount,
    outside_relations: outside,
  };
}

/** The root, with "vault" above it: a, with a1 and a2 below; b, with b1 below; and c. */
const FAMILY = [
  topic("root", "vault", 3),
  topic("a", "root", 2),
  topic("a1", "a"),
  topic("a2", "a"),
  topic("b", "root", 1),
  topic("b1", "b"),
  topic("c", "root"),
];

function at(layout: MapLayout, id: Id): MapNode {
  const node = layout.byId.get(id);
  if (!node) throw new Error(`${id} isn't on the map`);
  return node;
}

const idsOf = (layout: MapLayout) => layout.nodes.map((node) => node.id);

/**
 * Any tree, depth first with the root first, as `object_tree` returns it. Each new node picks its
 * parent; -1 picks the last node added, so that deep chains turn up as well as wide families.
 */
const treeArb = fc
  .array(fc.oneof(fc.nat(), fc.constant(-1)), { maxLength: 150 })
  .map((picks): TreeNode_Serialize[] => {
    const below = new Map<number, number[]>();
    picks.forEach((pick, i) => {
      const parent = pick < 0 ? i : pick % (i + 1);
      below.set(parent, [...(below.get(parent) ?? []), i + 1]);
    });
    const nodes: TreeNode_Serialize[] = [];
    const walk = (n: number, parent: Id | null) => {
      const id = String(n);
      const children = below.get(n) ?? [];
      nodes.push(topic(id, parent, children.length));
      for (const child of children) walk(child, id);
    };
    walk(0, null);
    return nodes;
  });

const mapArb = treeArb.chain((nodes) =>
  fc.subarray(nodes.map((node) => node.id)).map((ids) => ({ nodes, collapsed: new Set<Id>(ids) })),
);

describe("layoutMap, for any tree", () => {
  it("never overlaps two nodes", () => {
    fc.assert(
      fc.property(mapArb, ({ nodes, collapsed }) => {
        const shown = layoutMap(nodes, collapsed).nodes;
        return shown.every((a, i) =>
          shown
            .slice(i + 1)
            .every((b) => Math.abs(a.x - b.x) >= NODE_WIDTH || Math.abs(a.y - b.y) >= NODE_HEIGHT),
        );
      }),
    );
  });

  it("stands a parent to the left of its children, clear of them", () => {
    fc.assert(
      fc.property(mapArb, ({ nodes, collapsed }) => {
        const layout = layoutMap(nodes, collapsed);
        return layout.nodes.every(
          (node) => node.parent === null || node.x >= at(layout, node.parent).x + NODE_WIDTH,
        );
      }),
    );
  });

  it("puts siblings down the map in the order they came", () => {
    fc.assert(
      fc.property(mapArb, ({ nodes, collapsed }) => {
        const layout = layoutMap(nodes, collapsed);
        return layout.nodes.every((node) => {
          const family = node.children.flatMap((id) => layout.byId.get(id)?.y ?? []);
          return family.every((y, i) => i === 0 || y > (family[i - 1] ?? Infinity));
        });
      }),
    );
  });

  it("gives the same answer every time", () => {
    fc.assert(
      fc.property(mapArb, ({ nodes, collapsed }) => {
        const copy = nodes.map((node) => ({ ...node }));
        expect(layoutMap(copy, new Set(collapsed))).toEqual(layoutMap(nodes, collapsed));
      }),
    );
  });

  it("shows a node exactly when nothing above it is collapsed, in the order they came", () => {
    fc.assert(
      fc.property(mapArb, ({ nodes, collapsed }) => {
        const parents = new Map(nodes.map((node) => [node.id, node.parent_id]));
        const hidden = (id: Id) => {
          for (let up = parents.get(id) ?? null; up !== null; up = parents.get(up) ?? null) {
            if (collapsed.has(up)) return true;
          }
          return false;
        };
        const expected = nodes.map((node) => node.id).filter((id) => !hidden(id));
        expect(idsOf(layoutMap(nodes, collapsed))).toEqual(expected);
      }),
    );
  });
});

describe("layoutMap's nodes", () => {
  const layout = layoutMap(FAMILY, new Set());

  it("draws nothing for no nodes", () => {
    const empty = layoutMap([], new Set());
    expect(empty.nodes).toEqual([]);
    expect(empty.byId.size).toBe(0);
  });

  it("treats the first node as the root, whatever is above it in the vault", () => {
    expect(at(layout, "root")).toMatchObject({ parent: null, level: 1, index: 0, siblings: 1 });
    expect(layout.byId.has("vault")).toBe(false);
  });

  it("gives each node its place in the tree", () => {
    expect(at(layout, "root").children).toEqual(["a", "b", "c"]);
    expect(at(layout, "a")).toMatchObject({
      parent: "root",
      children: ["a1", "a2"],
      level: 2,
      index: 0,
      siblings: 3,
    });
    expect(at(layout, "a2")).toMatchObject({ parent: "a", level: 3, index: 1, siblings: 2 });
    expect(at(layout, "c")).toMatchObject({ children: [], index: 2 });
  });

  it("carries the title and the counts from the tree", () => {
    const counted = layoutMap([topic("root", null, 7, 4)], new Set());
    expect(at(counted, "root")).toMatchObject({
      title: "Title of root",
      childCount: 7,
      outsideRelations: 4,
    });
  });

  it("counts children the limit left out, without listing them", () => {
    const cut = layoutMap([topic("root", null, 5), topic("a", "root")], new Set());
    expect(at(cut, "root")).toMatchObject({ childCount: 5, children: ["a"] });
  });

  it("marks a collapsed node, which still lists the children it hides", () => {
    const folded = layoutMap(FAMILY, new Set(["a"]));
    expect(at(folded, "a")).toMatchObject({ collapsed: true, children: ["a1", "a2"] });
    expect(at(folded, "b").collapsed).toBe(false);
    expect(idsOf(folded)).toEqual(["root", "a", "b", "b1", "c"]);
  });

  it("closes the gap a collapsed branch leaves", () => {
    const folded = layoutMap(FAMILY, new Set(["a"]));
    expect(at(folded, "c").y - at(folded, "a").y).toBeLessThan(
      at(layout, "c").y - at(layout, "a").y,
    );
  });
});

describe("sibling", () => {
  const layout = layoutMap(FAMILY, new Set());

  it("finds the next and the previous", () => {
    expect(sibling(layout, at(layout, "b"), 1)).toBe("c");
    expect(sibling(layout, at(layout, "b"), -1)).toBe("a");
  });

  it("stops at the ends of the family", () => {
    expect(sibling(layout, at(layout, "a"), -1)).toBeNull();
    expect(sibling(layout, at(layout, "c"), 1)).toBeNull();
  });

  it("gives the root none", () => {
    expect(sibling(layout, at(layout, "root"), 1)).toBeNull();
  });
});

describe("isWithin", () => {
  const layout = layoutMap(FAMILY, new Set());

  it("counts a node and everything below it", () => {
    expect(isWithin(layout, "a", "a")).toBe(true);
    expect(isWithin(layout, "a1", "a")).toBe(true);
    expect(isWithin(layout, "a1", "root")).toBe(true);
  });

  it("doesn't count what is above it or beside it", () => {
    expect(isWithin(layout, "a", "a1")).toBe(false);
    expect(isWithin(layout, "b1", "a")).toBe(false);
  });
});

describe("nodeAt", () => {
  const layout = layoutMap(FAMILY, new Set());
  const a = at(layout, "a");

  it("finds a node anywhere on it, edges included", () => {
    expect(nodeAt(layout, a.x + 1, a.y + 1)?.id).toBe("a");
    expect(nodeAt(layout, a.x, a.y)?.id).toBe("a");
    expect(nodeAt(layout, a.x + NODE_WIDTH, a.y + NODE_HEIGHT)?.id).toBe("a");
  });

  it("finds nothing in the gaps", () => {
    expect(nodeAt(layout, a.x - 1, a.y)).toBeNull();
    expect(nodeAt(layout, a.x + NODE_WIDTH + 1, a.y)).toBeNull();
    expect(nodeAt(layout, -10_000, -10_000)).toBeNull();
  });
});

describe("fitViewport", () => {
  it("shows every node of any tree", () => {
    fc.assert(
      fc.property(treeArb, fc.integer({ min: 200, max: 3000 }), (nodes, width) => {
        const layout = layoutMap(nodes, new Set());
        const height = 600;
        const view = fitViewport(layout, width, height, 0.0001, 4);
        return layout.nodes.every((node) => {
          const left = node.x * view.zoom + view.x;
          const top = node.y * view.zoom + view.y;
          return (
            left >= 0 &&
            top >= 0 &&
            left + NODE_WIDTH * view.zoom <= width &&
            top + NODE_HEIGHT * view.zoom <= height
          );
        });
      }),
    );
  });

  it("keeps to the zooms it is given", () => {
    const one = layoutMap([topic("root", null)], new Set());
    expect(fitViewport(one, 1000, 1000, 0.1, 1).zoom).toBe(1);
    const family = layoutMap(FAMILY, new Set());
    expect(fitViewport(family, 10, 10, 0.5, 1).zoom).toBe(0.5);
  });

  it("centres a node that needs no zooming", () => {
    const one = layoutMap([topic("root", null)], new Set());
    const view = fitViewport(one, 1000, 800, 0.1, 1);
    const node = at(one, "root");
    expect(node.x + view.x + NODE_WIDTH / 2).toBeCloseTo(500);
    expect(node.y + view.y + NODE_HEIGHT / 2).toBeCloseTo(400);
  });

  it("falls back when there is nothing to fit, or nothing to fit it in", () => {
    const fallback = { x: 0, y: 0, zoom: 1 };
    expect(fitViewport(layoutMap([], new Set()), 800, 600, 0.1, 1)).toEqual(fallback);
    expect(fitViewport(layoutMap(FAMILY, new Set()), 0, 600, 0.1, 1)).toEqual(fallback);
    expect(fitViewport(layoutMap(FAMILY, new Set()), 800, 0, 0.1, 1)).toEqual(fallback);
  });
});

describe("dropAt", () => {
  const layout = layoutMap(FAMILY, new Set());
  const b = at(layout, "b");
  const onB = (share: number) => dropAt(layout, "c", b.x + 10, b.y + NODE_HEIGHT * share);

  it("reparents onto the middle of a node, as its last child", () => {
    expect(onB(0.5)).toEqual({ target: "b", where: "inside", parent: "b", position: "last" });
  });

  it("places before a node from its top quarter", () => {
    expect(onB(0.1)).toEqual({
      target: "b",
      where: "before",
      parent: "root",
      position: { before: "b" },
    });
  });

  it("places after a node from its bottom quarter", () => {
    expect(onB(0.9)).toEqual({
      target: "b",
      where: "after",
      parent: "root",
      position: { after: "b" },
    });
  });

  it("only reparents onto the root, which can't have siblings", () => {
    const root = at(layout, "root");
    expect(dropAt(layout, "c", root.x + 10, root.y + 1)?.where).toBe("inside");
    expect(dropAt(layout, "c", root.x + 10, root.y + NODE_HEIGHT - 1)?.where).toBe("inside");
  });

  it("refuses a drop into the dragged node's own branch", () => {
    const a1 = at(layout, "a1");
    expect(dropAt(layout, "a", a1.x + 10, a1.y + NODE_HEIGHT / 2)).toBeNull();
  });

  it("ignores the dragged node itself", () => {
    const c = at(layout, "c");
    expect(dropAt(layout, "c", c.x + 10, c.y + NODE_HEIGHT / 2)).toBeNull();
  });

  it("does nothing in a gap", () => {
    expect(dropAt(layout, "c", -10_000, -10_000)).toBeNull();
  });
});
