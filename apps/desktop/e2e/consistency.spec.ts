// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * Live consistency across views (ADR-0017): a change made in one view shows at once in every
 * other view of the same data, with no reload, because every committed change announces itself.
 */
import type { Locator } from "@playwright/test";
import { workspace } from "./data";
import { expect, test, type App } from "./fixtures";

test.beforeEach(async ({ seed, app }) => {
  await seed(workspace());
  await app.open();
});

/** The titles in a pane's Table, top to bottom. */
function tableTitles(app: App, pane: 0 | 1): Promise<string[]> {
  return app
    .pane(pane)
    .locator('.dbme-grid__body [role=row] [aria-colindex="2"]')
    .evaluateAll((cells) => cells.map((cell) => cell.textContent));
}

function tableCell(app: App, pane: 0 | 1, row: number, index: number): Locator {
  return app
    .pane(pane)
    .locator(".dbme-grid__body [role=row]")
    .nth(row)
    .locator(`[aria-colindex="${String(index)}"]`);
}

test.describe("in a split view", () => {
  test.skip(({ isMobile }) => isMobile, "the split needs a window 1100 px wide");

  test.beforeEach(async ({ app }) => {
    await app.press(app.page.getByRole("button", { name: "Split view" }));
    await expect(app.pane(1)).toBeVisible();
  });

  test("a rename in the table shows at once beside it and in the sidebar", async ({
    app,
    page,
  }) => {
    const rows = app.pane(1).getByRole("grid", { name: "Rows" });
    await rows.focus();
    await page.keyboard.press("F2");
    await page.keyboard.type("Alpha prime");
    await page.keyboard.press("Enter");
    // Home's text mentions Alpha; the mention shows the live title.
    await expect(app.editor(0).locator(".dbme-mention")).toHaveText("Alpha prime");
    await app.press((await app.treeItem("Home")).locator(".dbme-tree__toggle"));
    await expect(await app.treeItem("Alpha prime")).toBeVisible();
    await app.view("Gallery", 1);
    await expect(app.pane(1).getByRole("gridcell", { name: "Alpha prime" })).toBeVisible();

    // Undo takes it back everywhere.
    await app.press(page.getByRole("button", { name: "Undo" }));
    await expect(app.editor(0).locator(".dbme-mention")).toHaveText("Alpha");
    await expect(app.pane(1).getByRole("gridcell", { name: "Alpha" })).toBeVisible();
  });

  test("outline edits show in the table beside it", async ({ app, page }) => {
    await app.view("Outline", 0);
    const beta = app.pane(0).getByRole("textbox", { name: "Beta, level 1, expanded" });
    await beta.fill("Beta two");
    await expect.poll(() => tableTitles(app, 1)).toEqual(["Alpha", "Beta two", "Gamma"]);
    await app.pane(0).getByRole("textbox", { name: "Gamma, level 1" }).focus();
    await page.keyboard.press("Tab");
    await expect.poll(() => tableTitles(app, 1)).toEqual(["Alpha", "Beta two"]);
    await expect(app.pane(1).getByText("2 rows")).toBeVisible();
  });

  test("two editors on one page stay in step", async ({ app, page }) => {
    await app.view("Document", 1);
    await app.focusEditor(0);
    await page.keyboard.press("ControlOrMeta+End");
    await page.keyboard.press("Enter");
    await page.keyboard.press("Enter");
    await page.keyboard.type("Typed on the left");
    await expect(app.editor(1)).toContainText("Typed on the left");

    await app.focusEditor(1);
    await page.keyboard.press("ControlOrMeta+End");
    await page.keyboard.type(" and the right");
    await expect(app.editor(0)).toContainText("Typed on the left and the right");
  });
});

test("a change in one view shows in the others", async ({ app, page }) => {
  await app.view("Table");
  const title = tableCell(app, 0, 1, 2);
  await app.press(title);
  await app.press(title);
  const input = app.pane(0).getByRole("textbox", { name: "Title" });
  await input.fill("Beta renamed");
  await page.keyboard.press("Enter");
  await expect.poll(() => tableTitles(app, 0)).toEqual(["Alpha", "Beta renamed", "Gamma"]);

  await app.view("Gallery");
  await expect(app.pane(0).getByRole("gridcell", { name: "Beta renamed" })).toBeVisible();
  await app.view("Outline");
  await expect(
    app.pane(0).getByRole("textbox", { name: "Beta renamed, level 1, expanded" }),
  ).toBeVisible();
  await app.press((await app.treeItem("Home")).locator(".dbme-tree__toggle"));
  await expect(await app.treeItem("Beta renamed")).toBeVisible();
});
