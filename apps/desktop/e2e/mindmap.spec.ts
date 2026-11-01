// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * The Mind map view (ADR-0019): every edit is a command, so the vault and the other views follow,
 * and everything a pointer can do has a keyboard and a touch way too.
 */
import type { Locator, Page } from "@playwright/test";
import { workspace } from "./data";
import { expect, expectAccessible, test, type App, type TreeIds } from "./fixtures";

interface Summary {
  object: { title: string };
}

let ids: TreeIds;

test.beforeEach(async ({ seed, app }) => {
  ids = await seed(workspace());
  await app.open();
  await app.view("Mind map");
  await expect(map(app)).toBeVisible();
  await expect(node(app, "Beta notes")).toBeVisible();
});

function map(app: App, pane: 0 | 1 = 0): Locator {
  return app.pane(pane).getByRole("tree", { name: "Mind map" });
}

function node(app: App, title: string, pane: 0 | 1 = 0): Locator {
  return map(app, pane).getByRole("treeitem", { name: title, exact: true });
}

function button(app: App, name: string): Locator {
  return app.pane().getByRole("button", { name, exact: true });
}

/** The titles of an object's children in the vault, in order. */
async function children(app: App, title: string): Promise<string[]> {
  const page = await app.ipc<Summary[]>("objects_page", {
    source: { kind: "children", parent: ids.ids[title] },
    sort: { by: "manual", field: null, descending: false },
    offset: 0,
    limit: 100,
  });
  return page.map((summary) => summary.object.title);
}

async function centre(target: Locator, share = 0.5): Promise<{ x: number; y: number }> {
  const box = await target.boundingBox();
  if (!box) throw new Error("not on screen");
  return { x: box.x + box.width / 2, y: box.y + box.height * share };
}

async function mouseDrag(
  page: Page,
  from: { x: number; y: number },
  to: { x: number; y: number },
): Promise<void> {
  await page.mouse.move(from.x, from.y);
  await page.mouse.down();
  await page.mouse.move(from.x + 8, from.y + 8, { steps: 4 });
  await page.mouse.move(to.x, to.y, { steps: 12 });
  await page.mouse.up();
}

test("shows the page and everything below it as a tree", async ({ app }) => {
  await expect(node(app, "Home")).toHaveAttribute("aria-level", "1");
  await expect(node(app, "Alpha")).toHaveAttribute("aria-level", "2");
  await expect(node(app, "Beta")).toHaveAttribute("aria-expanded", "true");
  await expect(node(app, "Beta notes")).toHaveAttribute("aria-level", "3");
  await expect(node(app, "Gamma")).toHaveAttribute("aria-posinset", "3");
  await expect(node(app, "Gamma")).toHaveAttribute("aria-setsize", "3");
  await expect(map(app).getByRole("treeitem")).toHaveCount(5);
  await expectAccessible(app.page, ".dbme-map");
});

