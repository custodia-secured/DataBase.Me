// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * Test helpers for the editor: block trees made by fast-check, and an in-memory store that applies
 * block changes under the vault's rules (crates/store/src/txn.rs), refusing what the vault refuses.
 */
import type { BlockChange_Deserialize } from "@databaseme/client";
import fc from "fast-check";
import { mayHaveChildren, MARKS, type BlockTree, type JsonObject, type Run } from "./mapping";

/** IDs the arbitrary trees draw from, so two trees share blocks. */
export const ID_POOL = Array.from(
  { length: 14 },
  (_, i) => `01JB3R7Q8Y5K3M2N4P6Q8R0S${String(i).padStart(2, "0")}`,
);
export const OBJECT_IDS = ["01JB3R7Q8Y5K3M2N4P6Q8R0OB1", "01JB3R7Q8Y5K3M2N4P6Q8R0OB2"];

const textArb = fc.stringMatching(/^[a-c \n]{1,6}$/);

const textRunArb: fc.Arbitrary<Run> = fc
  .record(
    {
      text: textArb,
      bold: fc.constant(true as const),
      italic: fc.constant(true as const),
      underline: fc.constant(true as const),
      strike: fc.constant(true as const),
      code: fc.constant(true as const),
      link: fc.constantFrom("https://example.org/", "https://databaseme.org/a"),
    },
    { requiredKeys: ["text"] },
  )
  .map((run) => run as Run);

const runArb: fc.Arbitrary<Run> = fc.oneof(
  { weight: 4, arbitrary: textRunArb },
  { weight: 1, arbitrary: fc.constantFrom(...OBJECT_IDS).map((mention) => ({ mention })) },
);

/** Runs as the editor writes them: inline code alone, and no two neighbours with the same marks. */
export function canonicalRuns(runs: readonly Run[]): Run[] {
  const out: Run[] = [];
  for (const raw of runs) {
    let run: Run = raw;
    if ("text" in run && run.code) run = { text: run.text, code: true };
    const last = out.at(-1);
    if (
      last &&
      "text" in last &&
      "text" in run &&
      MARKS.every((mark) => last[mark] === run[mark]) &&
      last.link === run.link
    ) {
      out[out.length - 1] = { ...last, text: last.text + run.text };
    } else {
      out.push({ ...run });
    }
  }
  return out;
}

const inlineArb = fc.array(runArb, { maxLength: 4 }).map((runs) => canonicalRuns(runs) as never);
const extraArb = fc.option(fc.constant({ "x-future": 1 }), { nil: undefined });

const leafArb: fc.Arbitrary<{ kind: string; content: JsonObject }> = fc.oneof(
  fc.record({ kind: fc.constant("paragraph"), content: fc.record({ inline: inlineArb }) }),
  fc.record({
    kind: fc.constant("heading"),
    content: fc.record({ inline: inlineArb, level: fc.integer({ min: 1, max: 6 }) }),
  }),
  fc.record({ kind: fc.constant("quote"), content: fc.record({ inline: inlineArb }) }),
  fc.record({
    kind: fc.constant("code"),
    content: fc.record(
      { code: fc.stringMatching(/^[a-z\n ]{0,8}$/), language: fc.constantFrom("rust", "ts") },
      { requiredKeys: ["code"] },
    ),
  }),
  fc.record({ kind: fc.constant("divider"), content: fc.constant({}) }),
  fc.record({
    kind: fc.constant("image"),
    content: fc.record(
      { blob: fc.constant("sha256:" + "ab".repeat(32)), alt: fc.constantFrom("", "A cat") },
      { requiredKeys: ["blob"] },
    ),
  }),
);

const parentArb: fc.Arbitrary<{ kind: string; content: JsonObject }> = fc.oneof(
  fc.record({
    kind: fc.constantFrom("bullet", "numbered"),
    content: fc.record({ inline: inlineArb }),
  }),
  fc.record({
    kind: fc.constant("todo"),
    content: fc.record({ inline: inlineArb, checked: fc.boolean() }),
  }),
  fc.record({
    kind: fc.constant("kanban"),
    content: fc.record({ columns: fc.array(fc.string({ maxLength: 3 }), { maxLength: 2 }) }),
  }),
);

interface Shape {
  kind: string;
  content: JsonObject;
  children: Shape[];
}

const shapeArb: fc.Arbitrary<Shape[]> = fc.letrec<{ node: Shape; nodes: Shape[] }>((tie) => ({
  node: fc.oneof(
    { depthSize: "small", withCrossShrink: true },
    fc
      .tuple(leafArb, extraArb)
      .map(([leaf, extra]) => ({ ...leaf, content: { ...extra, ...leaf.content }, children: [] })),
    fc.tuple(parentArb, extraArb, tie("nodes")).map(([parent, extra, children]) => ({
      ...parent,
      content: { ...extra, ...parent.content },
      children,
    })),
  ),
  nodes: fc.array(tie("node"), { maxLength: 4 }),
})).nodes;

