// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * Themes (M7, ADR-0027): the setting puts `data-theme` on <html> and the tokens follow; "system"
 * leaves it to the OS; the choice survives a reload with no flash of the other theme; and the
 * dark palette passes axe on the Document and Table views.
 */
import type { Page } from "@playwright/test";
import { workspace } from "./data";
import { expect, expectAccessible, test, type App } from "./fixtures";

const LIGHT_BG = "rgb(255, 255, 255)";
const DARK_BG = "rgb(20, 23, 27)";

/** The page background as painted, which the background token decides. */
async function background(page: Page): Promise<string> {
  return page.evaluate(() => getComputedStyle(document.body).backgroundColor);
}

async function runCommand(app: App, label: string) {
  if (app.phone) await app.press(app.page.getByRole("button", { name: "Search", exact: true }));
  else await app.page.keyboard.press("Control+k");
  const box = app.page.getByRole("combobox", { name: "Search or type > for a command" });
  await expect(box).toBeFocused();
  await box.fill(`> ${label}`);
  const option = app.page.getByRole("option", { name: label });
  await expect(option).toBeVisible();
  await app.press(option);
}

test.beforeEach(async ({ seed, app }) => {
  await seed(workspace());
  await app.open();
});

test("the theme follows the setting, the OS under system, and survives a reload", async ({
  app,
  page,
}) => {
  await expect(page.locator("html")).not.toHaveAttribute("data-theme", /./);
  expect(await background(page)).toBe(LIGHT_BG);

  await runCommand(app, "Theme: dark");
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
  expect(await background(page)).toBe(DARK_BG);
  const settings = await app.ipc<{ theme: string }>("settings_get");
  expect(settings.theme).toBe("dark");

  // The reload paints dark from the first frame: the hint is read before the UI mounts.
  await page.reload();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
  await app.open();
  expect(await background(page)).toBe(DARK_BG);

  // Light wins over a dark OS; system follows it.
  await page.emulateMedia({ colorScheme: "dark" });
  await runCommand(app, "Theme: light");
  await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
  expect(await background(page)).toBe(LIGHT_BG);
  await runCommand(app, "Theme: follow the system");
  await expect(page.locator("html")).not.toHaveAttribute("data-theme", /./);
  expect(await background(page)).toBe(DARK_BG);
  await page.emulateMedia({ colorScheme: "light" });
  expect(await background(page)).toBe(LIGHT_BG);
});

test("the dark palette passes axe on the Document and Table views", async ({ app, page }) => {
  await runCommand(app, "Theme: dark");
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
  await expect(app.title()).toHaveValue("Home");
  await expectAccessible(page);
  await app.view("Table");
  await expect(app.pane().getByText("3 rows")).toBeVisible();
  await expectAccessible(page);
});
