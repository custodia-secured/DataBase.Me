// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * A canvas's elements and the store's rows (M8, Task 12, ADR-0035), with no drawing library in it: one row
 * an element, what changed between the scene and the rows, and how a card is made and moved, which the list
 * does with no scene loaded at all.
 */
import type { CanvasItem_Deserialize, CanvasItemRow_Serialize, Id } from "@databaseme/client";

/** The parts of a drawing library's element this reads. The rest travels in `style_json`. */
export interface SceneElement {
  id: string;
  type: string;
  x: number;
  y: number;
  width: number;
  height: number;
  index?: string | null;
  version: number;
  versionNonce: number;
  isDeleted?: boolean;
  customData?: Record<string, unknown> | null;
  text?: string;
  [key: string]: unknown;
}

export const CARD_WIDTH = 240;
export const CARD_HEIGHT = 88;
/** The start of a stack, in the fractional order the library uses. */
const BOTTOM = "a0";
/** How far apart the list and "Add card" leave things. */
const GAP = 40;

/** The object a card shows, if this element is a card. */
export function objectOf(element: Pick<SceneElement, "customData">): Id | null {
  const id = element.customData?.object_id;
  return typeof id === "string" ? id : null;
}

/**
 * Whether a canvas keeps this element. A picture needs its bytes kept as well, which comes later (ADR-0035),
 * and a web page in a frame is a request. A card is the one embedded thing that stays.
 */
export function isKept(element: Pick<SceneElement, "type" | "customData">): boolean {
  if (element.type === "image" || element.type === "iframe") return false;
  return element.type !== "embeddable" || objectOf(element) !== null;
}

export function itemOf(element: SceneElement): CanvasItem_Deserialize {
  const { id, x, y, width, height, index, ...style } = element;
  return {
    item_id: id,
    object_id: objectOf(element),
    x,
    y,
    w: width,
    h: height,
    z: index ?? BOTTOM,
    style_json: style as CanvasItem_Deserialize["style_json"],
  };
}

export function elementOf(row: CanvasItemRow_Serialize): SceneElement {
  return {
    ...(row.style_json as unknown as Pick<SceneElement, "type" | "version" | "versionNonce">),
    id: row.item_id,
    x: row.x ?? 0,
    y: row.y ?? 0,
    width: row.w ?? 0,
    height: row.h ?? 0,
    index: row.z,
  };
}

const signatureOf = (element: Pick<SceneElement, "version" | "versionNonce">) =>
  `${String(element.version)}:${String(element.versionNonce)}`;

/** What the store holds, as each element's version. */
export function signaturesOf(rows: readonly CanvasItemRow_Serialize[]): Map<string, string> {
  return new Map(rows.map((row) => [row.item_id, signatureOf(elementOf(row))]));
}

export interface CanvasChanges {
  upsert: CanvasItem_Deserialize[];
  remove: string[];
}

/**
 * What has to be written so the store holds `elements`, which may include deleted ones. Only an element
 * marked as deleted is removed. Excalidraw marks what a person deletes, and an element that is simply not
 * there is no such thing: a scene being torn down has none at all, and that must not empty the canvas.
 */
export function changesBetween(
  stored: ReadonlyMap<string, string>,
  elements: readonly SceneElement[],
): CanvasChanges {
  const upsert: CanvasItem_Deserialize[] = [];
  const remove: string[] = [];
  for (const element of elements) {
    if (!isKept(element)) continue;
    if (element.isDeleted === true) {
      if (stored.has(element.id)) remove.push(element.id);
    } else if (stored.get(element.id) !== signatureOf(element)) {
      upsert.push(itemOf(element));
    }
  }
  return { upsert, remove };
}

/** What the store holds once these changes are written. */
export function afterChanges(
  stored: ReadonlyMap<string, string>,
  { upsert, remove }: CanvasChanges,
): Map<string, string> {
  const next = new Map(stored);
  for (const id of remove) next.delete(id);
  for (const item of upsert) {
    next.set(item.item_id, signatureOf(item.style_json as unknown as SceneElement));
  }
  return next;
}

