// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * The mind map's keys (ADR-0019). Arrows move between nodes: Left to the parent, Right to the first
 * child, Up and Down among siblings. Tab adds a child and Enter a sibling, as most mind map tools
 * do, so these differ from the Outline's. Shift+Tab is left to the browser, so the keyboard can
 * always leave the map.
 */
import type { Id, Position } from "@databaseme/client";
import { sibling, type MapLayout, type MapNode } from "./layout";

export type MapCommand =
  | { kind: "focus"; id: Id }
  | { kind: "expand"; id: Id }
  | { kind: "toggle" }
  | { kind: "add-child" }
  | { kind: "add-sibling" }
  | { kind: "rename" }
  | { kind: "reorder"; position: Position }
  | { kind: "trash" }
  | { kind: "open" }
  /** The key is the map's, but there is nowhere to go: don't let the browser have it. */
  | { kind: "stay" };

export interface KeyLike {
  key: string;
  altKey: boolean;
  ctrlKey: boolean;
  metaKey: boolean;
  shiftKey: boolean;
}

/** What a key does on `node`, or null if it isn't one of the map's keys. */
export function mapCommand(event: KeyLike, node: MapNode, layout: MapLayout): MapCommand | null {
  const ctrl = event.ctrlKey || event.metaKey;
  const focus = (id: Id | null | undefined): MapCommand =>
    id === null || id === undefined ? { kind: "stay" } : { kind: "focus", id };

  switch (event.key) {
    case "ArrowLeft":
      return focus(node.parent);
    case "ArrowRight":
      if (node.collapsed) return { kind: "expand", id: node.id };
      return focus(node.children[0]);
    case "ArrowUp":
    case "ArrowDown": {
      const up = event.key === "ArrowUp";
      const other = sibling(layout, node, up ? -1 : 1);
      if (!event.altKey) return focus(other);
      if (other === null) return { kind: "stay" };
      return { kind: "reorder", position: up ? { before: other } : { after: other } };
    }
    case "Tab":
      return event.shiftKey ? null : { kind: "add-child" };
    case "Enter":
      if (ctrl) return { kind: "open" };
      return node.parent === null ? { kind: "add-child" } : { kind: "add-sibling" };
    case "F2":
      return { kind: "rename" };
    case "Delete":
      return node.parent === null ? { kind: "stay" } : { kind: "trash" };
    case " ":
      return node.children.length > 0 ? { kind: "toggle" } : { kind: "stay" };
    default:
      return null;
  }
}
