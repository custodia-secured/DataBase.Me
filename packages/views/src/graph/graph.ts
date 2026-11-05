// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * What the Graph view works out (M8, Task 11, ADR-0034), with no library in it: which objects a graph
 * reads, where a node starts, who a node's neighbours are, and how the layout is paced.
 */
import { MANUAL, type GraphData, type Id, type Query_Deserialize } from "@databaseme/client";
import type { Listing } from "../framework/listing";

/** The most nodes one graph reads, which is also the most the store gives. */
export const MOST_NODES = 200_000;

/** What a graph draws: what is below this page, or the whole vault. */
export type GraphScope = "here" | "vault";

export interface GraphOptions {
  scope: GraphScope;
}

/**
 * The query of a graph. "Here" reaches all the way down, where the other views read one level, since
 * what a graph is for is how things join up, and one level has no parent links to draw.
 */
export function graphQuery({ source, conditions }: Listing, scope: GraphScope): Query_Deserialize {
  const everywhere = scope === "vault";
  const from =
    source.kind === "type"
      ? { types: [source.type_id] }
      : !everywhere && source.parent
        ? { under: source.parent }
        : {};
  // The nodes' limit goes beside the query (`MOST_NODES`), as the store bounds a page of results lower.
  return { from, where: [...conditions], sort: MANUAL };
}

/**
 * Where a node starts, in a disc of radius 1, from its ID alone (FNV-1a, 32 bits). So the first frame
 * waits for no layout, and a node stays put when others come and go, which it would not if this went by
 * its place in the list.
 */
export function startOf(id: Id): { x: number; y: number } {
  let hash = 0x811c9dc5;
  for (let at = 0; at < id.length; at += 1) {
    hash = Math.imul(hash ^ id.charCodeAt(at), 0x01000193);
  }
  // Two numbers from one hash. The second is mixed again, or the angle would follow the radius.
  const first = (hash >>> 0) / 0x1_0000_0000;
  const second = (Math.imul(hash ^ (hash >>> 15), 0x2c1b3c6d) >>> 0) / 0x1_0000_0000;
  // The square root spreads them evenly over the disc, rather than bunching them at its middle.
  const radius = Math.sqrt(first);
  const angle = second * 2 * Math.PI;
  return { x: radius * Math.cos(angle), y: radius * Math.sin(angle) };
}

/** How wide the starting disc is: wide enough that the layout does not begin from a crush. */
export function spreadOf(nodes: number): number {
  return Math.max(1, Math.sqrt(nodes));
}

/** One node's links, with each list in the order the store gave. */
export interface Links {
  /** Nodes it has a relation with, either way round. */
  related: readonly number[];
  /** The node it sits inside, if that is in the graph. */
  parent: number | null;
  /** The nodes that sit inside it. */
  children: readonly number[];
}

/** One index per node, as flat arrays, since a graph may have 100,000 nodes. */
export interface LinkIndex {
  relatedFrom: Uint32Array;
  related: Uint32Array;
  childrenFrom: Uint32Array;
  children: Uint32Array;
  /** Each node's parent, or -1. */
  parents: Int32Array;
}

function compressed(pairs: readonly number[], nodes: number, bothWays: boolean) {
  const from = new Uint32Array(nodes + 1);
  const count = (node: number | undefined) => {
    if (node !== undefined && node + 1 < from.length) from[node + 1] = (from[node + 1] ?? 0) + 1;
  };
  for (let at = 0; at + 1 < pairs.length; at += 2) {
    const [source, target] = [pairs[at], pairs[at + 1]];
    count(source);
    if (bothWays && source !== target) count(target);
  }
  for (let node = 0; node < nodes; node += 1)
    from[node + 1] = (from[node + 1] ?? 0) + (from[node] ?? 0);
  const to = new Uint32Array(from[nodes] ?? 0);
  const next = from.slice(0, nodes);
  const put = (node: number, other: number) => {
    const place = next[node] ?? 0;
    to[place] = other;
    next[node] = place + 1;
  };
  for (let at = 0; at + 1 < pairs.length; at += 2) {
    const source = pairs[at];
    const target = pairs[at + 1];
    if (source === undefined || target === undefined) continue;
    put(source, target);
    if (bothWays && source !== target) put(target, source);
  }
  return { from, to };
}

