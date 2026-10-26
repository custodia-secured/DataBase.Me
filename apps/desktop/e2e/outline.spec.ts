// SPDX-License-Identifier: AGPL-3.0-or-later
/** The Outline view: every line is an object, and its keys create, nest and move objects. */
import type { Locator } from "@playwright/test";
import { workspace } from "./data";
import { expect, test, type App, type TreeIds } from "./fixtures";

interface Summary {
  object: { title: string };
}

let ids: TreeIds;

test.beforeEach(async ({ seed, app }) => {
  ids = await seed(workspace());
  await app.open();
  await app.view("Outline");
});

function outline(app: App): Locator {
  return app.pane().getByRole("region", { name: "Outline" });
}

/** The lines' titles, top to bottom, as shown. */
function lines(app: App): Promise<string[]> {
  return outline(app)
    .locator("input.dbme-outline__title")
    .evaluateAll((inputs) => inputs.map((input) => (input as HTMLInputElement).value));
}

/** The titles of an object's children in the vault, in order. */
async function children(app: App, title: string): Promise<string[]> {
  const parent = ids.ids[title];
  const page = await app.ipc<Summary[]>("objects_page", {
    source: { kind: "children", parent },
    sort: { by: "manual", field: null, descending: false },
    offset: 0,
    limit: 100,
  });
  return page.map((summary) => summary.object.title);
}

test("the outline works with the keyboard alone", async ({ app, page }) => {
  const line = (name: string) => outline(app).getByRole("textbox", { name, exact: true });
  await expect.poll(() => lines(app)).toEqual(["Alpha", "Beta", "Beta notes", "Gamma"]);

  // Enter adds a line after this one and moves to it.
  await line("Alpha, level 1").focus();
  await page.keyboard.press("End");
  await page.keyboard.press("Enter");
  await expect.poll(() => lines(app)).toEqual(["Alpha", "", "Beta", "Beta notes", "Gamma"]);
  await expect(line("Untitled, level 1")).toBeFocused();
  await page.keyboard.type("Delta");

  // Tab nests it under the line above; Shift+Tab takes it back out.
  await page.keyboard.press("Tab");
  await expect(line("Delta, level 2")).toBeFocused();
  await expect(line("Alpha, level 1, expanded")).toBeVisible();
  await expect.poll(() => children(app, "Alpha")).toEqual(["Delta"]);
  await page.keyboard.press("Shift+Tab");
  await expect(line("Delta, level 1")).toBeFocused();
  await expect.poll(() => children(app, "Home")).toEqual(["Alpha", "Delta", "Beta", "Gamma"]);

  // Alt+↓ moves it below its next sibling.
  await page.keyboard.press("Alt+ArrowDown");
  await expect(line("Delta, level 1")).toBeFocused();
  await expect.poll(() => children(app, "Home")).toEqual(["Alpha", "Beta", "Delta", "Gamma"]);

  // ↑ goes to the line above; Ctrl+↑ folds a line's children away and Ctrl+↓ shows them again.
  await page.keyboard.press("ArrowUp");
  await expect(line("Beta notes, level 2")).toBeFocused();
  await page.keyboard.press("ArrowUp");
  await expect(line("Beta, level 1, expanded")).toBeFocused();
  await page.keyboard.press("ControlOrMeta+ArrowUp");
  await expect(line("Beta, level 1, collapsed")).toBeFocused();
  await expect.poll(() => lines(app)).toEqual(["Alpha", "Beta", "Delta", "Gamma"]);
  await page.keyboard.press("ControlOrMeta+ArrowDown");
  await expect(line("Beta notes, level 2")).toBeVisible();

  // Backspace on an empty line deletes it.
  await line("Gamma, level 1").focus();
  await page.keyboard.press("Enter");
  await expect(line("Untitled, level 1")).toBeFocused();
  await page.keyboard.press("Backspace");
  await expect(line("Gamma, level 1")).toBeFocused();
  await expect.poll(() => children(app, "Home")).toEqual(["Alpha", "Beta", "Delta", "Gamma"]);

  // Ctrl+Enter opens the line's object.
  await page.keyboard.press("ControlOrMeta+Enter");
  await expect(app.currentCrumb()).toHaveText("Gamma");
});

test("lines are added, renamed and folded by touch or pointer", async ({ app, page }) => {
  await app.press(outline(app).getByRole("button", { name: "Add line" }));
  const added = outline(app).getByRole("textbox", { name: "Untitled, level 1" });
  await expect(added).toBeFocused();
  await page.keyboard.type("Epsilon");
  await app.press(outline(app).getByRole("button", { name: "Collapse Beta" }));
  await expect(outline(app).getByRole("button", { name: "Expand Beta" })).toBeVisible();
  await expect.poll(() => lines(app)).toEqual(["Alpha", "Beta", "Gamma", "Epsilon"]);
  await expect.poll(() => children(app, "Home")).toEqual(["Alpha", "Beta", "Gamma", "Epsilon"]);
});
