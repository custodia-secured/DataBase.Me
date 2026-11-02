// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * A map too tall for the screen (ADR-0019): nodes out of view aren't in the page, and on the overview
 * they are too small to read, so whatever the keyboard reaches has to come into view first.
 */
import type { Locator } from "@playwright/test";
import { tallMap } from "./data";
import { expect, test, type App } from "./fixtures";

test.beforeEach(async ({ seed, app }) => {
  await seed(tallMap());
  await app.open();
  await app.view("Mind map");
  await expect(node(app, "Tall")).toBeInViewport();
});

function map(app: App): Locator {
  return app.pane().getByRole("tree", { name: "Mind map" });
}

function node(app: App, title: string): Locator {
  return map(app).getByRole("treeitem", { name: title, exact: true });
}

test("a new node far down its family comes into view to be named", async ({ app, page }) => {
  await map(app).focus();
  await page.keyboard.press("ArrowRight");
  await expect(node(app, "A")).toBeInViewport();

  // It lands below A's 30 children, a long way off the screen.
  await page.keyboard.press("Tab");
  const name = map(app).getByRole("textbox", { name: "Rename Untitled" });
  await expect(name).toBeFocused();
  await expect(name).toBeInViewport();
  await page.keyboard.type("Named");
  await page.keyboard.press("Enter");
  await expect(map(app)).toBeFocused();
  await expect(node(app, "Named")).toBeInViewport();
  await expect(node(app, "Named")).toHaveAttribute("aria-posinset", "31");
});

test("a node named from the overview comes close enough to read", async ({ app, page }) => {
  await app.press(app.pane().getByRole("button", { name: "Fit the map" }));
  await expect(app.pane().locator("canvas[data-drawn]")).toBeVisible();
  await map(app).focus();

  await page.keyboard.press("F2");
  const name = map(app).getByRole("textbox", { name: "Rename Tall" });
  await expect(name).toBeFocused();
  await expect(name).toBeInViewport();
  await expect.poll(async () => (await name.boundingBox())?.width ?? 0).toBeGreaterThan(100);
});

test("the keyboard goes to a sibling that is far away, and it comes into view", async ({
  app,
  page,
}) => {
  await map(app).focus();
  await page.keyboard.press("ArrowRight");
  await expect(node(app, "A")).toBeInViewport();
  await page.keyboard.press("Delete");
  await expect(node(app, "B")).toBeInViewport();
  await expect(node(app, "B")).toHaveAttribute("aria-selected", "true");
  await expect(node(app, "B")).toHaveAttribute("aria-posinset", "1");
});
