// SPDX-License-Identifier: AGPL-3.0-or-later
/** axe (WCAG 2.2 A and AA) on every view, on an empty vault, and with the mention list open. */
import { workspace } from "./data";
import { expect, expectAccessible, test } from "./fixtures";

test("an empty vault passes axe", async ({ app, page }) => {
  await app.open();
  await expect(page.getByRole("heading", { name: "Nothing here yet" })).toBeVisible();
  await expectAccessible(page);
});

test.describe("with a workspace", () => {
  test.beforeEach(async ({ seed, app }) => {
    await seed(workspace());
    await app.open();
  });

  test("the Document view passes axe", async ({ app, page }) => {
    await expect(app.title()).toHaveValue("Home");
    await expect(app.editor().locator(".dbme-mention")).toHaveText("Alpha");
    await expectAccessible(page);
  });

  test("the Document view of a typed object passes axe", async ({ app, page }) => {
    await app.openPage("Home", "Alpha");
    await expect(app.pane().getByRole("button", { name: "Status: Doing" })).toBeVisible();
    await expectAccessible(page);
  });

  test("the Outline view passes axe", async ({ app, page }) => {
    await app.view("Outline");
    await expect(app.pane().getByRole("textbox", { name: "Beta notes, level 2" })).toBeVisible();
    await expectAccessible(page);
  });

  test("the Table view passes axe", async ({ app, page }) => {
    await app.view("Table");
    await expect(app.pane().getByText("3 rows")).toBeVisible();
    await expect(app.pane().getByRole("gridcell", { name: "Gamma", exact: true })).toBeVisible();
    await expectAccessible(page);
  });

  test("the Gallery view passes axe", async ({ app, page }) => {
    await app.view("Gallery");
    await expect(app.pane().locator(".dbme-card img")).toHaveCount(2);
    await expectAccessible(page);
  });

  test("the mention list passes axe", async ({ app, page }) => {
    await app.focusEditor();
    await page.keyboard.press("ControlOrMeta+End");
    await page.keyboard.press("Enter");
    await page.keyboard.press("Enter");
    await page.keyboard.type("[[");
    await expect(page.getByRole("listbox", { name: "Objects to mention" })).toBeVisible();
    await expectAccessible(page);
  });

  test("the split view passes axe", async ({ app, page, isMobile }) => {
    test.skip(isMobile, "the split needs a window 1100 px wide");
    await app.press(page.getByRole("button", { name: "Split view" }));
    await expect(app.pane(1).getByText("3 rows")).toBeVisible();
    await expectAccessible(page);
  });
});
