// SPDX-License-Identifier: AGPL-3.0-or-later
/** The sidebar's tree as a flat list of visible rows, for a virtualised ARIA tree. */
import type { Id, ObjectSummary_Serialize } from "@databaseme/client";

export interface TreeRow {
  id: Id;
  summary: ObjectSummary_Serialize;
  parent: Id | null;
  level: number;
  /** Position among its siblings, from 1. */
  position: number;
  siblings: number;
  hasChildren: boolean;
  expanded: boolean;
}

/**
 * The visible rows: the top level, and the children of each expanded row whose children are
 * loaded. `children(null)` is the top level; `undefined` means not loaded yet.
 *
 * Each object is listed once, where it is first met. Right after a move, one parent's children
 * may already be read again while the old parent's are not, so for a moment an object can be in
 * both lists, or two objects can each seem to hold the other.
 */
export function flattenTree(
  children: (parent: Id | null) => readonly ObjectSummary_Serialize[] | undefined,
  expanded: ReadonlySet<Id>,
): TreeRow[] {
  const rows: TreeRow[] = [];
  const seen = new Set<Id>();
  const walk = (parent: Id | null, level: number) => {
    const list = children(parent) ?? [];
    list.forEach((summary, index) => {
      const id = summary.object.id;
      if (seen.has(id)) return;
      seen.add(id);
      const hasChildren = summary.child_count > 0;
      const open = hasChildren && expanded.has(id);
      rows.push({
        id,
        summary,
        parent,
        level,
        position: index + 1,
        siblings: list.length,
        hasChildren,
        expanded: open,
      });
      if (open) walk(id, level + 1);
    });
  };
  walk(null, 1);
  return rows;
}

/** Whether `id` is `ancestor` or below it, going by the visible rows. */
export function isWithin(rows: readonly TreeRow[], id: Id, ancestor: Id): boolean {
  const parentOf = new Map(rows.map((row) => [row.id, row.parent]));
  for (let at: Id | null | undefined = id; at != null; at = parentOf.get(at)) {
    if (at === ancestor) return true;
  }
  return false;
}

/** The expanded rows below `id`, to forget when it collapses. */
export function expandedBelow(rows: readonly TreeRow[], id: Id): Id[] {
  return rows
    .filter((row) => row.expanded && row.id !== id && isWithin(rows, row.id, id))
    .map((row) => row.id);
}

/** Where a drop at height `y` within a row of `top` and `height` puts the dragged object. */
export function dropZone(y: number, top: number, height: number): "before" | "inside" | "after" {
  const share = (y - top) / height;
  if (share < 0.25) return "before";
  if (share > 0.75) return "after";
  return "inside";
}
