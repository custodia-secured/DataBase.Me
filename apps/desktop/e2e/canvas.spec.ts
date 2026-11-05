// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * The Canvas view (M8, Task 12, ADR-0035), under the real CSP: a drawing kept in the store, a stroke as one
 * undo, cards that follow their pages, and the list for whoever does not draw. The fixtures fail any test on a
 * CSP report, an uncaught error or a request leaving the machine, which is most of what is promised about
 * Excalidraw: that it needs nothing from anywhere.
 */
import type { Locator, Page } from "@playwright/test";
import { expect, expectAccessible, test, type App } from "./fixtures";

interface Handle {
  getSceneElements: () => readonly { id: string; type: string; x: number; y: number }[];
  getSceneElementsIncludingDeleted: () => readonly unknown[];
  updateScene: (scene: { elements: readonly unknown[]; captureUpdate?: string }) => void;
}

declare global {
  interface Window {
    /** Excalidraw's own handle, in the e2e build alone. */
    __dbmeCanvas?: Handle;
  }
}

function plan(): unknown {
  return {
    kind: "tree",
    objects: [
      {
        title: "Trip",
        children: [{ title: "Reykjavik" }, { title: "Nairobi" }],
      },
    ],
  };
}

type Ids = Record<string, string>;
const host = (app: App): Locator => app.pane().locator(".dbme-canvas__host");
const count = (app: App): Locator => app.pane().locator(".dbme-canvas__count");
const card = (app: App, title: string): Locator =>
  app.pane().locator(".dbme-canvas__card", { hasText: title });

async function open(app: App, seed: (plan: unknown) => Promise<{ ids: Ids }>): Promise<Ids> {
  const { ids } = await seed(plan());
  await app.open();
  await app.openPage("Trip");
  await app.view("Canvas");
  await expect(count(app)).toHaveText("0 cards and 0 drawn elements.");
  await ready(app.page);
  return ids;
}

/** Excalidraw has loaded and said what it can do. */
async function ready(page: Page): Promise<void> {
  await page.waitForFunction(() => window.__dbmeCanvas !== undefined);
  await expect(page.locator(".excalidraw canvas.interactive")).toBeVisible();
}

/** Everything an element has, as Excalidraw itself would write it. */
function rectangle(id: string, x: number, parts: Record<string, unknown> = {}): unknown {
  return {
    id,
    type: "rectangle",
    x,
    y: 40,
    width: 120,
    height: 80,
    angle: 0,
    strokeColor: "#1e1e1e",
    backgroundColor: "transparent",
    fillStyle: "solid",
    strokeWidth: 2,
    strokeStyle: "solid",
    roughness: 1,
    opacity: 100,
    groupIds: [],
    frameId: null,
    index: "a1",
    roundness: null,
    seed: 11,
    version: 2,
    versionNonce: 22,
    isDeleted: false,
    boundElements: null,
    updated: 1,
    link: null,
    locked: false,
    ...parts,
  };
}

/** Puts elements in the scene as a person's own change would. */
async function put(page: Page, ...elements: readonly unknown[]): Promise<void> {
  await page.evaluate((added) => {
    const api = window.__dbmeCanvas;
    if (!api) throw new Error("no canvas");
    api.updateScene({
      elements: [...api.getSceneElementsIncludingDeleted(), ...added],
      captureUpdate: "IMMEDIATELY",
    });
  }, elements);
}

/** Draws one rectangle: with the mouse where there is one, and through the handle on a phone. */
async function draw(app: App): Promise<void> {
  if (app.phone) {
    await put(app.page, rectangle("drawn", 40));
    return;
  }
  // The radio itself is under its icon, as a person finds it.
  await host(app)
    .locator("label", { has: app.page.getByTestId("toolbar-rectangle") })
    .click();
  const box = await host(app).boundingBox();
  if (!box) throw new Error("no stage");
  const { mouse } = app.page;
  await mouse.move(box.x + box.width / 2 - 80, box.y + box.height / 2 - 40);
  await mouse.down();
  await mouse.move(box.x + box.width / 2, box.y + box.height / 2, { steps: 5 });
  await mouse.move(box.x + box.width / 2 + 80, box.y + box.height / 2 + 40, { steps: 5 });
  await mouse.up();
}