function count(shapes: readonly Shape[]): number {
  return shapes.reduce((n, shape) => n + 1 + count(shape.children), 0);
}

function label(shapes: readonly Shape[], ids: string[]): BlockTree[] {
  return shapes.map((shape) => {
    const id = ids.shift() ?? "";
    return { id, kind: shape.kind, content: shape.content, children: label(shape.children, ids) };
  });
}

/** A valid block tree whose IDs come from {@link ID_POOL}, in an arbitrary order. */
export const treeArb: fc.Arbitrary<BlockTree[]> = shapeArb
  .filter((shapes) => count(shapes) <= ID_POOL.length)
  .chain((shapes) =>
    fc
      .shuffledSubarray(ID_POOL, { minLength: count(shapes), maxLength: count(shapes) })
      .map((ids) => label(shapes, [...ids])),
  );

/** An in-memory object's blocks that applies changes as the vault does, or throws. */
export class FakeBlocks {
  private readonly blocks = new Map<string, { kind: string; content: JsonObject }>();
  private readonly children = new Map<string | null, string[]>();
  private readonly parents = new Map<string, string | null>();

  constructor(tree: readonly BlockTree[]) {
    const load = (parent: string | null, siblings: readonly BlockTree[]) => {
      this.children.set(
        parent,
        siblings.map((block) => block.id),
      );
      for (const block of siblings) {
        this.blocks.set(block.id, { kind: block.kind, content: block.content });
        this.parents.set(block.id, parent);
        load(block.id, block.children);
      }
    };
    load(null, tree);
  }

  private checkParent(parent: string | null | undefined) {
    if (parent == null) return;
    const block = this.blocks.get(parent);
    if (!block) throw new Error(`parent ${parent} does not exist`);
    if (!mayHaveChildren(block.kind)) throw new Error(`a ${block.kind} block cannot have children`);
  }

  private insert(id: string, parent: string | null, after: string | null) {
    const siblings = this.children.get(parent) ?? [];
    let at = 0;
    if (after !== null) {
      const index = siblings.indexOf(after);
      if (index < 0 || after === id)
        throw new Error(`${after} is not a sibling under ${String(parent)}`);
      at = index + 1;
    }
    siblings.splice(at, 0, id);
    this.children.set(parent, siblings);
    this.parents.set(id, parent);
  }

  private detach(id: string) {
    const parent = this.parents.get(id) ?? null;
    const siblings = this.children.get(parent) ?? [];
    siblings.splice(siblings.indexOf(id), 1);
  }

  apply(change: BlockChange_Deserialize): void {
    switch (change.op) {
      case "create": {
        if (this.blocks.has(change.id)) throw new Error(`block ${change.id} already exists`);
        this.checkParent(change.parent);
        this.blocks.set(change.id, { kind: change.kind, content: change.content as JsonObject });
        this.insert(change.id, change.parent ?? null, change.after ?? null);
        return;
      }
      case "update": {
        if (!this.blocks.has(change.id)) throw new Error(`block ${change.id} does not exist`);
        if (!mayHaveChildren(change.kind) && (this.children.get(change.id) ?? []).length > 0)
          throw new Error(`block ${change.id} has children; it cannot become ${change.kind}`);
        this.blocks.set(change.id, { kind: change.kind, content: change.content as JsonObject });
        return;
      }
      case "move": {
        if (!this.blocks.has(change.id)) throw new Error(`block ${change.id} does not exist`);
        const parent = change.parent ?? null;
        this.checkParent(parent);
        for (let above = parent; above !== null; above = this.parents.get(above) ?? null) {
          if (above === change.id) throw new Error("a block cannot move below itself");
        }
        this.detach(change.id);
        this.insert(change.id, parent, change.after ?? null);
        return;
      }
      case "delete": {
        if (!this.blocks.has(change.id)) throw new Error(`block ${change.id} does not exist`);
        this.detach(change.id);
        const drop = (id: string) => {
          for (const child of this.children.get(id) ?? []) drop(child);
          this.blocks.delete(id);
          this.children.delete(id);
          this.parents.delete(id);
        };
        drop(change.id);
        return;
      }
    }
  }

  tree(): BlockTree[] {
    const build = (parent: string | null): BlockTree[] =>
      (this.children.get(parent) ?? []).map((id) => {
        const block = this.blocks.get(id);
        if (!block) throw new Error(`lost block ${id}`);
        return { id, kind: block.kind, content: block.content, children: build(id) };
      });
    return build(null);
  }
}
