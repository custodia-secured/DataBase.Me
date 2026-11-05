// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * The Kanban view (M8, Task 4): columns from a select field, moving a card by drag, by keyboard and
 * with the Move to control, its list alternative, and its phone layout.
 */
import type { Locator, Page } from "@playwright/test";
import { workspace } from "./data";
import { expect, expectAccessible, test, type App } from "./fixtures";

function column(app: App, name: string): Locator {
  return app.pane().getByRole("listbox", { name, exact: true });
}

function cards(app: App, name: string): Locator {
  return column(app, name).getByRole("option");
}

async function expectBoard(app: App, board: Record<string, string[]>): Promise<void> {
  for (const [name, titles] of Object.entries(board)) {
    await expect(cards(app, name)).toHaveText(titles);
  }
}

/** Drags with the mouse, and lets go only once the target shows it would take the card. */
async function drag(
  page: Page,
  card: Locator,
  to: { x: number; y: number },
  target: Locator,
): Promise<void> {
  const from = await card.boundingBox();
  if (!from) throw new Error("no box");
  const start = { x: from.x + from.width / 2, y: from.y + from.height / 2 };
  await page.mouse.move(start.x, start.y);
  await page.mouse.down();
  await page.mouse.move(start.x + 12, start.y, { steps: 4 });
  await page.mouse.move(to.x, to.y, { steps: 12 });
  await expect(target).toHaveClass(/is-over/);
  await page.mouse.up();
}

const START = { "To do": ["Beta"], Doing: ["Alpha"], Done: ["Gamma"], "No value": [] };

test.beforeEach(async ({ seed, app }) => {
  await seed(workspace());
  await app.open();
  await app.view("Kanban");
  await expectBoard(app, START);
});

test("the view is reached from More views, and the columns are the field's choices", async ({
  app,
  page,
}) => {
  await expect(app.pane().getByRole("tab", { name: "Kanban" })).toHaveAttribute(
    "aria-selected",
    "true",
  );
  await expect(app.pane().locator(".dbme-kanban__title")).toHaveText([
    "To do",
    "Doing",
    "Done",
    "No value",
  ]);
  await expect(
    app.pane().locator('[data-column="Done"]').locator(".dbme-kanban__head"),
  ).toContainText("1");
  await expectAccessible(page, ".dbme-kanban");
});

test("the keyboard moves the selection and carries a card", async ({ app, page, isMobile }) => {
  test.skip(isMobile, "the phone has no arrow keys; the Move to control is its way");
  await column(app, "To do").focus();
  await expect(cards(app, "To do").first()).toHaveAttribute("aria-selected", "true");
  await page.keyboard.press("ArrowRight");
  await expect(column(app, "Doing")).toBeFocused();
  await expect(cards(app, "Doing").first()).toHaveAttribute("aria-selected", "true");

  // Carried to the next column, and the selection goes with it.
  await page.keyboard.press("Alt+ArrowRight");
  await expectBoard(app, { "To do": ["Beta"], Doing: [], Done: ["Alpha", "Gamma"] });
  await expect(column(app, "Done")).toBeFocused();
  await expect(app.pane().locator(".dbme-kanban__selected").getByRole("status")).toHaveText(
    "Moved Alpha to Done.",
  );
  await expect(cards(app, "Done").filter({ hasText: "Alpha" })).toHaveAttribute(
    "aria-selected",
    "true",
  );

  // Along the column.
  await page.keyboard.press("Alt+ArrowDown");
  await expect(cards(app, "Done")).toHaveText(["Gamma", "Alpha"]);

  // One move is one undo.
  await app.pane().page().keyboard.press("Control+z");
  await expect(cards(app, "Done")).toHaveText(["Alpha", "Gamma"]);
  await app.pane().page().keyboard.press("Control+z");
  await expectBoard(app, START);
});

test("a key with nowhere to go does nothing", async ({ app, page, isMobile }) => {
  test.skip(isMobile, "the phone has no arrow keys");
  await column(app, "To do").focus();
  await page.keyboard.press("ArrowLeft");
  await expect(column(app, "To do")).toBeFocused();
  await page.keyboard.press("Alt+ArrowLeft");
  await expectBoard(app, START);
});

test("Enter opens the selected card", async ({ app, page, isMobile }) => {
  test.skip(isMobile, "the phone taps it");
  await column(app, "Doing").focus();
  await page.keyboard.press("Enter");
  await app.view("Document");
  await expect(app.title()).toHaveValue("Alpha");
});

test("a card can be dragged to another column", async ({ app, page, isMobile }) => {
  test.skip(isMobile, "touch uses the Move to control");
  const target = column(app, "Done");
  const to = await target.boundingBox();
  if (!to) throw new Error("no box");
  await drag(
    page,
    cards(app, "To do").first(),
    { x: to.x + to.width / 2, y: to.y + to.height - 8 },
    app.pane().locator('[data-column="Done"]'),
  );
  await expectBoard(app, { "To do": [], Doing: ["Alpha"], Done: ["Beta", "Gamma"] });
});

test("a card can be dragged along its column", async ({ app, page, isMobile }) => {
  test.skip(isMobile, "touch uses the Move to control");
  await column(app, "Doing").focus();
  await page.keyboard.press("Alt+ArrowRight");
  await expect(cards(app, "Done")).toHaveText(["Alpha", "Gamma"]);
  const alpha = cards(app, "Done").nth(0);
  const to = await alpha.boundingBox();
  if (!to) throw new Error("no box");
  await drag(
    page,
    cards(app, "Done").nth(1),
    { x: to.x + to.width / 2, y: to.y + to.height / 2 },
    alpha,
  );
  await expect(cards(app, "Done")).toHaveText(["Gamma", "Alpha"]);
});

