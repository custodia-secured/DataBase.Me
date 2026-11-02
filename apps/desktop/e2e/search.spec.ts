// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * Search (ADR-0020): the command palette by keyboard and touch, search across titles, bodies and
 * field values, the results surface with its facets, and saved views, which Undo covers.
 */
import type { Locator } from "@playwright/test";
import { workspace } from "./data";
import { expect, expectAccessible, test, type App } from "./fixtures";

test.beforeEach(async ({ seed, app }) => {
  await seed(workspace());
  await app.open();
  await expect(app.currentCrumb()).toHaveText("Home");
});

function palette(app: App): Locator {
  return app.page.getByRole("dialog", { name: "Search" });
}

function box(app: App): Locator {
  return palette(app).getByRole("combobox", { name: "Search or type > for a command" });
}

function option(app: App, name: string | RegExp): Locator {
  return palette(app).getByRole("option", { name });
}

/** Opens the palette the way the project does: Ctrl+K on a desktop, the Search button by touch. */
async function openPalette(app: App): Promise<void> {
  if (app.phone) await app.press(app.page.getByRole("button", { name: "Search", exact: true }));
  else await app.page.keyboard.press("Control+k");
  await expect(box(app)).toBeFocused();
}

function results(app: App): Locator {
  return app.page.getByRole("region", { name: "Results" });
}

/** A result row: its title, then its type. */
function row(app: App, title: string, type = "Milestone"): Locator {
  return results(app).getByRole("button", { name: `${title} ${type}`, exact: true });
}

/** On a phone the sidebar is a drawer, which must close before the page behind it is used. */
async function closeSidebar(app: App): Promise<void> {
  if (!app.phone) return;
  const drawer = app.page.getByRole("dialog", { name: "Pages" });
  if (await drawer.isVisible()) {
    await app.page.keyboard.press("Escape");
    await expect(drawer).toBeHidden();
  }
}

test("the palette finds pages by title and opens them", async ({ app, page }) => {
  await openPalette(app);
  await page.keyboard.type("notes");
  await expect(option(app, /Beta notes/)).toHaveAttribute("aria-selected", "true");
  await expectAccessible(page, ".dbme-dialog--palette");
  await page.keyboard.press("Enter");
  await expect(palette(app)).toBeHidden();
  await expect(app.currentCrumb()).toHaveText("Beta notes");
});

test("the palette finds words inside a page and shows where", async ({ app, page }) => {
  await app.openPage("Inbox");
  await openPalette(app);
  await page.keyboard.type("plan");
  const home = option(app, /Home/);
  await expect(home).toContainText("Write the plan");
  await expect(home.locator("mark")).toHaveText("plan");
  await app.press(home);
  await expect(app.currentCrumb()).toHaveText("Home");
});

test("a line starting with > lists commands", async ({ app, page }) => {
  await openPalette(app);
  await page.keyboard.type(">mind");
  await expect(option(app, "Switch to Mind map")).toHaveAttribute("aria-selected", "true");
  await expect(option(app, /Beta/)).toHaveCount(0);
  await page.keyboard.press("Enter");
  await expect(app.pane().getByRole("tab", { name: "Mind map" })).toHaveAttribute(
    "aria-selected",
    "true",
  );

  await openPalette(app);
  await page.keyboard.type(">nothing like this");
  await expect(palette(app).getByRole("status")).toHaveText("No such command.");
  await page.keyboard.press("Escape");
  await expect(palette(app)).toBeHidden();
});

test("every result opens on the results surface, where facets narrow it", async ({ app, page }) => {
  // "do" is the start of the field values "doing" and "done", on Alpha and Gamma.
  await openPalette(app);
  await page.keyboard.type("do");
  await page.keyboard.press("Control+Enter");
  const region = results(app);
  await expect(region.getByRole("heading", { name: "Search: do" })).toBeVisible();
  await expect(region.getByRole("status")).toHaveText("2 results");
  await expect(row(app, "Alpha")).toBeVisible();
  await expect(row(app, "Gamma")).toBeVisible();

  const chips = region.getByRole("group", { name: "Narrow the results" });
  await expect(chips.getByRole("button", { name: "Milestone (2)" })).toBeVisible();
  await app.press(chips.getByRole("button", { name: "Status: Done (1)" }));
  await expect(region.getByRole("status")).toHaveText("1 result");
  await expect(row(app, "Gamma")).toBeVisible();
  await expect(row(app, "Alpha")).toHaveCount(0);
  await expectAccessible(page, ".dbme-results");

  await app.press(row(app, "Gamma"));
  await expect(region).toBeHidden();
  await expect(app.currentCrumb()).toHaveText("Gamma");
});

test("a search is saved as a view, listed, opened, and undone", async ({ app, page }) => {
  await openPalette(app);
  await page.keyboard.type("do");
  await page.keyboard.press("Control+Enter");
  const region = results(app);
  await app.press(region.getByRole("button", { name: "Save as view…" }));
  await region.getByRole("textbox", { name: "View name" }).fill("Doing and done");
  await app.press(region.getByRole("button", { name: "Save", exact: true }));
  await expect(region.getByRole("heading", { name: "Doing and done" })).toBeVisible();

  const saved = (await app.sidebar()).getByRole("region", { name: "Saved views" });
  const item = saved.getByRole("button", { name: "Doing and done" });
  await expect(item).toHaveAttribute("aria-current", "page");
  await closeSidebar(app);

  // Close it, open it again from the sidebar, and from the palette.
  await app.press(region.getByRole("button", { name: "Close" }));
  await expect(region).toBeHidden();
  await app.press((await app.sidebar()).getByRole("button", { name: "Doing and done" }));
  await expect(region.getByRole("heading", { name: "Doing and done" })).toBeVisible();
  await expect(region.getByRole("status")).toHaveText("2 results");
  await app.press(region.getByRole("button", { name: "Close" }));
  await openPalette(app);
  await page.keyboard.type(">open view");
  await expect(option(app, "Open view: Doing and done")).toBeVisible();
  await page.keyboard.press("Enter");
  await expect(region.getByRole("heading", { name: "Doing and done" })).toBeVisible();

  // Rename, then delete; Undo brings it back.
  await app.press(region.getByRole("button", { name: "Rename view" }));
  await region.getByRole("textbox", { name: "View name" }).fill("Milestones in play");
  await app.press(region.getByRole("button", { name: "Save", exact: true }));
  await expect(region.getByRole("heading", { name: "Milestones in play" })).toBeVisible();
  await app.press(region.getByRole("button", { name: "Delete view" }));
  await expect(region).toBeHidden();
  await expect((await app.sidebar()).getByRole("region", { name: "Saved views" })).toHaveCount(0);
  if (app.phone) await page.keyboard.press("Escape");
  await app.press(page.getByRole("button", { name: "Undo" }));
  await expect(
    (await app.sidebar()).getByRole("button", { name: "Milestones in play" }),
  ).toBeVisible();
});