test("the map works with the keyboard alone", async ({ app, page }) => {
  const tree = map(app);
  const active = () => app.activeText(tree);
  await tree.focus();
  await expect.poll(active).toBe("Home");
  await page.keyboard.press("ArrowRight");
  await expect.poll(active).toBe("Alpha");
  await page.keyboard.press("ArrowDown");
  await expect.poll(active).toBe("Beta");
  await page.keyboard.press("ArrowRight");
  await expect.poll(active).toBe("Beta notes");
  await page.keyboard.press("ArrowLeft");
  await expect.poll(active).toBe("Beta");

  // Tab adds a child and names it; Enter adds a sibling and names it.
  await page.keyboard.press("Tab");
  await expect(tree.getByRole("textbox", { name: "Rename Untitled" })).toBeFocused();
  await page.keyboard.type("Delta");
  await page.keyboard.press("Enter");
  await expect(tree).toBeFocused();
  await expect.poll(() => children(app, "Beta")).toEqual(["Beta notes", "Delta"]);
  await expect.poll(active).toBe("Delta");
  await page.keyboard.press("Enter");
  await expect(tree.getByRole("textbox", { name: "Rename Untitled" })).toBeFocused();
  await page.keyboard.type("Epsilon");
  await page.keyboard.press("Enter");
  await expect.poll(() => children(app, "Beta")).toEqual(["Beta notes", "Delta", "Epsilon"]);

  // F2 renames.
  await expect(tree).toBeFocused();
  await page.keyboard.press("F2");
  await page.keyboard.type("Zeta");
  await page.keyboard.press("Enter");
  await expect.poll(() => children(app, "Beta")).toEqual(["Beta notes", "Delta", "Zeta"]);

  // Alt+Up and Alt+Down reorder among the siblings.
  await expect(tree).toBeFocused();
  await page.keyboard.press("Alt+ArrowUp");
  await expect.poll(() => children(app, "Beta")).toEqual(["Beta notes", "Zeta", "Delta"]);
  await page.keyboard.press("Alt+ArrowDown");
  await expect.poll(() => children(app, "Beta")).toEqual(["Beta notes", "Delta", "Zeta"]);

  // Delete sends it to the Trash, and the keyboard goes to the nearest sibling.
  await page.keyboard.press("Delete");
  await expect.poll(() => children(app, "Beta")).toEqual(["Beta notes", "Delta"]);
  await expect.poll(active).toBe("Delta");

  // Space folds a branch.
  await page.keyboard.press("ArrowLeft");
  await expect.poll(active).toBe("Beta");
  await page.keyboard.press("Space");
  await expect(node(app, "Beta")).toHaveAttribute("aria-expanded", "false");
  await expect(node(app, "Beta notes")).toBeHidden();
  await page.keyboard.press("Space");
  await expect(node(app, "Beta notes")).toBeVisible();

  // Control+Enter opens the page.
  await page.keyboard.press("Control+Enter");
  await expect(app.currentCrumb()).toHaveText("Beta");
});

test("the toolbar adds, moves, trashes and opens, and Undo takes it back", async ({
  app,
  page,
}) => {
  await app.press(node(app, "Alpha"));
  await expect(node(app, "Alpha")).toHaveAttribute("aria-selected", "true");

  await app.press(button(app, "Add child"));
  const name = map(app).getByRole("textbox", { name: "Rename Untitled" });
  await expect(name).toBeFocused();
  await page.keyboard.type("Delta");
  await page.keyboard.press("Enter");
  await expect.poll(() => children(app, "Alpha")).toEqual(["Delta"]);

  await app.press(node(app, "Gamma"));
  await app.press(button(app, "Move to…"));
  const dialog = page.getByRole("dialog", { name: "Move “Gamma” to…" });
  await dialog.getByRole("searchbox", { name: "Find a page by title" }).fill("Alpha");
  await app.press(dialog.getByRole("button", { name: "Alpha", exact: true }));
  await expect(dialog).toBeHidden();
  await expect.poll(() => children(app, "Alpha")).toEqual(["Delta", "Gamma"]);
  await expect(node(app, "Gamma")).toHaveAttribute("aria-level", "3");

  await app.press(page.getByRole("button", { name: "Undo" }));
  await expect.poll(() => children(app, "Home")).toEqual(["Alpha", "Beta", "Gamma"]);
  await expect(node(app, "Gamma")).toHaveAttribute("aria-level", "2");

  await app.press(node(app, "Gamma"));
  await app.press(button(app, "Trash"));
  await expect(node(app, "Gamma")).toBeHidden();
  await expect.poll(() => children(app, "Home")).toEqual(["Alpha", "Beta"]);
  await app.press(page.getByRole("button", { name: "Undo" }));
  await expect(node(app, "Gamma")).toBeVisible();

  await app.press(node(app, "Beta"));
  await app.press(button(app, "Open"));
  await expect(app.currentCrumb()).toHaveText("Beta");
});

test("a relation is drawn from the toolbar, shown, removed, and comes back with Undo", async ({
  app,
  page,
}) => {
  await app.press(node(app, "Alpha"));
  await app.press(button(app, "Relate to…"));
  const dialog = page.getByRole("dialog", { name: "Relate “Alpha” to…" });
  await dialog.getByRole("searchbox", { name: "Find a page by title" }).fill("Gamma");
  await app.press(dialog.getByRole("button", { name: "Gamma", exact: true }));
  await expect(dialog).toBeHidden();

  const list = app.pane().getByRole("list", { name: "Relations of Alpha" });
  // Home's text already mentions Alpha, so that relation is drawn too.
  await expect(list.getByText("→ Gamma (related)")).toBeVisible();
  await expect(map(app).locator(".dbme-map__relation")).toHaveCount(2);

  await app.press(list.getByRole("button", { name: "Remove the relation with Gamma" }));
  await expect(list.getByText("→ Gamma (related)")).toBeHidden();
  await expect(map(app).locator(".dbme-map__relation")).toHaveCount(1);

  await app.press(page.getByRole("button", { name: "Undo" }));
  await expect(map(app).locator(".dbme-map__relation")).toHaveCount(2);
});