const shown = (page: Page) =>
  page.evaluate(() => (window.__dbmeCanvas?.getSceneElements() ?? []).map((one) => one.type));

async function addCard(app: App, title: string): Promise<void> {
  await app.press(app.pane().getByRole("button", { name: "Add card" }));
  const dialog = app.page.getByRole("dialog");
  await dialog.getByLabel("Page").selectOption({ label: title });
  await app.press(dialog.getByRole("button", { name: "Add", exact: true }));
  await expect(card(app, title)).toBeVisible();
}

const undo = (app: App) => app.press(app.page.getByRole("button", { name: "Undo" }).first());
const redo = (app: App) => app.press(app.page.getByRole("button", { name: "Redo" }).first());

test("what is drawn stays, and a stroke is one undo", async ({ app, page, seed }) => {
  await open(app, seed);
  await draw(app);
  await expect(count(app)).toHaveText("0 cards and 1 drawn element.");

  await page.reload();
  await app.openPage("Trip");
  await app.view("Canvas");
  await expect(count(app)).toHaveText("0 cards and 1 drawn element.");
  await ready(page);
  await expect.poll(() => shown(page)).toEqual(["rectangle"]);

  await undo(app);
  await expect(count(app)).toHaveText("0 cards and 0 drawn elements.");
  await expect.poll(() => shown(page)).toEqual([]);
  await redo(app);
  await expect(count(app)).toHaveText("0 cards and 1 drawn element.");
  await expect.poll(() => shown(page)).toEqual(["rectangle"]);
});

test("opening a canvas changes nothing", async ({ app, page, seed }) => {
  await open(app, seed);
  await put(
    page,
    rectangle("box", 40),
    rectangle("words", 240, {
      type: "text",
      index: "a2",
      width: 60,
      height: 25,
      text: "Hello",
      originalText: "Hello",
      fontSize: 20,
      fontFamily: 5,
      textAlign: "left",
      verticalAlign: "top",
      containerId: null,
      autoResize: true,
      lineHeight: 1.25,
    }),
    rectangle("line", 400, {
      type: "arrow",
      index: "a3",
      points: [
        [0, 0],
        [100, 50],
      ],
      lastCommittedPoint: null,
      startBinding: null,
      endBinding: null,
      startArrowhead: null,
      endArrowhead: "arrow",
      elbowed: false,
    }),
  );
  await expect(count(app)).toHaveText("0 cards and 3 drawn elements.");

  await page.reload();
  await app.openPage("Trip");
  await app.view("Canvas");
  await ready(page);
  await expect.poll(() => shown(page)).toEqual(["rectangle", "text", "arrow"]);
  // Anything the library had rewritten would have gone in by now, and would be the last change.
  await page.waitForTimeout(800);
  await undo(app);
  await expect(count(app)).toHaveText("0 cards and 0 drawn elements.");
});

test("a card shows its page, and stays up to date with it", async ({ app, page, seed }) => {
  const ids = await open(app, seed);
  await addCard(app, "Reykjavik");
  await expect(count(app)).toHaveText("1 card and 0 drawn elements.");
  // A new card is chosen, so what can be done with it is right there.
  await expect(app.pane().locator(".dbme-canvas__selected")).toContainText("Reykjavik");

  await app.ipc("object_rename", { id: ids.Reykjavik, title: "Akureyri" });
  await page.reload();
  await app.openPage("Trip");
  await app.view("Canvas");
  await expect(card(app, "Akureyri")).toBeVisible();

  // Undo takes the name back, and the change arrives the way a real one does.
  await undo(app);
  await expect(card(app, "Reykjavik")).toBeVisible();
});

test("the card of a page in the Trash says so", async ({ app, page, seed }) => {
  const ids = await open(app, seed);
  await addCard(app, "Nairobi");
  await app.ipc("object_trash", { id: ids.Nairobi });
  await page.reload();
  await app.openPage("Trip");
  await app.view("Canvas");
  await expect(card(app, "In the Trash")).toBeVisible();
  await expect(count(app)).toHaveText("1 card and 0 drawn elements.");
});

