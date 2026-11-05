// SPDX-License-Identifier: AGPL-3.0-or-later
import type { CanvasItemRow_Serialize } from "@databaseme/client";
import { describe, expect, it } from "vitest";
import {
  canvasSummary,
  changesBetween,
  elementOf,
  freePlace,
  isKept,
  itemOf,
  kindOf,
  moved,
  nameOf,
  newCard,
  objectOf,
  placeLabel,
  sameScene,
  signaturesOf,
  sizeLabel,
  zAbove,
  type SceneElement,
} from "./scene";

function element(id: string, parts: Partial<SceneElement> = {}): SceneElement {
  return {
    id,
    type: "rectangle",
    x: 1,
    y: 2,
    width: 30,
    height: 40,
    index: "a1",
    version: 1,
    versionNonce: 7,
    ...parts,
  };
}

function row(from: SceneElement): CanvasItemRow_Serialize {
  const item = itemOf(from);
  return {
    id: `row-${from.id}`,
    view_id: "view",
    item_id: item.item_id,
    object_id: item.object_id ?? null,
    x: item.x,
    y: item.y,
    w: item.w,
    h: item.h,
    z: item.z,
    style_json: item.style_json,
    created_at: 0,
    updated_at: 0,
  };
}

describe("an element and its row", () => {
  it("come back the same, whatever else the element has", () => {
    const original = element("a", {
      points: [
        [0, 0],
        [1.5, 2.5],
      ],
      strokeColor: "#123",
      customData: { object_id: "page" },
    });
    const item = itemOf(original);
    expect(item.item_id).toBe("a");
    expect(item.object_id).toBe("page");
    expect([item.x, item.y, item.w, item.h, item.z]).toEqual([1, 2, 30, 40, "a1"]);
    expect(elementOf(row(original))).toEqual(original);
  });

  it("keeps what the columns hold out of the style", () => {
    const style = itemOf(element("a")).style_json as unknown as Record<string, unknown>;
    for (const column of ["id", "x", "y", "width", "height", "index"]) {
      expect(style).not.toHaveProperty(column);
    }
  });

  it("starts a new element at the bottom of the stack if it has no place yet", () => {
    expect(itemOf(element("a", { index: null })).z).toBe("a0");
  });

  it("tells a card by what it shows", () => {
    expect(objectOf(element("a"))).toBeNull();
    expect(objectOf(element("a", { customData: { object_id: 5 } }))).toBeNull();
    expect(objectOf(element("a", { customData: { object_id: "page" } }))).toBe("page");
  });
});

describe("what a canvas keeps", () => {
  it("keeps what is drawn and cards, and not pictures or web pages", () => {
    expect(isKept(element("a"))).toBe(true);
    expect(isKept(element("a", { type: "image" }))).toBe(false);
    expect(isKept(element("a", { type: "iframe" }))).toBe(false);
    expect(isKept(element("a", { type: "embeddable" }))).toBe(false);
    expect(isKept(element("a", { type: "embeddable", customData: { object_id: "p" } }))).toBe(true);
  });
});

describe("changesBetween", () => {
  const stored = signaturesOf([row(element("same")), row(element("older")), row(element("gone"))]);

  it("writes what is new or changed, removes what was deleted, and leaves the rest", () => {
    const { upsert, remove } = changesBetween(stored, [
      element("same"),
      element("older", { version: 2 }),
      element("gone", { isDeleted: true }),
      element("new"),
    ]);
    expect(upsert.map((item) => item.item_id)).toEqual(["older", "new"]);
    expect(remove).toEqual(["gone"]);
  });

  it("removes nothing because a scene is empty, as one being torn down is", () => {
    expect(changesBetween(stored, [])).toEqual({ upsert: [], remove: [] });
  });

  it("takes a new version by its nonce too", () => {
    const { upsert } = changesBetween(signaturesOf([row(element("a"))]), [
      element("a", { versionNonce: 8 }),
    ]);
    expect(upsert).toHaveLength(1);
  });

  it("removes a deleted element once, and forgets one that was never stored", () => {
    const { upsert, remove } = changesBetween(signaturesOf([row(element("a"))]), [
      element("a", { isDeleted: true }),
      element("never stored", { isDeleted: true }),
    ]);
    expect(upsert).toEqual([]);
    expect(remove).toEqual(["a"]);
  });

  it("leaves a picture out, and does not take it for a removal of anything", () => {
    const { upsert, remove } = changesBetween(new Map(), [element("p", { type: "image" })]);
    expect(upsert).toEqual([]);
    expect(remove).toEqual([]);
  });
});

