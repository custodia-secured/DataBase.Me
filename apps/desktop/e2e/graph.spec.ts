// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * The Graph view (M8, Task 11, ADR-0034), drawn under the real CSP: nodes and links on WebGL, a layout in a
 * worker of ours that can be paused, the reach of a graph, and the list for whoever cannot use a canvas or
 * whose machine cannot draw one. The fixtures fail any test on a CSP report, an uncaught error or a request
 * leaving the machine.
 */
import type { Locator, Page } from "@playwright/test";
import { graphLoad } from "./data";
import { expect, expectAccessible, test, type App } from "./fixtures";
import "./sigma-handle";

/** A family of pages under Trip, with relations among them. */
function plan(): unknown {
  return {
    kind: "tree",
    types: [{ name: "Stop" }, { name: "Traveller" }],
    objects: [
      {
        title: "Trip",
        children: [
          {
            title: "Reykjavik",
            types: ["Stop"],
            children: [{ title: "Harbour", types: ["Stop"] }],
          },
          { title: "Nairobi", types: ["Stop"] },
          { title: "Ada", types: ["Traveller"] },
          { title: "Untyped" },
        ],
      },
      { title: "Elsewhere" },
    ],
  };
}

type Ids = Record<string, string>;
const host = (app: App): Locator => app.pane().locator(".dbme-graph__host");
const count = (app: App): Locator => app.pane().locator(".dbme-graph__count").first();

async function relate(app: App, ids: Ids, from: string, to: string): Promise<void> {
  await app.ipc("relation_create", { src: ids[from], dst: ids[to] });
}

async function open(app: App, seed: (plan: unknown) => Promise<{ ids: Ids }>): Promise<Ids> {
  const { ids } = await seed(plan());
  await app.open();
  await relate(app, ids, "Reykjavik", "Nairobi");
  await relate(app, ids, "Ada", "Reykjavik");
  await relate(app, ids, "Harbour", "Ada");
  await app.page.reload();
  await app.openPage("Trip");
  await app.view("Graph");
  return ids;
}

/** The picture, on a phone as well, where the list comes first. */
async function picture(app: App): Promise<void> {
  if (app.phone) await app.press(app.pane().getByRole("checkbox", { name: "Show as list" }));
  await expect(host(app)).toHaveAttribute("data-drawn", "true");
}

async function list(app: App): Promise<Locator> {
  if (!app.phone) await app.press(app.pane().getByRole("checkbox", { name: "Show as list" }));
  const grid = app.pane().getByRole("grid", { name: "Nodes" });
  await expect(grid).toBeVisible();
  return grid;
}

/** Where a node is drawn on the page. */
async function screenPlace(page: Page, app: App, id: string): Promise<{ x: number; y: number }> {
  const inner = await page.evaluate((node) => {
    const sigma = window.__dbmeSigma;
    const data = sigma?.getNodeDisplayData(node);
    if (!sigma || !data) throw new Error(`node ${node} is not drawn`);
    return sigma.framedGraphToViewport(data);
  }, id);
  const box = await host(app).boundingBox();
  if (!box) throw new Error("the graph is not on the page");
  return { x: box.x + inner.x, y: box.y + inner.y };
}

/** Keeps the layout from starting, so that a node stays where the test finds it. */
async function holdLayout(page: Page): Promise<void> {
  await page.emulateMedia({ reducedMotion: "reduce" });
}

test("it draws the nodes and links on WebGL, and says how many in words", async ({
  app,
  seed,
  page,
}) => {
  await open(app, seed);
  await picture(app);
  // Below Trip, all the way down: five items, three relations and one parent link inside them.
  await expect(count(app)).toHaveText("5 items and 4 links.");
  await expect(host(app)).toHaveAttribute("data-nodes", "5");
  await expect(host(app)).toHaveAttribute("data-edges", "4");
  await expect(host(app)).toHaveAttribute("aria-label", "Graph. 5 items and 4 links.");
  // It takes the room the pane has, and does not stop at its smallest.
  if (!app.phone) expect((await host(app).boundingBox())?.height ?? 0).toBeGreaterThan(400);
  // Something reached the canvas. The buffer is only readable in the frame that drew it.
  const ink = await page.evaluate(() => {
    const sigma = window.__dbmeSigma;
    if (!sigma) return false;
    sigma.refresh();
    const gl = sigma.getWebGLContext();
    const pixels = new Uint8Array(4 * gl.drawingBufferWidth * gl.drawingBufferHeight);
    gl.readPixels(
      0,
      0,
      gl.drawingBufferWidth,
      gl.drawingBufferHeight,
      gl.RGBA,
      gl.UNSIGNED_BYTE,
      pixels,
    );
    return pixels.some((value) => value !== 0);
  });
  expect(ink).toBe(true);
  const key = app.pane().locator(".dbme-graph__key");
  await expect(key).toContainText("Stop");
  await expect(key).toContainText("Traveller");
  await expect(key).toContainText("An arrow is a relation");
  await expectAccessible(page, ".dbme-graph");
});