test("a card that is chosen can be opened and taken off", async ({ app, seed }) => {
  await open(app, seed);
  await addCard(app, "Nairobi");
  await addCard(app, "Reykjavik");
  const chosen = app.pane().locator(".dbme-canvas__selected");
  await app.press(chosen.getByRole("button", { name: "Remove from canvas" }));
  await expect(count(app)).toHaveText("1 card and 0 drawn elements.");
  await expect(card(app, "Reykjavik")).toHaveCount(0);
  await expect(chosen).toHaveCount(0);
  await undo(app);
  await expect(card(app, "Reykjavik")).toBeVisible();
  await expect(count(app)).toHaveText("2 cards and 0 drawn elements.");
});

test("dragging a card moves it, as one undo", async ({ app, page, seed }, testInfo) => {
  test.skip(testInfo.project.name.includes("phone"), "drawn with a mouse");
  const ids = await open(app, seed);
  await addCard(app, "Reykjavik");
  const at = async () => {
    const { items } = await app.ipc<{ items: { x: number; y: number }[] }>("canvas_read", {
      owner: ids.Trip,
    });
    return items[0];
  };
  const before = await at();
  const box = await card(app, "Reykjavik").boundingBox();
  if (!box || !before) throw new Error("no card");
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await page.mouse.down();
  await page.mouse.move(box.x + box.width / 2 + 40, box.y + box.height / 2 + 30, { steps: 4 });
  await page.mouse.move(box.x + box.width / 2 + 80, box.y + box.height / 2 + 60, { steps: 4 });
  await page.mouse.up();
  await expect.poll(async () => (await at())?.x).not.toBe(before.x);
  const after = await at();
  expect((after?.y ?? 0) - before.y).toBeGreaterThan(0);

  await undo(app);
  await expect.poll(async () => (await at())?.x).toBe(before.x);
  // The scene goes back as well, and its card with it.
  await expect
    .poll(() =>
      page.evaluate(
        () => window.__dbmeCanvas?.getSceneElements().find((one) => one.type === "embeddable")?.x,
      ),
    )
    .toBe(before.x);
});

test("the app's Undo takes a stroke back from the keyboard, once", async ({
  app,
  page,
  seed,
}, testInfo) => {
  test.skip(testInfo.project.name.includes("phone"), "a keyboard");
  await open(app, seed);
  await draw(app);
  await expect(count(app)).toHaveText("0 cards and 1 drawn element.");
  await draw(app);
  await expect(count(app)).toHaveText("0 cards and 2 drawn elements.");
  await page.keyboard.press("ControlOrMeta+z");
  await expect(count(app)).toHaveText("0 cards and 1 drawn element.");
  await expect.poll(() => shown(page)).toEqual(["rectangle"]);
  // It stays there: a second undo would have taken it by now, whether the app's or Excalidraw's own.
  await page.waitForTimeout(500);
  await expect(count(app)).toHaveText("0 cards and 1 drawn element.");
  await page.keyboard.press("ControlOrMeta+y");
  await expect(count(app)).toHaveText("0 cards and 2 drawn elements.");
});

test("a stroke not yet written goes in before Undo acts", async ({ app, page, seed }) => {
  await open(app, seed);
  await draw(app);
  // No wait for the store: Undo is pressed with the change still in hand.
  await undo(app);
  await expect.poll(() => shown(page)).toEqual([]);
  await expect(count(app)).toHaveText("0 cards and 0 drawn elements.");
  await redo(app);
  await expect(count(app)).toHaveText("0 cards and 1 drawn element.");
});

test("a change undone before the store is read again still leaves the scene", async ({
  app,
  page,
  seed,
}) => {
  await open(app, seed);
  // The scene has long finished starting, as it has by the time a person draws anything.
  await page.waitForTimeout(800);
  // Slow reads, so that the one after the stroke is overtaken by the one after Undo. What the app sees is then
  // an empty canvas followed by an empty canvas: nothing has changed, and the scene still has the stroke.
  await page.route("**/ipc", async (route) => {
    const body = route.request().postDataJSON() as { cmd?: string } | null;
    if (body?.cmd === "canvas_read") {
      await new Promise((resolve) => setTimeout(resolve, 500));
    }
    await route.continue();
  });
  await draw(app);
  await undo(app);
  await expect.poll(() => shown(page), { timeout: 4000 }).toEqual([]);
  await expect(count(app)).toHaveText("0 cards and 0 drawn elements.");
});

