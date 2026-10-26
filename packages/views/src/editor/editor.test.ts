// SPDX-License-Identifier: AGPL-3.0-or-later
import { getSchema, type JSONContent } from "@tiptap/core";
import { EditorState } from "@tiptap/pm/state";
import fc from "fast-check";
import { describe, expect, it } from "vitest";
import { diffBlocks, jsonEqual, longestIncreasing } from "./diff";
import { assignIds } from "./ids";
import {
  blocksToDoc,
  docToBlocks,
  inlineToRuns,
  rowsToTree,
  runsToInline,
  type BlockTree,
} from "./mapping";
import { editorExtensions } from "./nodes";
import { FakeBlocks, ID_POOL, OBJECT_IDS, treeArb } from "./testing";

const schema = getSchema(editorExtensions());

/** The blocks of a document after a trip through ProseMirror's own JSON. */
function throughProseMirror(doc: JSONContent): BlockTree[] {
  const node = schema.nodeFromJSON(doc);
  node.check();
  return docToBlocks(node.toJSON() as JSONContent);
}

const [A, B, C, D, E] = ID_POOL as [string, string, string, string, string];
const paragraph = (id: string, text: string, children: BlockTree[] = []): BlockTree => ({
  id,
  kind: "paragraph",
  content: { inline: text ? [{ text }] : [] },
  children,
});
const bullet = (id: string, text: string, children: BlockTree[] = []): BlockTree => ({
  ...paragraph(id, text, children),
  kind: "bullet",
});

describe("runs", () => {
  it("carry marks, links, line breaks and mentions through the editor", () => {
    const runs = [
      { text: "Hello ", bold: true as const },
      { text: "line\nbreak", italic: true as const, link: "https://example.org/" },
      { mention: OBJECT_IDS[0] ?? "" },
      { text: "code", code: true as const },
    ];
    expect(inlineToRuns(runsToInline(runs))).toEqual(runs);
  });

  it("merge neighbours with the same marks and drop marks inline code excludes", () => {
    expect(
      inlineToRuns(
        runsToInline([{ text: "a" }, { text: "b" }, { text: "c", code: true, bold: true }]),
      ),
    ).toEqual([{ text: "ab" }, { text: "c", code: true }]);
  });
});

describe("the block mapping", () => {
  it("round-trips every valid block tree through ProseMirror unchanged", () => {
    fc.assert(
      fc.property(
        treeArb.filter((tree) => tree.length > 0),
        (tree) => {
          expect(throughProseMirror(blocksToDoc(tree))).toEqual(tree);
        },
      ),
      { numRuns: 400 },
    );
  });

  it("groups list items of one kind in a row into one list", () => {
    const doc = blocksToDoc([
      bullet(A, "one"),
      bullet(B, "two"),
      paragraph(C, "p"),
      bullet(D, "three"),
    ]);
    expect(doc.content?.map((node) => node.type)).toEqual([
      "bulletList",
      "paragraph",
      "bulletList",
    ]);
    expect(doc.content?.[0]?.content).toHaveLength(2);
  });

  it("keeps kinds it does not know, with their children, untouched", () => {
    const tree: BlockTree[] = [
      { id: A, kind: "kanban", content: { columns: ["To do"] }, children: [paragraph(B, "card")] },
    ];
    const doc = blocksToDoc(tree);
    expect(doc.content?.[0]?.type).toBe("unknownBlock");
    expect(throughProseMirror(doc)).toEqual(tree);
  });

  it("keeps content it cannot read as an unknown block", () => {
    const tree: BlockTree[] = [
      { id: A, kind: "heading", content: { inline: [], level: 9 }, children: [] },
    ];
    expect(blocksToDoc(tree).content?.[0]?.type).toBe("unknownBlock");
    expect(throughProseMirror(blocksToDoc(tree))).toEqual(tree);
  });

  it("orders stored rows by sort key, then ID", () => {
    const row = (id: string, parent: string | null, sort_key: string) => ({
      id,
      parent_block_id: parent,
      sort_key,
      kind: "bullet",
      content_json: { inline: [] },
    });
    const tree = rowsToTree([
      row(C, A, "a1"),
      row(B, null, "a1"),
      row(A, null, "a0"),
      row(D, A, "a0"),
    ]);
    expect(tree.map((block) => block.id)).toEqual([A, B]);
    expect(tree[0]?.children.map((block) => block.id)).toEqual([D, C]);
  });
});

