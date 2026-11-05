// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * Gate 1c's layout: one pass of the same force calculation a worker of ours would run, over flat
 * arrays, from a same-origin script (the CSP gives workers `'self'`, and the library's own worker is
 * a `blob:` URL it would refuse).
 */
// @ts-expect-error The package ships no types for its inner module.
import iterate from "graphology-layout-forceatlas2/iterate.js";

interface Job {
  nodes: [number, number][];
  edges: [number, number][];
  settings: Record<string, unknown>;
}

// The record sizes the library's own helpers use.
const PPN = 10;
const PPE = 3;

self.onmessage = (event: MessageEvent<Job>) => {
  const { nodes, edges, settings } = event.data;
  const nodeMatrix = new Float32Array(nodes.length * PPN);
  const edgeMatrix = new Float32Array(edges.length * PPE);
  const degree = new Float32Array(nodes.length);
  for (const [source, target] of edges) {
    degree[source] = (degree[source] ?? 0) + 1;
    degree[target] = (degree[target] ?? 0) + 1;
  }
  nodes.forEach(([x, y], index) => {
    const at = index * PPN;
    nodeMatrix[at] = x;
    nodeMatrix[at + 1] = y;
    nodeMatrix[at + 6] = (degree[index] ?? 0) + 1;
    nodeMatrix[at + 7] = 1;
  });
  edges.forEach(([source, target], index) => {
    const at = index * PPE;
    edgeMatrix[at] = source * PPN;
    edgeMatrix[at + 1] = target * PPN;
    edgeMatrix[at + 2] = 1;
  });
  const before = nodeMatrix[0];
  (iterate as (options: unknown, nodes: Float32Array, edges: Float32Array) => void)(
    settings,
    nodeMatrix,
    edgeMatrix,
  );
  self.postMessage({ moved: Number(nodeMatrix[0] !== before) });
};