test("its reach goes from below the page to the whole vault, and a filter narrows either", async ({
  app,
  seed,
}) => {
  await open(app, seed);
  await picture(app);
  await app.pane().getByLabel("Reach").selectOption("vault");
  // Everything there is: Trip, its five, Elsewhere, and the parent links Trip now has as well.
  await expect(host(app)).toHaveAttribute("data-nodes", "7");
  await expect(count(app)).toHaveText("7 items and 8 links.");
  await app.pane().getByLabel("Reach").selectOption("here");
  await expect(host(app)).toHaveAttribute("data-nodes", "5");
  // Only what is a Stop, wherever it is, and the view no longer offers a reach.
  await app.pane().getByLabel("Show", { exact: true }).selectOption({ label: "All Stop" });
  await expect(host(app)).toHaveAttribute("data-nodes", "3");
  await expect(app.pane().getByLabel("Reach")).toHaveCount(0);
});

test("a click chooses a node, says what it joins, and it can be opened from there", async ({
  app,
  seed,
  page,
}) => {
  await holdLayout(page);
  const ids = await open(app, seed);
  await picture(app);
  const place = await screenPlace(page, app, ids.Reykjavik ?? "");
  if (app.phone) await page.touchscreen.tap(place.x, place.y);
  else await page.mouse.click(place.x, place.y);
  const chosen = app.pane().locator(".dbme-graph__selected");
  await expect(chosen).toContainText("Reykjavik");
  await expect(chosen).toContainText("Stop, 2 related, holds 1");
  await app.press(chosen.getByRole("button", { name: "Open" }));
  await expect(app.currentCrumb()).toHaveText("Reykjavik");
});

test("the layout moves the nodes, can be paused, and goes on when asked", async ({
  app,
  seed,
  page,
}) => {
  // Big enough to be still running by the time this gets to it: a few hundred passes at about 3 ms each.
  test.slow();
  await holdLayout(page);
  await seed(graphLoad(3000));
  await app.open();
  await app.view("Graph");
  await picture(app);
  const first = await page.evaluate(() => window.__dbmeSigma?.getGraph().nodes()[0] ?? "");
  const where = () =>
    page.evaluate((id) => {
      const graph = window.__dbmeSigma?.getGraph();
      return [graph?.getNodeAttribute(id, "x"), graph?.getNodeAttribute(id, "y")];
    }, first);
  const start = await where();
  await app.press(app.pane().getByRole("button", { name: "Run layout" }));
  await expect.poll(where).not.toEqual(start);
  await app.press(app.pane().getByRole("button", { name: "Pause layout" }));
  await expect(host(app)).toHaveAttribute("data-layout", "paused");
  const held = await where();
  // A step already on its way is dropped, so nothing moves from here.
  await page.waitForTimeout(600);
  expect(await where()).toEqual(held);
  await app.press(app.pane().getByRole("button", { name: "Run layout" }));
  await expect.poll(where).not.toEqual(held);
});

test("with reduced motion the layout waits to be asked", async ({ app, seed, page }) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
  await open(app, seed);
  await picture(app);
  await expect(host(app)).toHaveAttribute("data-layout", "paused");
  await app.press(app.pane().getByRole("button", { name: "Run layout" }));
  await expect(host(app)).toHaveAttribute("data-passes", /[1-9]/);
});

test("the camera moves by its buttons and by the keyboard", async ({ app, seed, page }) => {
  await open(app, seed);
  await picture(app);
  const camera = () =>
    page.evaluate(() => {
      const state = window.__dbmeSigma?.getCamera().getState();
      return { x: state?.x ?? 0, y: state?.y ?? 0, ratio: state?.ratio ?? 0 };
    });
  const start = await camera();
  await app.press(app.pane().getByRole("button", { name: "Zoom in" }));
  await expect.poll(async () => (await camera()).ratio).toBeLessThan(start.ratio);
  await app.press(app.pane().getByRole("button", { name: "Show all" }));
  await expect.poll(async () => (await camera()).ratio).toBeCloseTo(1);
  if (app.phone) return;
  await host(app).focus();
  await page.keyboard.press("ArrowRight");
  await expect.poll(async () => (await camera()).x).toBeGreaterThan(0.5);
  await page.keyboard.press("-");
  await expect.poll(async () => (await camera()).ratio).toBeGreaterThan(1);
  await page.keyboard.press("0");
  await expect.poll(async () => (await camera()).ratio).toBeCloseTo(1);
});

