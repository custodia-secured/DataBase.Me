// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * Where the mind map's nodes go (ADR-0019): a tidy tree with the root on the left and branches
 * growing right. Positions are computed from the tree every time and never stored. Nodes have a
 * fixed size, so no measuring pass is needed. Everything here is pure and deterministic.
 */
import type { Id, Position, TreeNode_Serialize } from "@databaseme/client";
import { hierarchy, tree } from "d3-hierarchy";

export const NODE_WIDTH = 184;
export const NODE_HEIGHT = 48;
const COLUMN = NODE_WIDTH + 64;
const ROW = NODE_HEIGHT + 12;

/** A node as the map draws it. */
export interface MapNode {
  id: Id;
  title: string;
  /** Its parent on the map; null for the root, whatever is above it in the vault. */
  parent: Id | null;
  /** Its children that were read, in order, shown or not. */
  children: readonly Id[];
  /** All its children in the vault, read or not. */
  childCount: number;
  /** Relations with objects that aren't on the map. */
  outsideRelations: number;
  /** How deep it is: 1 for the root. */
  level: number;
  /** Its place among its siblings, from 0. */
  index: number;
  /** How many siblings it has, counting itself. */
  siblings: number;
  /** Whether its branch is folded away. */
  collapsed: boolean;
  /** The top left corner of the node. */
  x: number;
  y: number;
}

export interface MapLayout {
  /** The nodes shown, depth first in sort-key order. */
  nodes: readonly MapNode[];
  byId: ReadonlyMap<Id, MapNode>;
}

interface Branch {
  id: Id;
  children: Branch[];
}

/**
 * Lays out `tree`'s nodes, which come depth first with the root first. What is below a collapsed
 * node isn't shown. A node whose parent wasn't read (the root only) is the root of the map.
 */
export function layoutMap(
  nodes: readonly TreeNode_Serialize[],
  collapsed: ReadonlySet<Id>,
): MapLayout {
  const first = nodes[0];
  if (!first) return { nodes: [], byId: new Map() };

  const children = new Map<Id, Id[]>(nodes.map((node) => [node.id, []]));
  const shown = new Map<Id, Branch>();
  const root: Branch = { id: first.id, children: [] };
  shown.set(first.id, root);
  const levels = new Map<Id, number>([[first.id, 1]]);
  for (const node of nodes.slice(1)) {
    const parent = node.parent_id;
    if (parent === null) continue;
    children.get(parent)?.push(node.id);
    levels.set(node.id, (levels.get(parent) ?? 0) + 1);
    const above = shown.get(parent);
    if (!above || collapsed.has(parent)) continue;
    const branch: Branch = { id: node.id, children: [] };
    above.children.push(branch);
    shown.set(node.id, branch);
  }

  const places = new Map<Id, { x: number; y: number }>();
  const laid = tree<Branch>()
    .nodeSize([ROW, COLUMN])
    .separation(() => 1)(hierarchy(root));
  // d3 grows trees downward: its x is across the branches, and its y is along them.
  laid.each((point) => {
    places.set(point.data.id, { x: point.y, y: point.x - NODE_HEIGHT / 2 });
  });

  const result: MapNode[] = [];
  const byId = new Map<Id, MapNode>();
  for (const node of nodes) {
    const place = places.get(node.id);
    if (!place) continue;
    const parent = node.id === first.id ? null : node.parent_id;
    const family = parent === null ? [node.id] : (children.get(parent) ?? []);
    const mapped: MapNode = {
      id: node.id,
      title: node.title,
      parent,
      children: children.get(node.id) ?? [],
      childCount: node.child_count,
      outsideRelations: node.outside_relations,
      level: levels.get(node.id) ?? 1,
      index: family.indexOf(node.id),
      siblings: family.length,
      collapsed: collapsed.has(node.id),
      ...place,
    };
    result.push(mapped);
    byId.set(node.id, mapped);
  }
  return { nodes: result, byId };
}

/** A sibling of `node`, `step` places along, if there is one. */
export function sibling(layout: MapLayout, node: MapNode, step: 1 | -1): Id | null {
  if (node.parent === null) return null;
  return layout.byId.get(node.parent)?.children[node.index + step] ?? null;
}

/** Whether `id` is `ancestor` or somewhere below it. */
export function isWithin(layout: MapLayout, id: Id, ancestor: Id): boolean {
  for (let at: Id | null = id; at !== null; at = layout.byId.get(at)?.parent ?? null) {
    if (at === ancestor) return true;
  }
  return false;
}

/** The node at a point on the map, if there is one. */
export function nodeAt(layout: MapLayout, x: number, y: number): MapNode | null {
  for (const node of layout.nodes) {
    if (x >= node.x && x <= node.x + NODE_WIDTH && y >= node.y && y <= node.y + NODE_HEIGHT) {
      return node;
    }
  }
  return null;
}

/** The viewport that shows all of `layout` in a view of this size, within these zooms. */
export function fitViewport(
  layout: MapLayout,
  width: number,
  height: number,
  minZoom: number,
  maxZoom: number,
): { x: number; y: number; zoom: number } {
  let left = Infinity;
  let top = Infinity;
  let right = -Infinity;
  let bottom = -Infinity;
  for (const node of layout.nodes) {
    left = Math.min(left, node.x);
    top = Math.min(top, node.y);
    right = Math.max(right, node.x + NODE_WIDTH);
    bottom = Math.max(bottom, node.y + NODE_HEIGHT);
  }
  if (layout.nodes.length === 0 || width <= 0 || height <= 0) return { x: 0, y: 0, zoom: 1 };
  const padding = 0.92;
  const zoom = Math.min(
    maxZoom,
    Math.max(
      minZoom,
      Math.min((width * padding) / (right - left), (height * padding) / (bottom - top)),
    ),
  );
  return {
    x: width / 2 - ((left + right) / 2) * zoom,
    y: height / 2 - ((top + bottom) / 2) * zoom,
    zoom,
  };
}

/** What dropping a dragged node would do. */
export interface Drop {
  /** The node under the pointer. */
  target: Id;
  where: "inside" | "before" | "after";
  parent: Id;
  position: Position;
}

/**
 * The drop at a point on the map: the middle of a node reparents onto it, its top and bottom
 * quarters place the dragged node before or after it. A drop never sets a free position, and a
 * node can't be dropped into its own branch.
 */
export function dropAt(layout: MapLayout, dragged: Id, x: number, y: number): Drop | null {
  for (const node of layout.nodes) {
    if (node.id === dragged) continue;
    if (x < node.x || x > node.x + NODE_WIDTH || y < node.y || y > node.y + NODE_HEIGHT) continue;
    if (isWithin(layout, node.id, dragged)) return null;
    const share = (y - node.y) / NODE_HEIGHT;
    if (node.parent !== null && share < 0.25) {
      return {
        target: node.id,
        where: "before",
        parent: node.parent,
        position: { before: node.id },
      };
    }
    if (node.parent !== null && share > 0.75) {
      return { target: node.id, where: "after", parent: node.parent, position: { after: node.id } };
    }
    return { target: node.id, where: "inside", parent: node.id, position: "last" };
  }
  return null;
}