describe("sameScene", () => {
  const rows = [row(element("a")), row(element("b"))];

  it("agrees when the scene shows what the rows hold, deleted and unkept aside", () => {
    expect(
      sameScene(
        [
          element("a"),
          element("b"),
          element("c", { isDeleted: true }),
          element("p", { type: "image" }),
        ],
        rows,
      ),
    ).toBe(true);
  });

  it("disagrees on a version, a missing element or an extra one", () => {
    expect(sameScene([element("a", { version: 2 }), element("b")], rows)).toBe(false);
    expect(sameScene([element("a")], rows)).toBe(false);
    expect(sameScene([element("a"), element("b"), element("c")], rows)).toBe(false);
    expect(sameScene([element("a"), element("b", { isDeleted: true })], rows)).toBe(false);
  });
});

describe("a card", () => {
  const random = () => 0.5;

  it("is an embed the store will take, showing its object", () => {
    const card = newCard("page", [], random);
    expect(card.object_id).toBe("page");
    expect(card.item_id).toMatch(/^[A-Za-z0-9_-]{1,64}$/);
    expect(card.z).toBe("a0");
    const style = card.style_json as unknown as Record<string, unknown>;
    expect(style.type).toBe("embeddable");
    expect(style.link).toBe("dbme:page");
  });

  it("goes below what is there, and above it in the stack", () => {
    const rows = [row(element("a", { x: 10, y: 20, height: 100, index: "a5" }))];
    expect(freePlace(rows)).toEqual({ x: 10, y: 160 });
    expect(zAbove(rows) > "a5").toBe(true);
    const card = newCard("page", rows, random);
    expect([card.x, card.y]).toEqual([10, 160]);
  });

  it("starts an empty canvas at its origin", () => {
    expect(freePlace([])).toEqual({ x: 0, y: 0 });
  });
});

describe("moved", () => {
  it("moves a row, as a new version a scene will take in", () => {
    const before = element("a");
    const after = moved(row(before), 20, -20, () => 0.25);
    expect([after.x, after.y]).toEqual([21, -18]);
    expect(
      changesBetween(signaturesOf([row(before)]), [elementOf(rowOf(after))]).upsert,
    ).toHaveLength(1);
    const style = after.style_json as unknown as Record<string, unknown>;
    expect(style.version).toBe(2);
    expect(style.versionNonce).not.toBe(before.versionNonce);
  });

  function rowOf(item: ReturnType<typeof itemOf>): CanvasItemRow_Serialize {
    return { ...row(element("x")), ...item, object_id: item.object_id ?? null };
  }
});

describe("what the list says", () => {
  const titles = new Map([["page", "Reykjavik"]]);

  it("names a card by its page, a text by what it says, and the rest by kind", () => {
    const card = row(element("c", { type: "embeddable", customData: { object_id: "page" } }));
    expect(nameOf(card, titles)).toBe("Reykjavik");
    expect(nameOf(card, new Map())).toBe("A page");
    expect(kindOf(card)).toBe("Card");
    const text = row(element("t", { type: "text", text: "  Hello  " }));
    expect(nameOf(text, titles)).toBe("Hello");
    expect(kindOf(text)).toBe("Text");
    expect(nameOf(row(element("r")), titles)).toBe("Rectangle");
    expect(nameOf(row(element("e", { type: "text", text: " " })), titles)).toBe("Text");
  });

  it("gives places and sizes in whole pixels", () => {
    const shape = row(element("a", { x: 10.6, y: -2.2, width: 30.4, height: 40 }));
    expect(placeLabel(shape)).toBe("11, -2");
    expect(sizeLabel(shape)).toBe("30 by 40");
  });

  it("says what is on the canvas in words", () => {
    expect(canvasSummary([])).toBe("0 cards and 0 drawn elements.");
    expect(
      canvasSummary([
        row(element("c", { type: "embeddable", customData: { object_id: "page" } })),
        row(element("a")),
        row(element("b")),
      ]),
    ).toBe("1 card and 2 drawn elements.");
  });
});
