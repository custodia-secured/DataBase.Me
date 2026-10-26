// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * The editor's mapping between an object's blocks and a ProseMirror document (ADR-0016). Both
 * directions are pure functions over JSON, so they can be property-tested without a browser.
 *
 * - Text blocks (paragraph, heading, quote) become text nodes; their `inline` runs become text
 *   with marks, hard breaks (a `\n` in a run) and mention nodes.
 * - List items (bullet, numbered, todo) become list items; consecutive items of one kind share a
 *   list node. An item's first paragraph holds its text and the blocks after it are its children.
 * - Code, divider and image blocks become their own nodes.
 * - A kind this version does not know, or content it cannot read, becomes an `unknownBlock` atom
 *   that carries the block and its children untouched.
 *
 * Content keys a known kind does not use are kept in the node's `extra` attribute and written back.
 * Run keys that are not marks are dropped when a block is written.
 */
import type { JSONContent } from "@tiptap/core";
import { newId, type Value } from "@databaseme/client";

export type JsonObject = Record<string, Value>;

/** A block and the blocks below it, in order. */
export interface BlockTree {
  id: string;
  kind: string;
  content: JsonObject;
  children: BlockTree[];
}

/** A text run, as stored in `inline` (FORMAT.md §3.5). */
export type Run =
  | {
      text: string;
      bold?: true;
      italic?: true;
      underline?: true;
      strike?: true;
      code?: true;
      link?: string;
    }
  | { mention: string };

export const MARKS = ["bold", "italic", "underline", "strike", "code"] as const;
type MarkName = (typeof MARKS)[number];

/** The ProseMirror node of each list item kind, and the list node that holds it. */
const LIST_KINDS = {
  bullet: { list: "bulletList", item: "listItem" },
  numbered: { list: "orderedList", item: "listItem" },
  todo: { list: "taskList", item: "taskItem" },
} as const;
type ListKind = keyof typeof LIST_KINDS;

/** The content keys each known kind uses. Other keys go to `extra`. */
const KNOWN_KEYS: Readonly<Record<string, readonly string[]>> = {
  paragraph: ["inline"],
  heading: ["inline", "level"],
  quote: ["inline"],
  bullet: ["inline"],
  numbered: ["inline"],
  todo: ["inline", "checked"],
  code: ["code", "language"],
  divider: [],
  image: ["blob", "alt"],
};

/** The node types that are blocks and carry a block ID. */
export const BLOCK_NODES = [
  "paragraph",
  "heading",
  "quote",
  "codeBlock",
  "horizontalRule",
  "image",
  "listItem",
  "taskItem",
  "unknownBlock",
] as const;

/** The node types that hold list items. */
export const LIST_NODES = ["bulletList", "orderedList", "taskList"] as const;