test("a stroke stays on screen while the store is being read", async ({ app, page, seed }) => {
  await open(app, seed);
  await page.route("**/ipc", async (route) => {
    const body = route.request().postDataJSON() as { cmd?: string } | null;
    if (body?.cmd === "canvas_read") {
      await new Promise((resolve) => setTimeout(resolve, 900));
    }
    await route.continue();
  });
  // Straight away, so that what the scene was told as it started is still waiting: it is older than the stroke.
  await draw(app);
  const seen: number[] = [];
  for (let at = 0; at < 14; at += 1) {
    seen.push((await shown(page)).length);
    await page.waitForTimeout(100);
  }
  expect(seen.filter((length) => length !== 1)).toEqual([]);
});

test("leaving saves what is in hand, and takes nothing away", async ({ app, page, seed }) => {
  const ids = await open(app, seed);
  const stored = async () =>
    (await app.ipc<{ items: unknown[] }>("canvas_read", { owner: ids.Trip })).items.length;

  // Still in hand: no wait for the store before leaving.
  await draw(app);
  await app.view("Document");
  await expect.poll(stored).toBe(1);
  await app.view("Canvas");
  await expect(count(app)).toHaveText("0 cards and 1 drawn element.");
  await ready(page);
  await expect.poll(() => shown(page)).toEqual(["rectangle"]);

  // Already stored: Excalidraw empties its scene as it goes, and that is not a person deleting anything.
  await app.view("Document");
  await page.waitForTimeout(600);
  expect(await stored()).toBe(1);
  await app.view("Canvas");
  await ready(page);
  await expect.poll(() => shown(page)).toEqual(["rectangle"]);

  await app.press(app.pane().getByRole("checkbox", { name: "Show as list" }));
  await page.waitForTimeout(600);
  expect(await stored()).toBe(1);
});

test("the list has everything with where it is, and moves it", async ({ app, page, seed }) => {
  await open(app, seed);
  await addCard(app, "Reykjavik");
  await put(page, rectangle("box", 40, { index: "a9" }));
  await expect(count(app)).toHaveText("1 card and 1 drawn element.");

  await app.press(app.pane().getByRole("checkbox", { name: "Show as list" }));
  const grid = app.pane().getByRole("grid", { name: "On this canvas" });
  await expect(grid.getByRole("columnheader")).toHaveText(["Title", "Kind", "Position", "Size"]);
  const rows = grid.locator('[role="rowgroup"]').nth(1).getByRole("row");
  await expect(rows).toHaveCount(2);
  await expect(rows.nth(0).getByRole("gridcell")).toHaveText([
    "Reykjavik",
    "Card",
    "0, 0",
    "240 by 88",
  ]);
  await expect(rows.nth(1).getByRole("gridcell")).toHaveText([
    "Rectangle",
    "Rectangle",
    "40, 40",
    "120 by 80",
  ]);
  await expectAccessible(page, ".dbme-canvas");

  const chosen = app.pane().locator(".dbme-canvas__selected");
  await app.press(chosen.getByRole("button", { name: "Move right" }));
  await expect(rows.nth(0).getByRole("gridcell").nth(2)).toHaveText("20, 0");
  await expect(app.pane().getByRole("status").filter({ hasText: "moved to" })).toHaveText(
    "Reykjavik moved to 20, 0.",
  );
  await undo(app);
  await expect(rows.nth(0).getByRole("gridcell").nth(2)).toHaveText("0, 0");

  if (!app.phone) {
    await grid.focus();
    await page.keyboard.press("Alt+ArrowDown");
    await expect(rows.nth(0).getByRole("gridcell").nth(2)).toHaveText("0, 20");
    await page.keyboard.press("ArrowDown");
    await expect(chosen).toContainText("Rectangle");
    await page.keyboard.press("Alt+ArrowLeft");
    await expect(rows.nth(1).getByRole("gridcell").nth(2)).toHaveText("20, 40");
    await page.keyboard.press("ArrowUp");
    await page.keyboard.press("Enter");
    await expect(app.currentCrumb()).toHaveText("Reykjavik");
  }
});

