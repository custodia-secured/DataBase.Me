// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * The changes that turn one block tree into another, in an order the vault accepts (ADR-0016).
 * The editor sends them as one `blocks_apply` batch per flush, so each flush is one undo step.
 *
 * The vault checks every change as it applies it: a parent must be a block that may have children,
 * `after` must be a sibling under that parent, and a block keeps no children once it becomes a
 * kind that may not have them. The order below satisfies all three:
 *
 * 1. In document order, each block of the new tree is created, updated or moved into place just
 *    after its previous sibling. Its parent and that sibling are always in place already. Blocks
 *    that keep their parent and lie on a longest increasing run of their old order stay where
 *    they are, so a reorder moves as few blocks as possible.
 * 2. Blocks that are gone are deleted, top-most only: the vault deletes the blocks below them, and
 *    every block that survives was moved out in step 1.
 * 3. Last, blocks that lose the right to have children change kind; by now they have none.
 */
import type { BlockChange_Deserialize } from "@databaseme/client";
import { mayHaveChildren, type BlockTree, type JsonObject } from "./mapping";

interface Flat {
  parent: string | null;
  kind: string;
  content: JsonObject;
}

interface Indexed {
  blocks: Map<string, Flat>;
  children: Map<string | null, string[]>;
}

function index(tree: readonly BlockTree[]): Indexed {
  const blocks = new Map<string, Flat>();
  const children = new Map<string | null, string[]>();
  const walk = (parent: string | null, siblings: readonly BlockTree[]) => {
    children.set(
      parent,
      siblings.map((block) => block.id),
    );
    for (const block of siblings) {
      blocks.set(block.id, { parent, kind: block.kind, content: block.content });
      walk(block.id, block.children);
    }
  };
  walk(null, tree);
  return { blocks, children };
}

/** Deep equality of JSON values; object key order does not matter. */
export function jsonEqual(a: unknown, b: unknown): boolean {
  if (a === b) return true;
  if (typeof a !== "object" || typeof b !== "object" || a === null || b === null) return false;
  if (Array.isArray(a) || Array.isArray(b)) {
    if (!Array.isArray(a) || !Array.isArray(b) || a.length !== b.length) return false;
    return a.every((item, i) => jsonEqual(item, b[i]));
  }
  const aKeys = Object.keys(a);
  const bKeys = Object.keys(b);
  if (aKeys.length !== bKeys.length) return false;
  const bObject = b as Record<string, unknown>;
  const aObject = a as Record<string, unknown>;
  return aKeys.every((key) => key in bObject && jsonEqual(aObject[key], bObject[key]));
}

/** The indexes of a longest strictly increasing subsequence of `values`. */
export function longestIncreasing(values: readonly number[]): number[] {
  // tails[k]: index of the smallest tail of an increasing run of length k + 1.
  const tails: number[] = [];
  const previous: number[] = Array.from({ length: values.length }, () => -1);
  values.forEach((value, i) => {
    let low = 0;
    let high = tails.length;
    while (low < high) {
      const mid = (low + high) >> 1;
      if ((values[tails[mid] ?? 0] ?? 0) < value) low = mid + 1;
      else high = mid;
    }
    if (low > 0) previous[i] = tails[low - 1] ?? -1;
    tails[low] = i;
  });
  const run: number[] = [];
  for (let i = tails.at(-1) ?? -1; i >= 0; i = previous[i] ?? -1) run.push(i);
  return run.reverse();
}

/** Whether `tree` is only the empty paragraph a new document starts with. */
function isBlankStart(tree: readonly BlockTree[]): boolean {
  const [only, ...rest] = tree;
  const inline = only?.content.inline;
  return (
    rest.length === 0 &&
    only?.kind === "paragraph" &&
    Array.isArray(inline) &&
    inline.length === 0 &&
    Object.keys(only.content).length === 1
  );
}

/** The changes that turn `before` into `after`. Both are complete trees of one object's blocks. */
export function diffBlocks(
  before: readonly BlockTree[],
  after: readonly BlockTree[],
): BlockChange_Deserialize[] {
  // Opening an object with no blocks shows one empty paragraph; it is not saved until typed in.
  if (before.length === 0 && isBlankStart(after)) return [];

  const old = index(before);
  const target = index(after);
  const placing: BlockChange_Deserialize[] = [];
  const losing: BlockChange_Deserialize[] = [];

  const staying = (parent: string | null, ids: readonly string[]): Set<string> => {
    const oldOrder = new Map((old.children.get(parent) ?? []).map((id, i) => [id, i]));
    const kept = ids.filter((id) => oldOrder.has(id));
    const run = longestIncreasing(kept.map((id) => oldOrder.get(id) ?? 0));
    return new Set(run.map((i) => kept[i] ?? ""));
  };

  const place = (parent: string | null, ids: readonly string[]) => {
    const anchors = staying(parent, ids);
    let previous: string | null = null;
    for (const id of ids) {
      const block = target.blocks.get(id);
      if (!block) continue;
      const was = old.blocks.get(id);
      if (!was) {
        placing.push({
          op: "create",
          id,
          parent,
          after: previous,
          kind: block.kind,
          content: block.content,
        });
      } else {
        if (was.kind !== block.kind || !jsonEqual(was.content, block.content)) {
          const update: BlockChange_Deserialize = {
            op: "update",
            id,
            kind: block.kind,
            content: block.content,
          };
          if (mayHaveChildren(was.kind) && !mayHaveChildren(block.kind)) losing.push(update);
          else placing.push(update);
        }
        if (!anchors.has(id)) placing.push({ op: "move", id, parent, after: previous });
      }
      place(id, target.children.get(id) ?? []);
      previous = id;
    }
  };
  place(null, target.children.get(null) ?? []);

  const deleting: BlockChange_Deserialize[] = [];
  for (const [id, block] of old.blocks) {
    if (target.blocks.has(id)) continue;
    if (block.parent === null || target.blocks.has(block.parent)) {
      deleting.push({ op: "delete", id });
    }
  }
  return [...placing, ...deleting, ...losing];
}