/** Whether the scene already shows what the rows hold. */
export function sameScene(
  elements: readonly SceneElement[],
  rows: readonly CanvasItemRow_Serialize[],
): boolean {
  const shown = elements.filter((element) => element.isDeleted !== true && isKept(element));
  if (shown.length !== rows.length) return false;
  const stored = signaturesOf(rows);
  return shown.every((element) => stored.get(element.id) === signatureOf(element));
}

const whole = (random: () => number) => Math.floor(random() * 2 ** 31);

/** Above everything there. Adding to the end of a fractional index always sorts after it. */
export function zAbove(rows: readonly CanvasItemRow_Serialize[]): string {
  const top = rows.at(-1)?.z;
  return top === undefined ? BOTTOM : `${top}V`;
}

/** Below whatever is lowest on the canvas, or at the origin of an empty one. */
export function freePlace(rows: readonly CanvasItemRow_Serialize[]): { x: number; y: number } {
  if (rows.length === 0) return { x: 0, y: 0 };
  const left = Math.min(...rows.map((row) => row.x ?? 0));
  const bottom = Math.max(...rows.map((row) => (row.y ?? 0) + (row.h ?? 0)));
  return { x: left, y: bottom + GAP };
}

/** A card, as a row: the library fills in anything it wants beyond these as it reads it. */
export function newCard(
  objectId: Id,
  rows: readonly CanvasItemRow_Serialize[],
  random: () => number = Math.random,
): CanvasItem_Deserialize {
  const item = `card-${whole(random).toString(36)}${whole(random).toString(36)}`;
  const { x, y } = freePlace(rows);
  return {
    item_id: item,
    object_id: objectId,
    x,
    y,
    w: CARD_WIDTH,
    h: CARD_HEIGHT,
    z: zAbove(rows),
    style_json: {
      type: "embeddable",
      // What a library needs to call this an embed. It is never loaded: the card is drawn by the app.
      link: `dbme:${objectId}`,
      customData: { object_id: objectId },
      angle: 0,
      strokeColor: "#1e1e1e",
      backgroundColor: "transparent",
      fillStyle: "solid",
      strokeWidth: 1,
      strokeStyle: "solid",
      roughness: 0,
      opacity: 100,
      groupIds: [],
      frameId: null,
      roundness: { type: 3 },
      seed: whole(random),
      version: 1,
      versionNonce: whole(random),
      isDeleted: false,
      boundElements: null,
      updated: 0,
      locked: false,
    },
  };
}

/** A row moved by so much, as a new version of its element, so that a scene takes it in. */
export function moved(
  row: CanvasItemRow_Serialize,
  dx: number,
  dy: number,
  random: () => number = Math.random,
): CanvasItem_Deserialize {
  const element = elementOf(row);
  return itemOf({
    ...element,
    x: element.x + dx,
    y: element.y + dy,
    version: element.version + 1,
    versionNonce: whole(random),
  });
}

/** What kind of thing an element is, in words. */
export function kindOf(row: CanvasItemRow_Serialize): string {
  if (row.object_id !== null) return "Card";
  const { type } = elementOf(row);
  return type.charAt(0).toUpperCase() + type.slice(1);
}

/** What the list calls an element: a card's title, what a text says, or else its kind. */
export function nameOf(row: CanvasItemRow_Serialize, titles: ReadonlyMap<Id, string>): string {
  if (row.object_id !== null) return titles.get(row.object_id) ?? "A page";
  const { text } = elementOf(row);
  return typeof text === "string" && text.trim() !== "" ? text.trim() : kindOf(row);
}

export const placeLabel = (row: CanvasItemRow_Serialize) =>
  `${String(Math.round(row.x ?? 0))}, ${String(Math.round(row.y ?? 0))}`;

export const sizeLabel = (row: CanvasItemRow_Serialize) =>
  `${String(Math.round(row.w ?? 0))} by ${String(Math.round(row.h ?? 0))}`;

const plural = (count: number, one: string, many: string) =>
  `${String(count)} ${count === 1 ? one : many}`;

/** The canvas in words. */
export function canvasSummary(rows: readonly CanvasItemRow_Serialize[]): string {
  const cards = rows.filter((row) => row.object_id !== null).length;
  const drawn = rows.length - cards;
  return `${plural(cards, "card", "cards")} and ${plural(drawn, "drawn element", "drawn elements")}.`;
}