test("the list does not load the drawing library", async ({ app, page, seed }) => {
  const { ids } = await seed<{ ids: Ids }>(plan());
  await app.open();
  await app.ipc("canvas_apply", {
    owner: ids.Trip,
    upsert: [
      {
        item_id: "box",
        object_id: null,
        x: 1,
        y: 2,
        w: 3,
        h: 4,
        z: "a1",
        style_json: { type: "rectangle", version: 1, versionNonce: 1 },
      },
    ],
    remove: [],
  });
  await page.reload();
  const asked: string[] = [];
  page.on("request", (request) => {
    asked.push(request.url());
  });
  await app.openPage("Trip");
  await app.view("Canvas");
  await app.press(app.pane().getByRole("checkbox", { name: "Show as list" }));
  await expect(app.pane().getByRole("grid", { name: "On this canvas" })).toBeVisible();
  // The picture was there for a moment, so this is only about what comes after a new start.
  await page.reload();
  asked.length = 0;
  await app.openPage("Trip");
  await expect(app.title()).toHaveValue("Trip");
  expect(asked.filter((url) => url.includes("excalidraw-assets"))).toEqual([]);
});

test("nothing on a canvas leads out of the app", async ({ app, page, seed }) => {
  await open(app, seed);
  await draw(app);
  await expect(count(app)).toHaveText("0 cards and 1 drawn element.");
  const links = () => page.locator(".excalidraw a[href]:visible").count();
  expect(await links()).toBe(0);

  await app.press(host(app).getByTestId("main-menu-trigger"));
  const menu = host(app).locator(".dropdown-menu");
  await expect(menu).toBeVisible();
  expect(await links()).toBe(0);
  await expect(menu.locator(".dropdown-menu-item")).toHaveCount(2);

  await app.press(menu.getByTestId("help-menu-item"));
  await expect(page.locator(".excalidraw .HelpDialog")).toBeVisible();
  expect(await links()).toBe(0);
  // Excalidraw would focus a link that is hidden here, and Escape would then have nothing to close.
  await expect(page.locator(".excalidraw .HelpDialog .Modal__content")).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(page.locator(".excalidraw .HelpDialog")).toHaveCount(0);

  // The library, and the tools that would embed a page.
  await expect(host(app).locator(".default-sidebar-trigger")).toBeHidden();
  await expect(host(app).getByTestId("toolbar-magicframe")).toHaveCount(0);
  await app.press(host(app).locator(".App-toolbar__extra-tools-trigger"));
  await expect(host(app).locator(".dropdown-menu")).toBeVisible();
  // Two items have this name: a web page in a frame, and the Mermaid importer, which is not installed.
  await expect(host(app).locator('[data-testid="toolbar-embeddable"]:visible')).toHaveCount(0);
  expect(await links()).toBe(0);
});

test("a shape cannot be given a link, and Ctrl+K is still the palette", async ({
  app,
  page,
  seed,
}, testInfo) => {
  test.skip(testInfo.project.name.includes("phone"), "a keyboard");
  await open(app, seed);
  await draw(app);
  await expect(count(app)).toHaveText("0 cards and 1 drawn element.");
  // What was just drawn is chosen.
  await page.keyboard.press("ControlOrMeta+k");
  await expect(page.locator(".excalidraw-hyperlinkContainer:visible")).toHaveCount(0);
  await expect(page.getByRole("dialog", { name: "Search" })).toBeVisible();
  await page.keyboard.press("Escape");

  const box = await host(app).boundingBox();
  if (!box) throw new Error("no stage");
  await page.mouse.click(box.x + box.width / 2, box.y + box.height / 2 - 40, { button: "right" });
  await expect(page.locator(".context-menu")).toBeVisible();
  await expect(page.locator('.context-menu [data-testid="hyperlink"]:visible')).toHaveCount(0);
  await page.keyboard.press("Escape");

  // Finding text is there, and the library that shares its sidebar, with its link to a site of them, is not.
  await page.mouse.click(box.x + box.width / 2, box.y + box.height - 40);
  await page.keyboard.press("ControlOrMeta+f");
  await expect(page.locator(".default-sidebar input")).toBeVisible();
  await expect(page.locator(".default-sidebar .sidebar-triggers")).toBeHidden();
  await expect(page.locator(".excalidraw a[href]:visible")).toHaveCount(0);
});