test("it follows a change, keeping the nodes where they were", async ({ app, seed, page }) => {
  await holdLayout(page);
  const ids = await open(app, seed);
  await picture(app);
  const where = (id: string) =>
    page.evaluate((node) => window.__dbmeSigma?.getGraph().getNodeAttribute(node, "x"), id);
  const before = await where(ids.Nairobi ?? "");
  // Through the UI, since a change has to arrive the way a real one does: Undo takes the last relation away.
  await app.press(page.getByRole("button", { name: "Undo" }).first());
  await expect(host(app)).toHaveAttribute("data-edges", "3");
  await expect(count(app)).toHaveText("5 items and 3 links.");
  expect(await where(ids.Nairobi ?? "")).toEqual(before);
  await app.press(page.getByRole("button", { name: "Redo" }).first());
  await expect(host(app)).toHaveAttribute("data-edges", "4");
  expect(await where(ids.Nairobi ?? "")).toEqual(before);
});

test("a new name changes that label and nothing else", async ({ app, seed, page }) => {
  await holdLayout(page);
  const ids = await open(app, seed);
  await app.ipc("object_rename", { id: ids.Nairobi, title: "Mombasa" });
  await page.reload();
  await app.openPage("Trip");
  await app.view("Graph");
  await picture(app);
  const label = () =>
    page.evaluate(
      (id) => window.__dbmeSigma?.getGraph().getNodeAttribute(id, "label"),
      ids.Nairobi ?? "",
    );
  await expect.poll(label).toBe("Mombasa");
  // Marks the graph that is drawn, which is a new one if anything builds it again.
  await page.evaluate(() => {
    (window as unknown as { __drawn: unknown }).__drawn = window.__dbmeSigma?.getGraph();
  });
  // Undo takes the name back, and the change arrives the way a real one does.
  await app.press(page.getByRole("button", { name: "Undo" }).first());
  await expect.poll(label).toBe("Nairobi");
  expect(
    await page.evaluate(
      () => (window as unknown as { __drawn: unknown }).__drawn === window.__dbmeSigma?.getGraph(),
    ),
  ).toBe(true);
});

test("the chosen node carries between the picture and the list", async ({ app, seed, page }) => {
  test.skip(app.phone, "the keyboard path; a phone chooses by touch in the test above");
  await holdLayout(page);
  const ids = await open(app, seed);
  await picture(app);
  const place = await screenPlace(page, app, ids.Harbour ?? "");
  await page.mouse.click(place.x, place.y);
  const chosen = app.pane().locator(".dbme-graph__selected");
  await expect(chosen).toContainText("Harbour");
  const grid = await list(app);
  // The list opens on it, and what lies near it is already there.
  await expect(app.pane().getByRole("region", { name: "Near Harbour" })).toBeVisible();
  await grid.focus();
  await page.keyboard.press("ArrowUp");
  await expect(app.pane().getByRole("region", { name: "Near Harbour" })).toHaveCount(0);
  const near = await app
    .pane()
    .getByRole("region", { name: /^Near / })
    .getAttribute("aria-label");
  const title = (near ?? "").replace("Near ", "");
  await app.press(app.pane().getByRole("checkbox", { name: "Show as list" }));
  await expect(chosen).toContainText(title);
});

test("the list has every node with its neighbours, by keyboard alone", async ({
  app,
  seed,
  page,
}) => {
  await open(app, seed);
  const grid = await list(app);
  await expect(grid).toHaveAttribute("aria-rowcount", "6");
  await expect(grid).toContainText("Reykjavik");
  await expect(grid).toContainText("Harbour");
  const row = grid.getByRole("row", { name: /^Reykjavik/ });
  await expect(row).toContainText("Stop");
  await expect(row).toContainText("Nairobi, Ada");
  const inside = grid.getByRole("row", { name: /^Harbour/ });
  await expect(inside).toContainText("Reykjavik");
  await expectAccessible(page, ".dbme-graph");
  if (app.phone) return;
  await grid.focus();
  await page.keyboard.press("Home");
  await expect.poll(() => app.activeText(grid)).not.toBeNull();
  await page.keyboard.press("Enter");
  await expect(app.currentCrumb()).not.toHaveText("Trip");
});

test("the list finds a node by name, and says how many", async ({ app, seed }) => {
  await open(app, seed);
  const grid = await list(app);
  await app.pane().getByLabel("Find").fill("REYK");
  await expect(app.pane().getByText("1 found")).toBeVisible();
  await expect(grid).toHaveAttribute("aria-rowcount", "2");
  await expect(grid.getByRole("row", { name: /^Reykjavik/ })).toBeVisible();
  // Nairobi is still named in that row, as what Reykjavik is related to, but has no row of its own.
  await expect(grid.getByRole("row", { name: /^Nairobi/ })).toHaveCount(0);
  await app.pane().getByLabel("Find").fill("nothing like it");
  await expect(app.pane().getByText("0 found")).toBeVisible();
});