export function indexLinks(data: GraphData): LinkIndex {
  const nodes = data.ids.length;
  const relations = compressed(data.edges, nodes, true);
  const family = compressed(data.parent_edges, nodes, false);
  const parents = new Int32Array(nodes).fill(-1);
  for (let at = 0; at + 1 < data.parent_edges.length; at += 2) {
    const parent = data.parent_edges[at];
    const child = data.parent_edges[at + 1];
    if (parent !== undefined && child !== undefined) parents[child] = parent;
  }
  return {
    relatedFrom: relations.from,
    related: relations.to,
    childrenFrom: family.from,
    children: family.to,
    parents,
  };
}

export function linksOf(index: LinkIndex, node: number): Links {
  const parent = index.parents[node] ?? -1;
  return {
    related: Array.from(
      index.related.subarray(index.relatedFrom[node] ?? 0, index.relatedFrom[node + 1] ?? 0),
    ),
    parent: parent < 0 ? null : parent,
    children: Array.from(
      index.children.subarray(index.childrenFrom[node] ?? 0, index.childrenFrom[node + 1] ?? 0),
    ),
  };
}

/** A node's title, or what stands in for an empty one. */
export function titleOf(titles: readonly string[], node: number): string {
  const title = titles[node] ?? "";
  return title === "" ? "Untitled" : title;
}

/** The relation count and the first few names, for a row of the list. */
export function namesOf(titles: readonly string[], nodes: readonly number[], most = 3): string {
  if (nodes.length === 0) return "";
  const named = nodes.slice(0, most).map((node) => titleOf(titles, node));
  const rest = nodes.length - named.length;
  return rest > 0 ? `${named.join(", ")} and ${String(rest)} more` : named.join(", ");
}

/** Which nodes have `text` in their title, ignoring case; `null` when nothing is being looked for. */
export function search(titles: readonly string[], text: string): number[] | null {
  const wanted = text.trim().toLowerCase();
  if (wanted === "") return null;
  const found: number[] = [];
  titles.forEach((title, node) => {
    if (title.toLowerCase().includes(wanted)) found.push(node);
  });
  return found;
}

const plural = (count: number, one: string, many: string) =>
  `${count.toLocaleString("en")} ${count === 1 ? one : many}`;

/** The graph in words: what the picture says to anyone who cannot see it. */
export function graphSummary(data: GraphData): string {
  const links = (data.edges.length + data.parent_edges.length) / 2;
  return `${plural(data.ids.length, "item", "items")} and ${
    links === 0 ? "no links" : plural(links, "link", "links")
  }.`;
}

/** The layout's input, as arrays a worker can take over. */
export interface LayoutJob {
  /** x, y, x, y. */
  positions: Float32Array;
  /** source, target, source, target, as node indexes, with no node joined to itself. */
  edges: Uint32Array;
}

export function layoutJob(
  data: GraphData,
  placeOf: (id: Id) => { x: number; y: number },
): LayoutJob {
  const positions = new Float32Array(data.ids.length * 2);
  data.ids.forEach((id, node) => {
    const { x, y } = placeOf(id);
    positions[node * 2] = x;
    positions[node * 2 + 1] = y;
  });
  const pairs = [data.edges, data.parent_edges];
  const total = pairs.reduce((sum, list) => sum + list.length, 0);
  const edges = new Uint32Array(total);
  let filled = 0;
  for (const list of pairs) {
    for (let at = 0; at + 1 < list.length; at += 2) {
      const source = list[at];
      const target = list[at + 1];
      if (source === undefined || target === undefined || source === target) continue;
      edges[filled] = source;
      edges[filled + 1] = target;
      filled += 2;
    }
  }
  return { positions, edges: edges.slice(0, filled) };
}

/**
 * How many passes one run of the layout makes: more for a small graph, which also takes them quickly.
 * "Run layout" goes on from there. A pass over 100,000 nodes is about half a second (M8 gate 1c).
 */
export function passesFor(nodes: number): number {
  return Math.round(Math.max(100, Math.min(1000, 2_000_000 / Math.max(1, nodes))));
}

/**
 * How long to wait before asking for the next positions, given how long the last ones took to draw: the
 * page keeps nine tenths of its time, so a big graph settles in steps and stays usable as it does.
 */
export function waitAfter(drawnInMs: number): number {
  return Math.max(16, drawnInMs * 9);
}

/** How many types get a colour of their own; the rest share one, and the list says which is which. */
export const TYPE_COLOURS = 8;

/** Which colour slot a node takes: 0 to 7 for its type, or -1 for no type or one past the palette. */
export function colourSlot(typeIndex: number): number {
  return typeIndex >= 0 && typeIndex < TYPE_COLOURS ? typeIndex : -1;
}