test("a card's link opens its page", async ({ app, page, seed }, testInfo) => {
  test.skip(testInfo.project.name.includes("phone"), "with a mouse");
  await open(app, seed);
  await addCard(app, "Reykjavik");
  // Nothing chosen: a chosen element's link is Excalidraw's editor, and the app's Open is there instead.
  const stage = await host(app).boundingBox();
  if (!stage) throw new Error("no stage");
  await page.mouse.click(stage.x + stage.width / 2, stage.y + stage.height - 30);
  await expect(app.pane().locator(".dbme-canvas__selected")).toHaveCount(0);
  const box = await card(app, "Reykjavik").boundingBox();
  if (!box) throw new Error("no card");
  await page.mouse.move(box.x + box.width + 8, box.y - 8, { steps: 3 });
  await page.mouse.down();
  await page.mouse.up();
  await expect(app.currentCrumb()).toHaveText("Reykjavik");
});

test("a picture is not kept, and that is said", async ({ app, page, seed }) => {
  await open(app, seed);
  await put(
    page,
    rectangle("picture", 40, {
      type: "image",
      fileId: "nothing",
      status: "pending",
      scale: [1, 1],
    }),
  );
  await expect(app.pane().getByText("A canvas does not keep pictures yet.")).toBeVisible();
  await page.waitForTimeout(600);
  await expect(count(app)).toHaveText("0 cards and 0 drawn elements.");
  await page.reload();
  await app.openPage("Trip");
  await app.view("Canvas");
  await ready(page);
  await expect.poll(() => shown(page)).toEqual([]);
});

test("the fonts' licences ship with them", async ({ page }) => {
  const notice = await page.request.get("/excalidraw-assets/NOTICE.txt");
  expect(notice.status()).toBe(200);
  const text = await notice.text();
  expect(text).toContain("SIL OPEN FONT LICENSE");
  expect(text).toContain("Excalifont");
  const left = await page.request.get(
    "/excalidraw-assets/fonts/Liberation/LiberationSans-Regular.woff2",
  );
  expect(left.status()).toBe(404);
  const kept = await page.request.get("/excalidraw-assets/fonts/Virgil/Virgil-Regular.woff2");
  expect(kept.status()).toBe(200);
});

test("it fills its pane, in both themes, and passes axe", async ({ app, page, seed }) => {
  await open(app, seed);
  await addCard(app, "Reykjavik");
  const panel = app.pane().locator(".pane__panel--canvas");
  const [stage, whole] = await Promise.all([host(app).boundingBox(), panel.boundingBox()]);
  if (!stage || !whole) throw new Error("no stage");
  // On a phone the bar and what is chosen wrap to more lines.
  expect(whole.height - stage.height).toBeLessThan(app.phone ? 300 : 160);
  expect(stage.x + stage.width).toBeLessThanOrEqual(whole.x + whole.width + 1);
  await expect
    .poll(() => page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth))
    .toBe(true);

  await expectAccessible(page, ".dbme-canvas > .dbme-view-bar");
  await expect(app.pane().locator(".dbme-canvas__selected")).toBeVisible();
  await expectAccessible(page, ".dbme-canvas__selected");
  await app.ipc("settings_set", { patch: { theme: "dark" } });
  await page.reload();
  await app.openPage("Trip");
  await app.view("Canvas");
  await ready(page);
  await expect(page.locator(".excalidraw.theme--dark")).toBeVisible();
  await expectAccessible(page, ".dbme-canvas > .dbme-view-bar");
  await app.press(app.pane().getByRole("checkbox", { name: "Show as list" }));
  await expectAccessible(page, ".dbme-canvas");
});