describe("block IDs", () => {
  const stateOf = (doc: JSONContent) =>
    EditorState.create({ schema, doc: schema.nodeFromJSON(doc) });
  let next = 0;
  const makeId = () => `01JB3R7Q8Y5K3M2N4P6Q8RNEW${String((next += 1))}`;

  it("give new and duplicated blocks fresh IDs and leave the rest", () => {
    const doc: JSONContent = {
      type: "doc",
      content: [
        { type: "paragraph", attrs: { id: A } },
        { type: "paragraph", attrs: { id: A } },
        { type: "paragraph", attrs: { id: null } },
      ],
    };
    const state = stateOf(doc);
    const tr = state.tr;
    expect(assignIds(state.doc, tr, makeId)).toBe(true);
    const ids = docToBlocks(state.apply(tr).doc.toJSON() as JSONContent).map((block) => block.id);
    expect(ids[0]).toBe(A);
    expect(new Set(ids).size).toBe(3);
  });

  it("let a list item made from a paragraph keep the paragraph's block", () => {
    const doc: JSONContent = {
      type: "doc",
      content: [
        {
          type: "bulletList",
          content: [
            {
              type: "listItem",
              attrs: { id: null },
              content: [{ type: "paragraph", attrs: { id: A } }],
            },
          ],
        },
      ],
    };
    const state = stateOf(doc);
    const tr = state.tr;
    assignIds(state.doc, tr, makeId);
    expect(docToBlocks(state.apply(tr).doc.toJSON() as JSONContent)[0]?.id).toBe(A);
  });

  it("change nothing in a document that is already right", () => {
    const state = stateOf(blocksToDoc([bullet(A, "x", [paragraph(B, "y")]), paragraph(C, "z")]));
    expect(assignIds(state.doc, state.tr, makeId)).toBe(false);
  });
});

describe("the block diff", () => {
  const apply = (before: BlockTree[], after: BlockTree[]) => {
    const store = new FakeBlocks(before);
    for (const change of diffBlocks(before, after)) store.apply(change);
    return store.tree();
  };

  it("finds no changes between equal trees", () => {
    fc.assert(
      fc.property(treeArb, (tree) => {
        expect(diffBlocks(tree, structuredClone(tree))).toEqual([]);
      }),
    );
  });

  it("turns any tree into any other through changes the vault accepts", () => {
    fc.assert(
      fc.property(treeArb, treeArb, (before, after) => {
        expect(apply(before, after)).toEqual(after);
      }),
      { numRuns: 1000 },
    );
  });

  it("moves only the blocks that left their order", () => {
    const before = [paragraph(A, "a"), paragraph(B, "b"), paragraph(C, "c"), paragraph(D, "d")];
    const after = [paragraph(B, "b"), paragraph(C, "c"), paragraph(D, "d"), paragraph(A, "a")];
    expect(diffBlocks(before, after)).toEqual([{ op: "move", id: A, parent: null, after: D }]);
  });

  it("indents and outdents by moving under the previous item", () => {
    const flat = [bullet(A, "a"), bullet(B, "b")];
    const nested = [bullet(A, "a", [bullet(B, "b")])];
    expect(diffBlocks(flat, nested)).toEqual([{ op: "move", id: B, parent: A, after: null }]);
    expect(apply(nested, flat)).toEqual(flat);
  });

  it("deletes children before their parent stops being a list item", () => {
    const before = [bullet(A, "a", [paragraph(B, "b"), paragraph(C, "c")])];
    const after = [paragraph(A, "a"), paragraph(C, "c")];
    const changes = diffBlocks(before, after);
    expect(changes.map((change) => change.op)).toEqual(["move", "delete", "update"]);
    expect(apply(before, after)).toEqual(after);
  });

  it("deletes only the top-most of the blocks that are gone", () => {
    const before = [bullet(A, "a", [paragraph(B, "b")]), paragraph(E, "e")];
    expect(diffBlocks(before, [paragraph(E, "e")])).toEqual([{ op: "delete", id: A }]);
  });

  it("saves nothing for the empty paragraph a new document opens with", () => {
    expect(diffBlocks([], docToBlocks(blocksToDoc([])))).toEqual([]);
    expect(diffBlocks([], [paragraph(A, "typed")])).toHaveLength(1);
  });

  it("compares content ignoring key order", () => {
    expect(jsonEqual({ a: 1, b: [1, { c: 2 }] }, { b: [1, { c: 2 }], a: 1 })).toBe(true);
    expect(jsonEqual({ a: 1 }, { a: 1, b: undefined })).toBe(false);
  });

  it("finds a longest increasing run", () => {
    expect(longestIncreasing([3, 1, 2, 0, 4]).map((i) => [3, 1, 2, 0, 4][i])).toEqual([1, 2, 4]);
    expect(longestIncreasing([])).toEqual([]);
  });
});
