// SPDX-License-Identifier: AGPL-3.0-or-later
/** The Table view: a virtualised grid of objects and their fields, one tab stop. */
import type { Locator } from "@playwright/test";
import { workspace } from "./data";
import { expect, test, type App } from "./fixtures";

test.beforeEach(async ({ seed, app }) => {
  await seed(workspace());
  await app.open();
  await app.view("Table");
});

function grid(app: App): Locator {
  return app.pane().getByRole("grid", { name: "Rows" });
}

/** One column's cells, top to bottom: 1 is Open, 2 is Title, then the fields. */
function column(app: App, index: number): Promise<string[]> {
  return grid(app)
    .locator(`.dbme-grid__body [role=row] [aria-colindex="${String(index)}"]`)
    .evaluateAll((cells) =>
      cells.map(
        (cell) =>
          cell.querySelector("[aria-label]")?.getAttribute("aria-label") ?? cell.textContent,
      ),
    );
}

/** The cell at a row (from 0) and column (from 1). */
function cell(app: App, row: number, index: number): Locator {
  return grid(app)
    .locator(".dbme-grid__body [role=row]")
    .nth(row)
    .locator(`[aria-colindex="${String(index)}"]`);
}

test("shows the fields as columns and the children as rows", async ({ app }) => {
  await expect(grid(app).getByRole("columnheader")).toHaveText([
    "Open",
    "Title",
    "Status",
    "Due",
    "Estimate",
    "Shipped",
  ]);
  await expect.poll(() => column(app, 2)).toEqual(["Alpha", "Beta", "Gamma"]);
  await expect.poll(() => column(app, 3)).toEqual(["Doing", "To do", "Done"]);
  await expect(app.pane().getByText("3 rows")).toBeVisible();
});

test("the grid works with the keyboard alone", async ({ app, page }) => {
  const rows = grid(app);
  const active = () => app.activeText(rows);
  await rows.focus();
  await expect.poll(active).toBe("Alpha");

  // F2 edits the cell; Enter saves and moves down.
  await page.keyboard.press("F2");
  await expect(rows.getByRole("textbox", { name: "Title" })).toBeFocused();
  await page.keyboard.press("End");
  await page.keyboard.type(" one");
  await page.keyboard.press("Enter");
  await expect(rows).toBeFocused();
  await expect.poll(active).toBe("Beta");
  await expect.poll(() => column(app, 2)).toEqual(["Alpha one", "Beta", "Gamma"]);

  // Typing starts an edit that replaces the value.
  await page.keyboard.press("ArrowRight");
  await page.keyboard.press("ArrowRight");
  await page.keyboard.press("ArrowRight");
  await expect.poll(active).toBe("8");
  await page.keyboard.type("13");
  await page.keyboard.press("Tab");
  await expect.poll(() => column(app, 5)).toEqual(["3", "13", "1"]);

  // A select browses with the arrow keys and saves on Enter.
  await page.keyboard.press("ArrowLeft");
  await page.keyboard.press("ArrowLeft");
  await page.keyboard.press("ArrowLeft");
  await expect.poll(active).toBe("To do");
  await page.keyboard.press("Enter");
  await expect(rows.getByRole("combobox", { name: "Status" })).toBeFocused();
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("Enter");
  await expect.poll(() => column(app, 3)).toEqual(["Doing", "Doing", "Done"]);

  // Space flips a yes/no cell: End goes to Gamma's last one, Shipped.
  await page.keyboard.press("End");
  await expect.poll(() => column(app, 6)).toEqual(["No", "No", "Yes"]);
  await page.keyboard.press("Space");
  await expect.poll(() => column(app, 6)).toEqual(["No", "No", "No"]);

  // The header row is above the first row; Enter there sorts, then reverses, then unsorts.
  await page.keyboard.press("ControlOrMeta+Home");
  await page.keyboard.press("ArrowUp");
  await page.keyboard.press("ArrowRight");
  const title = rows.getByRole("columnheader", { name: "Title" });
  await page.keyboard.press("Enter");
  await expect(title).toHaveAttribute("aria-sort", "ascending");
  await page.keyboard.press("Enter");
  await expect(title).toHaveAttribute("aria-sort", "descending");
  await expect.poll(() => column(app, 2)).toEqual(["Gamma", "Beta", "Alpha one"]);

  // Ctrl+Enter opens the row's object.
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("ControlOrMeta+Enter");
  await expect(app.currentCrumb()).toHaveText("Gamma");
});

test("cells edit by touch or pointer, and New row adds an object", async ({ app, page }) => {
  const estimate = cell(app, 0, 5);
  await app.press(estimate);
  await app.press(estimate);
  const input = grid(app).getByRole("spinbutton", { name: "Estimate" });
  await expect(input).toBeFocused();
  await input.fill("21");
  await page.keyboard.press("Enter");
  await expect.poll(() => column(app, 5)).toEqual(["21", "8", "1"]);

  await app.press(app.pane().getByRole("button", { name: "New row" }));
  const name = grid(app).getByRole("textbox", { name: "Title" });
  await expect(name).toBeFocused();
  await name.fill("Delta");
  await page.keyboard.press("Enter");
  await expect(app.pane().getByText("4 rows")).toBeVisible();
  await expect.poll(() => column(app, 2)).toEqual(["Alpha", "Beta", "Gamma", "Delta"]);

  const open = cell(app, 1, 1);
  await expect(open).toHaveText("Open Beta");
  await app.press(open);
  await app.press(open);
  await expect(app.currentCrumb()).toHaveText("Beta");
});

test("the source picker lists every object of a type", async ({ app }) => {
  await app.openPage("Inbox");
  await expect.poll(() => column(app, 2)).toEqual(["Idea one", "Idea two"]);
  await app.pane().getByRole("combobox", { name: "Show" }).selectOption({ label: "All Milestone" });
  await expect.poll(() => column(app, 2)).toEqual(["Alpha", "Beta", "Gamma"]);
});