test("the list reaches further along the relations, from the store", async ({ app, seed }) => {
  await open(app, seed);
  const grid = await list(app);
  await app.pane().getByLabel("Find").fill("Harbour");
  await expect(grid).toHaveAttribute("aria-rowcount", "2");
  const near = app.pane().getByRole("region", { name: "Near Harbour" });
  // One relation from Harbour is Ada, a second is Reykjavik, and a third is Nairobi.
  await expect(near.getByRole("status")).toHaveText("1 item");
  await expect(near.getByRole("button", { name: "Ada" })).toBeVisible();
  await near.getByLabel("Within").selectOption("2");
  await expect(near.getByRole("status")).toHaveText("2 items");
  await near.getByLabel("Within").selectOption("3");
  await expect(near.getByRole("status")).toHaveText("3 items");
  await app.press(near.getByRole("button", { name: "Nairobi" }));
  await expect(app.currentCrumb()).toHaveText("Nairobi");
});

test("a machine that cannot draw a graph gets the list, and it says so", async ({
  app,
  seed,
  page,
}) => {
  await page.addInitScript(() => {
    // eslint-disable-next-line @typescript-eslint/unbound-method -- it is called on its canvas below
    const original = HTMLCanvasElement.prototype.getContext;
    HTMLCanvasElement.prototype.getContext = function (
      this: HTMLCanvasElement,
      kind: string,
      ...rest: unknown[]
    ) {
      if (kind.startsWith("webgl")) return null;
      return (original as (...args: unknown[]) => unknown).call(this, kind, ...rest);
    } as typeof original;
  });
  await open(app, seed);
  if (app.phone) await app.press(app.pane().getByRole("checkbox", { name: "Show as list" }));
  await expect(app.pane().getByText("This device cannot draw the graph")).toBeVisible();
  await expect(app.pane().getByRole("grid", { name: "Nodes" })).toBeVisible();
});

test("a saved graph keeps its reach", async ({ app, seed }) => {
  await open(app, seed);
  await picture(app);
  await app.pane().getByLabel("Reach").selectOption("vault");
  await expect(host(app)).toHaveAttribute("data-nodes", "7");
  await app.press(app.pane().getByRole("button", { name: "Save view" }));
  await app.page.getByRole("dialog").getByLabel("Name").fill("Everything");
  await app.press(app.page.getByRole("dialog").getByRole("button", { name: "Save" }));
  await expect(app.page.getByRole("dialog")).toBeHidden();
  await app.page.reload();
  await app.view("Graph");
  await picture(app);
  await expect(host(app)).toHaveAttribute("data-nodes", "5");
  await app.pane().getByLabel("Saved view").selectOption({ label: "Everything" });
  await expect(host(app)).toHaveAttribute("data-nodes", "7");
});

test("with nothing below the page it says so", async ({ app, seed }) => {
  await seed({ kind: "tree", objects: [{ title: "Alone" }] });
  await app.open();
  await app.view("Graph");
  await expect(app.pane().getByText("There is nothing here to draw yet.")).toBeVisible();
  await expect(count(app)).toHaveText("0 items and no links.");
});

test("it holds in dark, in colours of its own", async ({ app, seed, page }) => {
  const ids = await open(app, seed);
  await picture(app);
  const light = await page.evaluate(
    (id) => window.__dbmeSigma?.getGraph().getNodeAttribute(id, "color"),
    ids.Reykjavik ?? "",
  );
  await app.ipc("settings_set", { patch: { theme: "dark" } });
  await page.reload();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
  await app.view("Graph");
  await picture(app);
  const dark = await page.evaluate(
    (id) => window.__dbmeSigma?.getGraph().getNodeAttribute(id, "color"),
    ids.Reykjavik ?? "",
  );
  expect(dark).not.toEqual(light);
  await expectAccessible(page, ".dbme-graph");
});

test("a phone starts on the list, and the picture fits it", async ({ app, seed, page }) => {
  test.skip(!app.phone, "the phone's own layout");
  await open(app, seed);
  await expect(app.pane().getByRole("checkbox", { name: "Show as list" })).toBeChecked();
  await expect(app.pane().getByRole("grid", { name: "Nodes" })).toBeVisible();
  await picture(app);
  const width = page.viewportSize()?.width ?? 0;
  const box = await app.pane().locator(".dbme-graph").boundingBox();
  expect((box?.x ?? 0) + (box?.width ?? 0)).toBeLessThanOrEqual(width);
  const stage = await host(app).boundingBox();
  expect(stage?.height ?? 0).toBeGreaterThan(200);
  await expectAccessible(page, ".dbme-graph");
});