test.describe("with a mouse", () => {
  test.skip(({ isMobile }) => isMobile, "these are drags with a mouse");

  test("dragging a node onto another puts it inside", async ({ app, page }) => {
    await mouseDrag(page, await centre(node(app, "Gamma")), await centre(node(app, "Alpha")));
    await expect.poll(() => children(app, "Alpha")).toEqual(["Gamma"]);
    await expect(node(app, "Gamma")).toHaveAttribute("aria-level", "3");
  });

  test("dragging a node to the edge of another puts it beside", async ({ app, page }) => {
    await mouseDrag(page, await centre(node(app, "Gamma")), await centre(node(app, "Alpha"), 0.1));
    await expect.poll(() => children(app, "Home")).toEqual(["Gamma", "Alpha", "Beta"]);
  });

  test("a node can't be dragged into its own branch", async ({ app, page }) => {
    await mouseDrag(page, await centre(node(app, "Beta")), await centre(node(app, "Beta notes")));
    await expect(node(app, "Beta")).toBeVisible();
    await expect.poll(() => children(app, "Home")).toEqual(["Alpha", "Beta", "Gamma"]);
  });

  test("dragging between handles draws a relation", async ({ app, page }) => {
    await app.press(node(app, "Alpha"));
    const handle = map(app).locator(
      `.react-flow__node[data-id="${ids.ids.Alpha ?? ""}"] .dbme-map__source`,
    );
    await mouseDrag(page, await centre(handle), await centre(node(app, "Gamma")));
    await expect(map(app).locator(".dbme-map__relation")).toHaveCount(2);
    await expect(
      app.pane().getByRole("list", { name: "Relations of Alpha" }).getByText("→ Gamma (related)"),
    ).toBeVisible();
  });
});

test("a long press and a drag moves a node on a touch screen", async ({
  app,
  page,
  isMobile,
  browserName,
}) => {
  test.skip(
    !isMobile || browserName === "webkit",
    "Playwright cannot send touch drags to WebKit; Move to… is the touch path tested there",
  );
  const from = await centre(node(app, "Gamma"));
  const to = await centre(node(app, "Alpha"));
  const cdp = await page.context().newCDPSession(page);
  const touch = (
    type: "touchStart" | "touchMove" | "touchEnd",
    points: { x: number; y: number }[],
  ) => cdp.send("Input.dispatchTouchEvent", { type, touchPoints: points });
  await touch("touchStart", [from]);
  await expect(node(app, "Gamma")).toHaveAttribute("aria-selected", "true");
  for (let step = 1; step <= 8; step++) {
    await touch("touchMove", [
      {
        x: from.x + ((to.x - from.x) * step) / 8,
        y: from.y + ((to.y - from.y) * step) / 8,
      },
    ]);
  }
  await touch("touchEnd", []);
  await expect.poll(() => children(app, "Alpha")).toEqual(["Gamma"]);
});

test.describe("beside the Outline", () => {
  test.skip(({ isMobile }) => isMobile, "the split needs a window 1100 px wide");

  test("edits in either show at once in the other", async ({ app, page }) => {
    await app.press(page.getByRole("button", { name: "Split view" }));
    await expect(app.pane(1)).toBeVisible();
    await app.view("Outline", 1);
    const outline = app.pane(1).getByRole("region", { name: "Outline" });

    // From the map to the Outline.
    await map(app).focus();
    await page.keyboard.press("ArrowRight");
    await page.keyboard.press("F2");
    await page.keyboard.type("Alpha prime");
    await page.keyboard.press("Enter");
    await expect(outline.getByRole("textbox", { name: "Alpha prime, level 1" })).toBeVisible();

    // From the Outline to the map.
    await outline.getByRole("textbox", { name: "Gamma, level 1" }).fill("Gamma two");
    await expect(node(app, "Gamma two")).toBeVisible();
    await outline.getByRole("textbox", { name: "Gamma two, level 1" }).focus();
    await page.keyboard.press("Tab");
    await expect(node(app, "Gamma two")).toHaveAttribute("aria-level", "3");
  });
});