test("the Move to control moves the selected card, at any size", async ({ app, page }) => {
  await app.press(cards(app, "Doing").first());
  const control = app.pane().getByLabel("Move “Alpha” to");
  await control.selectOption({ label: "No value" });
  await expectBoard(app, { Doing: [], "No value": ["Alpha"] });
  await expectAccessible(page, ".dbme-kanban__selected");

  // The value is gone from the page, not set to something else.
  await app.view("Table");
  const row = app
    .pane()
    .getByRole("grid", { name: "Rows" })
    .locator(".dbme-grid__body [role=row]")
    .first();
  await expect(row).toContainText("Alpha");
  await expect(row.locator('[aria-colindex="3"]')).toHaveText("");
});

test("New card adds to its column in one undo", async ({ app, page }) => {
  await app.press(app.pane().getByRole("button", { name: "New card in Doing" }));
  const dialog = page.getByRole("dialog", { name: "New card in Doing" });
  await expect(dialog.getByRole("button", { name: "Add card" })).toBeDisabled();
  await expectAccessible(page, "dialog");
  await dialog.getByLabel("Title").fill("Delta");
  await app.press(dialog.getByRole("button", { name: "Add card" }));
  await expect(dialog).toBeHidden();
  await expect(cards(app, "Doing")).toHaveText(["Alpha", "Delta"]);

  await page.getByRole("button", { name: "Undo" }).click();
  await expect(cards(app, "Doing")).toHaveText(["Alpha"]);
});

test("a filter narrows every column", async ({ app }) => {
  await app.press(app.pane().getByRole("button", { name: /^Filter/ }));
  const dialog = app.page.getByRole("dialog", { name: "Filter" });
  await app.press(dialog.getByRole("button", { name: "Add condition" }));
  await dialog.getByLabel("Field").selectOption({ label: "Title" });
  await dialog.getByLabel("Value").fill("a");
  await dialog.getByLabel("Comparison").selectOption({ label: "starts with" });
  await app.press(dialog.getByRole("button", { name: "Apply" }));
  await expectBoard(app, { "To do": [], Doing: ["Alpha"], Done: [] });
});

test("Show as list is the same cards with their column written out", async ({
  app,
  page,
  isMobile,
}) => {
  await app.pane().getByRole("checkbox", { name: "Show as list" }).check();
  const grid = app.pane().getByRole("grid", { name: "Cards" });
  await expect(grid.getByRole("columnheader")).toHaveText([
    "Title",
    "Status",
    "Due",
    "Estimate",
    "Shipped",
  ]);
  await expect(grid.locator(".dbme-grid__body [role=row]")).toHaveCount(3);
  const alpha = grid.locator(".dbme-grid__body [role=row]").filter({ hasText: "Alpha" });
  await expect(alpha.locator('[aria-colindex="2"]')).toHaveText("Doing");
  await expectAccessible(page, ".dbme-kanban");
  if (!isMobile) {
    await grid.focus();
    await page.keyboard.press("Enter");
    await app.view("Document");
    await expect(app.title()).toHaveValue("Alpha");
  }
});

test("a saved board keeps what it shows", async ({ app }) => {
  await app.pane().getByRole("checkbox", { name: "Show as list" }).check();
  await app.press(app.pane().getByRole("button", { name: "Save view" }));
  const dialog = app.page.getByRole("dialog", { name: "Save view" });
  await dialog.getByLabel("Name").fill("As a list");
  await app.press(dialog.getByRole("button", { name: "Save", exact: true }));
  await expect(dialog).toBeHidden();
  await app.pane().getByLabel("Saved view").selectOption({ label: "None" });
  await expect(column(app, "Doing")).toBeVisible();
  await app.pane().getByLabel("Saved view").selectOption({ label: "As a list" });
  await expect(app.pane().getByRole("grid", { name: "Cards" })).toBeVisible();
});

test("objects with no choice field say what a board needs", async ({ app }) => {
  await app.openPage("Inbox");
  await expect(app.pane().getByText("A board needs a field with choices")).toBeVisible();
});

test("a phone shows one column at a time, a swipe from the next", async ({ app, isMobile }) => {
  test.skip(!isMobile, "the phone layout");
  const board = app.pane().locator(".dbme-kanban__columns");
  const box = await board.boundingBox();
  const first = await app.pane().locator(".dbme-kanban__column").first().boundingBox();
  if (!box || !first) throw new Error("no boxes");
  expect(first.width).toBeGreaterThan(box.width * 0.8);
  expect(first.width).toBeLessThan(box.width);
  await expect(board).toHaveCSS("scroll-snap-type", /x/);
  const wider = await board.evaluate((element) => element.scrollWidth > element.clientWidth);
  expect(wider).toBe(true);
});

test("it holds in dark", async ({ app, page }) => {
  await app.ipc("settings_set", { patch: { theme: "dark" } });
  await page.reload();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
  await app.view("Kanban");
  await expect(cards(app, "Doing")).toHaveText(["Alpha"]);
  await expectAccessible(page, ".dbme-kanban");
});
