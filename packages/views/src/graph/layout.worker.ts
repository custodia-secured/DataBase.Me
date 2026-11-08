// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * The Graph's layout (M8, Task 11, ADR-0034): ForceAtlas2 in a worker of ours, since the library's own
 * worker is a `blob:` URL the CSP refuses (gate 1c). It does nothing until asked: the page asks for the
 * next positions when it has drawn the last, so pausing is just not asking, and a paused layout costs
 * nothing.
 */
// @ts-expect-error The package ships no types for its inner modules.
import iterate from "graphology-layout-forceatlas2/iterate.js";

export type LayoutRequest =
  | { kind: "start"; positions: Float32Array; edges: Uint32Array; settings: object }
  | { kind: "next"; budgetMs: number };

export interface LayoutReply {
  /** x, y, x, y, in the order the page gave them. */
  positions: Float32Array;
  /** How many passes have run since "start". */
  passes: number;
}

// The record sizes the library's own helpers use.
const PPN = 10;
const PPE = 3;

const run = iterate as (settings: object, nodes: Float32Array, edges: Float32Array) => void;

let nodeMatrix = new Float32Array(0);
let edgeMatrix = new Float32Array(0);
let settings: object = {};
let passes = 0;

function start(job: Extract<LayoutRequest, { kind: "start" }>): void {
  const count = job.positions.length / 2;
  nodeMatrix = new Float32Array(count * PPN);
  edgeMatrix = new Float32Array((job.edges.length / 2) * PPE);
  settings = job.settings;
  passes = 0;
  const degree = new Uint32Array(count);
  for (let at = 0; at + 1 < job.edges.length; at += 2) {
    const source = job.edges[at] ?? 0;
    const target = job.edges[at + 1] ?? 0;
    degree[source] = (degree[source] ?? 0) + 1;
    degree[target] = (degree[target] ?? 0) + 1;
    const place = (at / 2) * PPE;
    edgeMatrix[place] = source * PPN;
    edgeMatrix[place + 1] = target * PPN;
    edgeMatrix[place + 2] = 1;
  }
  for (let node = 0; node < count; node += 1) {
    const place = node * PPN;
    nodeMatrix[place] = job.positions[node * 2] ?? 0;
    nodeMatrix[place + 1] = job.positions[node * 2 + 1] ?? 0;
    // Mass, then convergence, as the library's own helper fills them.
    nodeMatrix[place + 6] = (degree[node] ?? 0) + 1;
    nodeMatrix[place + 7] = 1;
    nodeMatrix[place + 8] = 1;
  }
}

/** Runs passes for about `budgetMs`, and at least one, and sends the positions over. */
function next(budgetMs: number): void {
  const began = performance.now();
  do {
    run(settings, nodeMatrix, edgeMatrix);
    passes += 1;
  } while (performance.now() - began < budgetMs);
  const count = nodeMatrix.length / PPN;
  const positions = new Float32Array(count * 2);
  for (let node = 0; node < count; node += 1) {
    positions[node * 2] = nodeMatrix[node * PPN] ?? 0;
    positions[node * 2 + 1] = nodeMatrix[node * PPN + 1] ?? 0;
  }
  const reply: LayoutReply = { positions, passes };
  self.postMessage(reply, { transfer: [positions.buffer] });
}

self.onmessage = (event: MessageEvent<LayoutRequest>) => {
  if (event.data.kind === "start") start(event.data);
  else next(event.data.budgetMs);
};