export function isObject(value: unknown): value is JsonObject {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isListKind(kind: string): kind is ListKind {
  return kind in LIST_KINDS;
}

/** Whether blocks of `kind` may have children: list items, and kinds this version does not know. */
export function mayHaveChildren(kind: string): boolean {
  return isListKind(kind) || !(kind in KNOWN_KEYS);
}

function extraOf(kind: string, content: JsonObject): JsonObject | null {
  const known = KNOWN_KEYS[kind] ?? [];
  const extra: JsonObject = {};
  let any = false;
  for (const [key, value] of Object.entries(content)) {
    if (!known.includes(key)) {
      extra[key] = value;
      any = true;
    }
  }
  return any ? extra : null;
}

function withExtra(extra: unknown, content: JsonObject): JsonObject {
  return isObject(extra) ? { ...extra, ...content } : content;
}

// --- Runs ---------------------------------------------------------------------------------------

function readRuns(value: Value | undefined): Run[] | null {
  if (!Array.isArray(value)) return null;
  const runs: Run[] = [];
  for (const run of value) {
    if (!isObject(run)) return null;
    if (typeof run.mention === "string" && run.text === undefined) {
      runs.push({ mention: run.mention });
    } else if (typeof run.text === "string" && run.text !== "" && run.mention === undefined) {
      const text: Run = { text: run.text };
      for (const mark of MARKS) if (run[mark] === true) text[mark] = true;
      if (typeof run.link === "string" && run.link !== "") text.link = run.link;
      runs.push(text);
    } else {
      return null;
    }
  }
  return runs;
}

/** The ProseMirror marks of a text run. Inline code excludes every other mark, as in the editor. */
function marksOf(run: Exclude<Run, { mention: string }>): JSONContent["marks"] {
  if (run.code) return [{ type: "code" }];
  const marks: NonNullable<JSONContent["marks"]> = [];
  for (const mark of MARKS) if (run[mark]) marks.push({ type: mark });
  if (run.link !== undefined) marks.push({ type: "link", attrs: { href: run.link } });
  return marks;
}

export function runsToInline(runs: readonly Run[]): JSONContent[] {
  const nodes: JSONContent[] = [];
  for (const run of runs) {
    if ("mention" in run) {
      nodes.push({ type: "mention", attrs: { id: run.mention } });
      continue;
    }
    const marks = marksOf(run);
    const withMarks = (node: JSONContent): JSONContent =>
      marks && marks.length > 0 ? { ...node, marks } : node;
    run.text.split("\n").forEach((part, index) => {
      if (index > 0) nodes.push(withMarks({ type: "hardBreak" }));
      if (part !== "") nodes.push(withMarks({ type: "text", text: part }));
    });
  }
  return nodes;
}

function sameMarks(a: Run, b: Run): boolean {
  if ("mention" in a || "mention" in b) return false;
  return MARKS.every((mark) => a[mark] === b[mark]) && a.link === b.link;
}

export function inlineToRuns(nodes: readonly JSONContent[] | undefined): Run[] {
  const runs: Run[] = [];
  for (const node of nodes ?? []) {
    let run: Run;
    if (node.type === "mention") {
      const id: unknown = node.attrs?.id;
      if (typeof id !== "string") continue;
      run = { mention: id };
    } else if (node.type === "text" || node.type === "hardBreak") {
      const text = node.type === "text" ? (node.text ?? "") : "\n";
      if (text === "") continue;
      const textRun: Run = { text };
      for (const mark of node.marks ?? []) {
        if (MARKS.includes(mark.type as MarkName)) textRun[mark.type as MarkName] = true;
        const href: unknown = mark.attrs?.href;
        if (mark.type === "link" && typeof href === "string" && href !== "") textRun.link = href;
      }
      run = textRun;
    } else {
      continue;
    }
    const last = runs.at(-1);
    if (last && "text" in last && "text" in run && sameMarks(last, run)) {
      last.text += run.text;
    } else {
      runs.push(run);
    }
  }
  return runs;
}

// --- Blocks to document ------------------------------------------------------------------------

function unknownNode(block: BlockTree): JSONContent {
  return {
    type: "unknownBlock",
    attrs: { id: block.id, kind: block.kind, content: block.content, children: block.children },
  };
}

function textNode(type: string, attrs: JsonObject, runs: readonly Run[]): JSONContent {
  const inline = runsToInline(runs);
  return inline.length > 0 ? { type, attrs, content: inline } : { type, attrs };
}

/** The node of one block that is not a list item, or null if it must be kept as unknown. */
function blockNode(block: BlockTree): JSONContent | null {
  const { id, kind, content } = block;
  const extra = extraOf(kind, content);
  switch (kind) {
    case "paragraph":
    case "quote": {
      const runs = readRuns(content.inline);
      return runs && textNode(kind, { id, extra }, runs);
    }
    case "heading": {
      const runs = readRuns(content.inline);
      const level = content.level;
      if (!runs || typeof level !== "number" || !Number.isInteger(level) || level < 1 || level > 6)
        return null;
      return textNode("heading", { id, level, extra }, runs);
    }
    case "code": {
      const code = content.code;
      const language = content.language ?? null;
      if (typeof code !== "string" || (language !== null && typeof language !== "string"))
        return null;
      const attrs = { id, language, extra };
      return code === ""
        ? { type: "codeBlock", attrs }
        : { type: "codeBlock", attrs, content: [{ type: "text", text: code }] };
    }
    case "divider":
      return { type: "horizontalRule", attrs: { id, extra } };
    case "image": {
      const blob = content.blob;
      const alt = content.alt ?? null;
      if (typeof blob !== "string" || (alt !== null && typeof alt !== "string")) return null;
      return { type: "image", attrs: { id, blob, alt, extra } };
    }
    default:
      return null;
  }
}

function itemNode(block: BlockTree & { kind: ListKind }): JSONContent | null {
  const runs = readRuns(block.content.inline);
  const checked = block.content.checked;
  if (!runs || (block.kind === "todo" && typeof checked !== "boolean")) return null;
  const attrs: JsonObject = { id: block.id, extra: extraOf(block.kind, block.content) };
  if (block.kind === "todo") attrs.checked = checked ?? false;
  // The first paragraph mirrors the item's ID (see ids.ts); its own ID is never read.
  const paragraph = textNode("paragraph", { id: block.id, extra: null }, runs);
  return {
    type: LIST_KINDS[block.kind].item,
    attrs,
    content: [paragraph, ...blocksToNodes(block.children)],
  };
}

/** The nodes of a list of sibling blocks: list items of one kind in a row share a list node. */
export function blocksToNodes(blocks: readonly BlockTree[]): JSONContent[] {
  const nodes: JSONContent[] = [];
  // The list node the previous item went into, and its kind.
  let listKind: ListKind | null = null;
  let items: JSONContent[] = [];
  for (const block of blocks) {
    const { kind } = block;
    if (isListKind(kind)) {
      const item = itemNode(block as BlockTree & { kind: ListKind });
      if (item) {
        if (listKind !== kind) {
          listKind = kind;
          items = [];
          nodes.push({ type: LIST_KINDS[kind].list, content: items });
        }
        items.push(item);
        continue;
      }
    }
    listKind = null;
    const node = blockNode(block);
    // Children of a block that is not a list item exist only under unknown kinds, kept whole.
    nodes.push(node && block.children.length === 0 ? node : unknownNode(block));
  }
  return nodes;
}

/** The editor document for an object's blocks. An object with no blocks gets one empty paragraph. */
export function blocksToDoc(blocks: readonly BlockTree[]): JSONContent {
  const content = blocksToNodes(blocks);
  if (content.length === 0)
    content.push({ type: "paragraph", attrs: { id: newId(), extra: null } });
  return { type: "doc", content };
}

// --- Document to blocks ------------------------------------------------------------------------

/** A node that should carry a block ID has none. The ID plugin prevents this; it is a bug. */
export class MissingIdError extends Error {
  constructor(type: string | undefined) {
    super(`a ${type ?? "block"} node has no block ID`);
    this.name = "MissingIdError";
  }
}

function idOf(node: JSONContent): string {
  const id: unknown = node.attrs?.id;
  if (typeof id !== "string" || id === "") throw new MissingIdError(node.type);
  return id;
}

function treeOf(value: unknown): BlockTree[] {
  return Array.isArray(value) ? (value as BlockTree[]) : [];
}

function nodeToBlock(node: JSONContent): BlockTree | null {
  const attrs = node.attrs ?? {};
  const make = (kind: string, content: JsonObject): BlockTree => ({
    id: idOf(node),
    kind,
    content: withExtra(attrs.extra, content),
    children: [],
  });
  switch (node.type) {
    case "paragraph":
    case "quote":
      return make(node.type, { inline: inlineToRuns(node.content) });
    case "heading":
      return make("heading", {
        inline: inlineToRuns(node.content),
        level: Number(attrs.level ?? 1),
      });
    case "codeBlock": {
      const content: JsonObject = {
        code: (node.content ?? []).map((text) => text.text ?? "").join(""),
      };
      if (typeof attrs.language === "string" && attrs.language !== "")
        content.language = attrs.language;
      return make("code", content);
    }
    case "horizontalRule":
      return make("divider", {});
    case "image": {
      const content: JsonObject = { blob: String(attrs.blob) };
      if (typeof attrs.alt === "string") content.alt = attrs.alt;
      return make("image", content);
    }
    case "unknownBlock":
      return {
        id: idOf(node),
        kind: String(attrs.kind),
        content: isObject(attrs.content) ? attrs.content : {},
        children: treeOf(attrs.children),
      };
    default:
      return null;
  }
}

function itemToBlock(item: JSONContent, kind: ListKind): BlockTree {
  const [first, ...rest] = item.content ?? [];
  const hasText = first?.type === "paragraph";
  const content: JsonObject = { inline: hasText ? inlineToRuns(first.content) : [] };
  if (kind === "todo") content.checked = item.attrs?.checked === true;
  return {
    id: idOf(item),
    kind,
    content: withExtra(item.attrs?.extra, content),
    children: nodesToBlocks(hasText ? rest : (item.content ?? [])),
  };
}

function listKindOf(list: string | undefined): ListKind | null {
  if (list === "bulletList") return "bullet";
  if (list === "orderedList") return "numbered";
  if (list === "taskList") return "todo";
  return null;
}

export function nodesToBlocks(nodes: readonly JSONContent[]): BlockTree[] {
  const blocks: BlockTree[] = [];
  for (const node of nodes) {
    const listKind = listKindOf(node.type);
    if (listKind) {
      for (const item of node.content ?? []) blocks.push(itemToBlock(item, listKind));
      continue;
    }
    const block = nodeToBlock(node);
    if (block) blocks.push(block);
  }
  return blocks;
}

/** The blocks of an editor document. */
export function docToBlocks(doc: JSONContent): BlockTree[] {
  return nodesToBlocks(doc.content ?? []);
}

// --- Rows ---------------------------------------------------------------------------------------

/** A stored block row, as `blocks_list` returns it. */
export interface BlockRowLike {
  id: string;
  parent_block_id: string | null;
  sort_key: string;
  kind: string;
  content_json: Value;
}

function compareKeys(a: BlockRowLike, b: BlockRowLike): number {
  if (a.sort_key !== b.sort_key) return a.sort_key < b.sort_key ? -1 : 1;
  return a.id < b.id ? -1 : a.id > b.id ? 1 : 0;
}

/** The block tree of an object's rows: siblings by sort key, then ID (FORMAT.md §3.5). */
export function rowsToTree(rows: readonly BlockRowLike[]): BlockTree[] {
  const byParent = new Map<string | null, BlockRowLike[]>();
  const ids = new Set(rows.map((row) => row.id));
  for (const row of rows) {
    // A row whose parent is missing would be invisible; show it at the top level instead.
    const parent =
      row.parent_block_id !== null && ids.has(row.parent_block_id) ? row.parent_block_id : null;
    const siblings = byParent.get(parent);
    if (siblings) siblings.push(row);
    else byParent.set(parent, [row]);
  }
  const build = (parent: string | null): BlockTree[] =>
    (byParent.get(parent) ?? []).sort(compareKeys).map((row) => ({
      id: row.id,
      kind: row.kind,
      content: isObject(row.content_json) ? row.content_json : {},
      children: build(row.id),
    }));
  return build(null);
}
