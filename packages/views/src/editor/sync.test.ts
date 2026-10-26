// SPDX-License-Identifier: AGPL-3.0-or-later
import type { BlockChange_Deserialize } from "@databaseme/client";
import type { JSONContent } from "@tiptap/core";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { blocksToDoc, docToBlocks, type BlockRowLike, type BlockTree } from "./mapping";
import { EditorSync, IDLE_MS, MAX_WAIT_MS } from "./sync";
import { FakeBlocks, ID_POOL } from "./testing";

const [A, B, C] = ID_POOL as [string, string, string];
const para = (id: string, text: string): BlockTree => ({
  id,
  kind: "paragraph",
  content: { inline: text ? [{ text }] : [] },
  children: [],
});

function rowsOf(tree: readonly BlockTree[]): BlockRowLike[] {
  const rows: BlockRowLike[] = [];
  const walk = (parent: string | null, blocks: readonly BlockTree[]) => {
    blocks.forEach((block, i) => {
      rows.push({
        id: block.id,
        parent_block_id: parent,
        sort_key: `a${String(i).padStart(4, "0")}`,
        kind: block.kind,
        content_json: block.content,
      });
      walk(block.id, block.children);
    });
  };
  walk(null, tree);
  return rows;
}

/** An editor holding a document, a vault holding blocks, and the sync between them. */
function setup(initial: BlockTree[]) {
  const store = new FakeBlocks(initial);
  let doc: JSONContent = blocksToDoc(initial);
  const shown: BlockTree[][] = [];
  const applied: BlockChange_Deserialize[][] = [];
  const errors: unknown[] = [];
  let failNext = 0;
  const vault = {
    load: vi.fn(() => Promise.resolve(rowsOf(store.tree()))),
    apply: vi.fn((changes: BlockChange_Deserialize[]) => {
      if (failNext > 0) {
        failNext -= 1;
        return Promise.reject(new Error("the vault changed"));
      }
      // The vault applies a batch whole or not at all.
      const trial = new FakeBlocks(store.tree());
      for (const change of changes) trial.apply(change);
      for (const change of changes) store.apply(change);
      applied.push(changes);
      return Promise.resolve();
    }),
  };
  const editor = {
    getJSON: () => doc,
    show: (next: JSONContent) => {
      doc = next;
      shown.push(docToBlocks(next));
    },
  };
  const sync = new EditorSync(editor, vault, rowsOf(initial), (error) => errors.push(error));
  return {
    store,
    sync,
    vault,
    shown,
    applied,
    errors,
    type(tree: BlockTree[]) {
      doc = blocksToDoc(tree);
      sync.changed();
    },
    failNext(count: number) {
      failNext = count;
    },
  };
}

beforeEach(() => {
  vi.useFakeTimers();
});
afterEach(() => {
  vi.useRealTimers();
});

describe("editor sync", () => {
  it("saves typing in one batch once the typist pauses", async () => {
    const t = setup([para(A, "a")]);
    t.type([para(A, "ab")]);
    t.type([para(A, "abc")]);
    await vi.advanceTimersByTimeAsync(IDLE_MS - 1);
    expect(t.applied).toHaveLength(0);
    await vi.advanceTimersByTimeAsync(1);
    expect(t.applied).toEqual([
      [{ op: "update", id: A, kind: "paragraph", content: { inline: [{ text: "abc" }] } }],
    ]);
    expect(t.store.tree()).toEqual([para(A, "abc")]);
  });

  it("saves at least every two seconds while typing goes on", async () => {
    const t = setup([para(A, "")]);
    for (let i = 1; i <= 10; i++) {
      t.type([para(A, "x".repeat(i))]);
      await vi.advanceTimersByTimeAsync(MAX_WAIT_MS / 8);
    }
    expect(t.applied.length).toBeGreaterThanOrEqual(1);
  });

  it("ignores the echo of its own save", async () => {
    const t = setup([para(A, "a")]);
    t.type([para(A, "b")]);
    await t.sync.flush();
    await t.sync.reconcile();
    expect(t.shown).toHaveLength(0);
  });

  it("shows a change made elsewhere", async () => {
    const t = setup([para(A, "a")]);
    t.store.apply({
      op: "create",
      id: B,
      parent: null,
      after: A,
      kind: "paragraph",
      content: { inline: [] },
    });
    await t.sync.reconcile();
    expect(t.shown).toEqual([[para(A, "a"), para(B, "")]]);
  });

  it("saves unsaved typing before showing a change made elsewhere", async () => {
    const t = setup([para(A, "a"), para(B, "b")]);
    t.type([para(A, "typed"), para(B, "b")]);
    t.store.apply({
      op: "update",
      id: B,
      kind: "paragraph",
      content: { inline: [{ text: "elsewhere" }] },
    });
    await t.sync.reconcile();
    expect(t.store.tree()).toEqual([para(A, "typed"), para(B, "elsewhere")]);
    expect(t.shown.at(-1)).toEqual([para(A, "typed"), para(B, "elsewhere")]);
  });

  it("retries a failed save against what the vault holds", async () => {
    const t = setup([para(A, "a")]);
    t.type([para(A, "a"), para(C, "new")]);
    t.failNext(1);
    await t.sync.flush();
    expect(t.errors).toEqual([]);
    expect(t.store.tree()).toEqual([para(A, "a"), para(C, "new")]);
  });

  it("reloads and reports when the retry fails too", async () => {
    const t = setup([para(A, "a")]);
    t.type([para(A, "lost")]);
    t.failNext(2);
    await t.sync.flush();
    expect(t.errors).toHaveLength(1);
    expect(t.shown.at(-1)).toEqual([para(A, "a")]);
  });

  it("does not save the empty paragraph of a new document", async () => {
    const t = setup([]);
    await t.sync.flush();
    await t.sync.reconcile();
    expect(t.applied).toEqual([]);
    expect(t.shown).toEqual([]);
  });
});
