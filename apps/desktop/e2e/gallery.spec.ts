// SPDX-License-Identifier: AGPL-3.0-or-later
/** The Gallery view: objects as cards with their first image as the cover. */
import type { Locator } from "@playwright/test";
import { workspace } from "./data";
import { expect, test, type App } from "./fixtures";

test.beforeEach(async ({ seed, app }) => {
  await seed(workspace());
  await app.open();
  await app.view("Gallery");
});

function cards(app: App): Locator {
  return app.pane().getByRole("grid", { name: "Cards" });
}

test("shows a card per object, with covers from attached images", async ({ app }) => {
  await expect(cards(app).getByRole("gridcell")).toHaveCount(3);
  for (const [title, covered] of [
    ["Alpha", true],
    ["Beta", true],
    ["Gamma", false],
  ] as const) {
    const card = cards(app).getByRole("gridcell", { name: title });
    await expect(card).toBeVisible();
    if (covered) {
      // Covers are read over IPC and shown through blob: URLs, which the CSP allows.
      await expect(card.locator("img")).toHaveAttribute("src", /^blob:/);
      await expect
        .poll(() => card.locator("img").evaluate((img) => (img as HTMLImageElement).naturalWidth))
        .toBe(64);
    } else {
      await expect(card.locator("img")).toHaveCount(0);
    }
  }
});

test("the cards work with the keyboard alone", async ({ app, page }) => {
  const grid = cards(app);
  await grid.focus();
  const active = () => app.activeText(grid);
  await expect.poll(active).toBe("Alpha");
  await page.keyboard.press("ArrowRight");
  await expect.poll(active).toBe("Beta");
  await page.keyboard.press("ControlOrMeta+End");
  await expect.poll(active).toBe("Gamma");
  await page.keyboard.press("ControlOrMeta+Home");
  await expect.poll(active).toBe("Alpha");
  await page.keyboard.press("ArrowRight");
  await page.keyboard.press("Enter");
  await expect(app.currentCrumb()).toHaveText("Beta");
});

test("a card opens on its second tap or click", async ({ app }) => {
  const gamma = cards(app).getByRole("gridcell", { name: "Gamma" });
  await app.press(gamma);
  await expect.poll(() => app.activeText(cards(app))).toBe("Gamma");
  await expect(app.currentCrumb()).toHaveText("Home");
  await app.press(gamma);
  await expect(app.currentCrumb()).toHaveText("Gamma");
});

test("the source picker lists every object of a type", async ({ app }) => {
  await app.openPage("Inbox");
  await expect.poll(() => names(app)).toEqual(["Idea one", "Idea two"]);
  await app.pane().getByRole("combobox", { name: "Show" }).selectOption({ label: "All Milestone" });
  await expect.poll(() => names(app)).toEqual(["Alpha", "Beta", "Gamma"]);
});

function names(app: App): Promise<string[]> {
  return cards(app)
    .locator(".dbme-card__title")
    .evaluateAll((titles) => titles.map((title) => title.textContent));
}
