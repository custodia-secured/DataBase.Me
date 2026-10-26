// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * Block IDs in the editor (ADR-0016). Every block node carries the ID of its block, made in the UI
 * the moment the block exists. After each change, a plugin gives an ID to every block node that has
 * none or whose ID an earlier node already has (a split, a paste, a duplicate).
 *
 * The first paragraph of a list item holds the item's text and mirrors the item's ID. Wrapping a
 * paragraph in a list therefore keeps its block (only its kind changes), and so does lifting the
 * item back out.
 */
import { Extension } from "@tiptap/core";
import type { Node as PmNode } from "@tiptap/pm/model";
import { Plugin, PluginKey, type Transaction } from "@tiptap/pm/state";
import { ReplaceStep } from "@tiptap/pm/transform";
import { newId } from "@databaseme/client";
import { BLOCK_NODES } from "./mapping";

const BLOCKS = new Set<string>(BLOCK_NODES);
const ITEMS = new Set(["listItem", "taskItem"]);

/**
 * Gives every block node in `doc` a unique ID, recording the changes in `tr`. Returns whether it
 * changed anything.
 */
export function assignIds(doc: PmNode, tr: Transaction, makeId: () => string = newId): boolean {
  const seen = new Set<string>();
  let changed = false;
  const setAttrs = (pos: number, node: PmNode, attrs: Record<string, unknown>) => {
    tr.setNodeMarkup(pos, undefined, { ...node.attrs, ...attrs });
    changed = true;
  };

  doc.descendants((node, pos, parent, index) => {
    const type = node.type.name;
    if (!BLOCKS.has(type)) return !node.isTextblock;
    // An item's first paragraph is handled with its item.
    if (type === "paragraph" && parent && ITEMS.has(parent.type.name) && index === 0) return false;

    let id = node.attrs.id as string | null;
    if (id === null || seen.has(id)) {
      const first = node.firstChild;
      const candidate =
        ITEMS.has(type) && first?.type.name === "paragraph"
          ? (first.attrs.id as string | null)
          : null;
      id = candidate !== null && !seen.has(candidate) ? candidate : makeId();
      const attrs: Record<string, unknown> = { id };
      // A copy of an unknown block cannot copy the blocks below it: their IDs are taken.
      if (type === "unknownBlock" && node.attrs.id !== null) attrs.children = [];
      setAttrs(pos, node, attrs);
    }
    seen.add(id);

    if (ITEMS.has(type)) {
      const first = node.firstChild;
      if (first?.type.name === "paragraph" && first.attrs.id !== id) {
        setAttrs(pos + 1, first, { id });
      }
    }
    return !node.isTextblock && !node.isAtom;
  });
  return changed;
}

/**
 * Whether a transaction can have added, split or duplicated block nodes. Typing inside one
 * text block cannot, and is skipped so a keystroke never walks the whole document.
 */
export function mayChangeBlocks(tr: Transaction): boolean {
  if (!tr.docChanged) return false;
  return tr.steps.some((step, index) => {
    if (!(step instanceof ReplaceStep)) return true;
    const doc = tr.docs[index];
    if (!doc) return true;
    const $from = doc.resolve(step.from);
    const $to = doc.resolve(step.to);
    if (!$from.sameParent($to) || !$from.parent.isTextblock) return true;
    let blocks = false;
    step.slice.content.forEach((node) => {
      if (!node.isInline) blocks = true;
    });
    return blocks;
  });
}

const key = new PluginKey("blockIds");

export const BlockIds = Extension.create({
  name: "blockIds",

  addGlobalAttributes() {
    return [
      {
        types: [...BLOCK_NODES],
        attributes: {
          // IDs are never read from HTML: pasted blocks are new blocks.
          id: { default: null, keepOnSplit: false, rendered: false, parseHTML: () => null },
          extra: { default: null, keepOnSplit: false, rendered: false, parseHTML: () => null },
        },
      },
    ];
  },

  addProseMirrorPlugins() {
    return [
      new Plugin({
        key,
        appendTransaction(transactions, _old, state) {
          if (!transactions.some(mayChangeBlocks)) return null;
          const tr = state.tr;
          if (!assignIds(state.doc, tr)) return null;
          return tr.setMeta("addToHistory", false);
        },
      }),
    ];
  },
});
