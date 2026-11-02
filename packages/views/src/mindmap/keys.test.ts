// SPDX-License-Identifier: AGPL-3.0-or-later
import type { Id, TreeNode_Serialize } from "@databaseme/client";
import { describe, expect, it } from "vitest";
import { mapCommand, type KeyLike } from "./keys";
import { layoutMap } from "./layout";

function topic(id: Id, parent: Id | null): TreeNode_Serialize {
  return {
    id,
    parent_id: parent,
    title: id,
    type_ids: [],
    child_count: 0,
    outside_relations: 0,
  };
}

/** The root has first, middle (with kid below) and last (collapsed, with hidden below). */
const layout = layoutMap(
  [
    topic("root", "vault"),
    topic("first", "root"),
    topic("middle", "root"),
    topic("kid", "middle"),
    topic("last", "root"),
    topic("hidden", "last"),
  ],
  new Set(["last"]),
);

function press(key: string, on: Id, modifiers: Partial<KeyLike> = {}) {
  const node = layout.byId.get(on);
  if (!node) throw new Error(`${on} isn't on the map`);
  const event = { key, altKey: false, ctrlKey: false, metaKey: false, shiftKey: false };
  return mapCommand({ ...event, ...modifiers }, node, layout);
}

describe("mapCommand", () => {
  describe("arrows", () => {
    it("Left goes to the parent, and the root has nowhere to go", () => {
      expect(press("ArrowLeft", "kid")).toEqual({ kind: "focus", id: "middle" });
      expect(press("ArrowLeft", "root")).toEqual({ kind: "stay" });
    });

    it("Right goes to the first child, and a leaf has nowhere to go", () => {
      expect(press("ArrowRight", "root")).toEqual({ kind: "focus", id: "first" });
      expect(press("ArrowRight", "first")).toEqual({ kind: "stay" });
    });

    it("Right opens a collapsed node instead of going into it", () => {
      expect(press("ArrowRight", "last")).toEqual({ kind: "expand", id: "last" });
    });

    it("Up and Down go among siblings, and stop at the ends", () => {
      expect(press("ArrowUp", "middle")).toEqual({ kind: "focus", id: "first" });
      expect(press("ArrowDown", "middle")).toEqual({ kind: "focus", id: "last" });
      expect(press("ArrowUp", "first")).toEqual({ kind: "stay" });
      expect(press("ArrowDown", "last")).toEqual({ kind: "stay" });
      expect(press("ArrowDown", "root")).toEqual({ kind: "stay" });
    });
  });

  describe("reordering", () => {
    it("Alt+Up goes before the sibling above", () => {
      expect(press("ArrowUp", "middle", { altKey: true })).toEqual({
        kind: "reorder",
        position: { before: "first" },
      });
    });

    it("Alt+Down goes after the sibling below", () => {
      expect(press("ArrowDown", "middle", { altKey: true })).toEqual({
        kind: "reorder",
        position: { after: "last" },
      });
    });

    it("stays at the ends of the family", () => {
      expect(press("ArrowUp", "first", { altKey: true })).toEqual({ kind: "stay" });
      expect(press("ArrowDown", "last", { altKey: true })).toEqual({ kind: "stay" });
      expect(press("ArrowUp", "root", { altKey: true })).toEqual({ kind: "stay" });
    });
  });

  describe("adding", () => {
    it("Tab adds a child", () => {
      expect(press("Tab", "first")).toEqual({ kind: "add-child" });
    });

    it("Shift+Tab is left to the browser, so the keyboard can leave the map", () => {
      expect(press("Tab", "first", { shiftKey: true })).toBeNull();
    });

    it("Enter adds a sibling, or a child on the root, which can't have one", () => {
      expect(press("Enter", "first")).toEqual({ kind: "add-sibling" });
      expect(press("Enter", "root")).toEqual({ kind: "add-child" });
    });
  });

  it("Ctrl+Enter and Cmd+Enter open the node, the root too", () => {
    expect(press("Enter", "first", { ctrlKey: true })).toEqual({ kind: "open" });
    expect(press("Enter", "first", { metaKey: true })).toEqual({ kind: "open" });
    expect(press("Enter", "root", { ctrlKey: true })).toEqual({ kind: "open" });
  });

  it("F2 renames", () => {
    expect(press("F2", "first")).toEqual({ kind: "rename" });
  });

  it("Delete trashes, but never the root", () => {
    expect(press("Delete", "first")).toEqual({ kind: "trash" });
    expect(press("Delete", "root")).toEqual({ kind: "stay" });
  });

  it("Space folds a node with children, collapsed or not, and does nothing on a leaf", () => {
    expect(press(" ", "middle")).toEqual({ kind: "toggle" });
    expect(press(" ", "last")).toEqual({ kind: "toggle" });
    expect(press(" ", "first")).toEqual({ kind: "stay" });
  });

  it("leaves other keys alone", () => {
    expect(press("a", "first")).toBeNull();
    expect(press("Escape", "first")).toBeNull();
    expect(press("Home", "first")).toBeNull();
  });
});
