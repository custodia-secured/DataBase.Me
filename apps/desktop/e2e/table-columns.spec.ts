// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * The Table's columns (ADR-0030), made through the UI. The first test seeds nothing, so that its
 * columns are ones a person can make: the seed writes fields straight to the store, which is how
 * the Table's other tests passed while the app had no way to add a column (#15).
 */
import type { Locator } from "@playwright/test";
import { workspace } from "./data";
import { expect, expectAccessible, test, type App } from "./fixtures";

interface Summary {
  object: { id: string; title: string };
  type_ids: string[];
}

function grid(app: App): Locator {
  return app.pane().getByRole("grid", { name: "Rows" });
}

function headers(app: App): Locator {
  return grid(app).getByRole("columnheader");
}

/** The cell at a row (from 0) and column (from 1). */
function cell(app: App, row: number, index: number): Locator {
  return grid(app)
    .locator(".dbme-grid__body [role=row]")
    .nth(row)
    .locator(`[aria-colindex="${String(index)}"]`);
}

/** One column's text, top to bottom. */
function column(app: App, index: number): Promise<string[]> {
  return grid(app)
    .locator(`.dbme-grid__body [role=row] [aria-colindex="${String(index)}"]`)
    .allTextContents();
}

/** Types a value into a cell, as a person does: choose it, start editing, type, Enter. */
async function fillCell(app: App, row: number, index: number, text: string): Promise<void> {
  await app.press(cell(app, row, index));
  await app.press(cell(app, row, index));
  await grid(app).locator(".dbme-field-editor input").fill(text);
  await grid(app).locator(".dbme-field-editor input").press("Enter");
}

function dialog(app: App, name: string): Locator {
  return app.page.getByRole("dialog", { name });
}

/** Adds a column through the dialog, and waits for it to close. */
async function addColumn(
  app: App,
  name: string,
  holds?: { kind: string; choices?: string },
): Promise<void> {
  await app.press(app.pane().getByRole("button", { name: "New column" }));
  const form = dialog(app, "New column");
  await form.getByRole("textbox", { name: "Name" }).fill(name);
  if (holds) {
    await form.getByRole("combobox", { name: "Holds" }).selectOption({ label: holds.kind });
    if (holds.choices !== undefined) {
      await form.getByRole("textbox", { name: "Choices" }).fill(holds.choices);
    }
  }
  await app.press(form.getByRole("button", { name: "Add column" }));
  await expect(form).toBeHidden();
}

test("columns are made from an empty vault, with nothing seeded", async ({ app, page }) => {
  await app.open();
  await app.press(page.getByRole("button", { name: "New page" }).first());
  await app.title().fill("Infrastructure");
  await app.title().press("Enter");
  await expect(app.currentCrumb()).toHaveText("Infrastructure");

  await app.view("Table");
  await expect(headers(app)).toHaveText(["Open", "Title"]);
  await expect(app.pane().getByText("0 rows")).toBeVisible();

  // An empty Table: the column needs a type and a row to show on, and the dialog says so.
  await app.press(app.pane().getByRole("button", { name: "New column" }));
  await expect(dialog(app, "New column")).toContainText(
    "The pages here get a new type, Infrastructure, to hold this column. It also adds a first row.",
  );
  await app.press(dialog(app, "New column").getByRole("button", { name: "Cancel" }));
  await addColumn(app, "Serial number");
  await expect(headers(app)).toHaveText(["Open", "Title", "Serial number"]);
  await expect(app.pane().getByText("1 row", { exact: true })).toBeVisible();

  await fillCell(app, 0, 3, "X-1");
  await expect.poll(() => column(app, 3)).toEqual(["X-1"]);

  // The next row is of the same type, so the column is there for it.
  await app.press(app.pane().getByRole("button", { name: "New row" }));
  const name = grid(app).getByRole("textbox", { name: "Title" });
  await expect(name).toBeFocused();
  await name.fill("Second");
  await name.press("Enter");
  await expect(app.pane().getByText("2 rows")).toBeVisible();
  await fillCell(app, 1, 3, "X-2");
  await expect.poll(() => column(app, 3)).toEqual(["X-1", "X-2"]);

  // A column of another kind, whose cells edit as that kind.
  await addColumn(app, "Status", { kind: "Choice from a list", choices: "To do, Done" });
  await expect(headers(app)).toHaveText(["Open", "Title", "Serial number", "Status"]);
  await app.press(cell(app, 0, 4));
  await app.press(cell(app, 0, 4));
  await grid(app).getByRole("combobox", { name: "Status" }).selectOption({ label: "Done" });
  await expect.poll(() => column(app, 4)).toEqual(["Done", ""]);

  // Adding a column is one undo step: the cell, then the column.
  await app.press(page.getByRole("button", { name: "Undo" }));
  await expect.poll(() => column(app, 4)).toEqual(["", ""]);
  await app.press(page.getByRole("button", { name: "Undo" }));
  await expect(headers(app)).toHaveText(["Open", "Title", "Serial number"]);
  await expect.poll(() => column(app, 3)).toEqual(["X-1", "X-2"]);
});

test.describe("with a workspace", () => {
  test.beforeEach(async ({ seed, app }) => {
    await seed(workspace());
    await app.open();
  });

  test("pages with no type all get the new type", async ({ app }) => {
    await app.openPage("Inbox");
    await app.view("Table");
    await expect(headers(app)).toHaveText(["Open", "Title"]);

    await addColumn(app, "Owner");
    await expect(headers(app)).toHaveText(["Open", "Title", "Owner"]);
    await expect.poll(() => column(app, 2)).toEqual(["Idea one", "Idea two"]);

    const types = await app.ipc<{ id: string; name: string }[]>("types_list");
    const inbox = types.find((type) => type.name === "Inbox")?.id;
    expect(inbox).toBeDefined();
    const page = await app.ipc<Summary[]>("objects_page", {
      source: { kind: "type", type_id: inbox },
      sort: { by: "manual", field: null, descending: false },
      offset: 0,
      limit: 10,
    });
    expect(page.map((row) => row.object.title)).toEqual(["Idea one", "Idea two"]);

    // The type is one Show can now choose.
    await app.pane().getByRole("combobox", { name: "Show" }).selectOption({ label: "All Inbox" });
    await expect.poll(() => column(app, 2)).toEqual(["Idea one", "Idea two"]);
  });

  test("a column of a shared type goes on it, and a new row takes that type", async ({ app }) => {
    await app.view("Table");
    await app.press(app.pane().getByRole("button", { name: "New column" }));
    await expect(dialog(app, "New column")).toContainText(
      "This column goes on the type Milestone, so every page of that type gets it.",
    );
    await app.press(dialog(app, "New column").getByRole("button", { name: "Cancel" }));

    await addColumn(app, "Owner");
    await expect(headers(app)).toHaveText([
      "Open",
      "Title",
      "Status",
      "Due",
      "Estimate",
      "Shipped",
      "Owner",
    ]);

    await app.press(app.pane().getByRole("button", { name: "New row" }));
    const name = grid(app).getByRole("textbox", { name: "Title" });
    await name.fill("Delta");
    await name.press("Enter");
    await expect(app.pane().getByText("4 rows")).toBeVisible();
    const found = await app.ipc<Summary[]>("object_search", { text: "Delta", limit: 1 });
    expect(found[0]?.type_ids).toHaveLength(1);
  });

  test("a Table of a type says so and adds to it", async ({ app }) => {
    await app.openPage("Inbox");
    await app.view("Table");
    await app
      .pane()
      .getByRole("combobox", { name: "Show" })
      .selectOption({ label: "All Milestone" });
    await expect.poll(() => column(app, 2)).toEqual(["Alpha", "Beta", "Gamma"]);

    await app.press(app.pane().getByRole("button", { name: "New column" }));
    await expect(dialog(app, "New column")).toContainText(
      "Every page of the type Milestone gets this column.",
    );
    await app.press(dialog(app, "New column").getByRole("button", { name: "Cancel" }));
    await addColumn(app, "Owner");
    await expect(headers(app).last()).toHaveText("Owner");
  });

  test("a name in use or a list with no choices is not accepted", async ({ app }) => {
    await app.view("Table");
    await app.press(app.pane().getByRole("button", { name: "New column" }));
    const form = dialog(app, "New column");
    const add = form.getByRole("button", { name: "Add column" });
    await expect(add).toBeDisabled();

    await form.getByRole("textbox", { name: "Name" }).fill(" status ");
    await app.press(add);
    await expect(form.getByRole("alert")).toContainText("already a column called “status”");
    await expect(form).toBeVisible();

    await form.getByRole("textbox", { name: "Name" }).fill("title");
    await app.press(add);
    await expect(form.getByRole("alert")).toContainText("already a column called “title”");

    await form.getByRole("textbox", { name: "Name" }).fill("Stage");
    await form
      .getByRole("combobox", { name: "Holds" })
      .selectOption({ label: "Choice from a list" });
    await expect(add).toBeDisabled();
    await form.getByRole("textbox", { name: "Choices" }).fill(" , ");
    await expect(add).toBeDisabled();
    await form.getByRole("textbox", { name: "Choices" }).fill("Early, Late");
    await expect(add).toBeEnabled();

    // Cancel forgets what was typed.
    await app.press(form.getByRole("button", { name: "Cancel" }));
    await expect(form).toBeHidden();
    await app.press(app.pane().getByRole("button", { name: "New column" }));
    await expect(form.getByRole("textbox", { name: "Name" })).toHaveValue("");
    await expect(form.getByRole("textbox", { name: "Choices" })).toBeHidden();
  });

  test("a column is renamed and deleted, and its values stay", async ({ app, page }) => {
    await app.view("Table");
    await app.press(app.pane().getByRole("button", { name: "Edit columns" }));
    const edit = dialog(app, "Edit columns");

    const estimate = edit.getByRole("textbox", { name: "Name of the Estimate column" });
    await estimate.fill("Points");
    await estimate.press("Enter");
    await expect(headers(app).nth(4)).toHaveText("Points", { useInnerText: false });
    await expect(edit.getByRole("textbox", { name: "Name of the Points column" })).toBeFocused();

    // A name already in use is refused, and the input goes back.
    const points = edit.getByRole("textbox", { name: "Name of the Points column" });
    await points.fill("status");
    await points.press("Enter");
    await expect(points).toHaveValue("Points");

    // Escape closes the dialog without saving what was typed.
    await points.fill("Effort");
    await points.press("Escape");
    await expect(edit).toBeHidden();
    await expect(headers(app).nth(4)).toHaveText("Points");
    await app.press(app.pane().getByRole("button", { name: "Edit columns" }));
    await expect(points).toHaveValue("Points");

    await app.press(edit.getByRole("button", { name: "Delete the Shipped column" }));
    await expect(edit.getByRole("listitem")).toHaveCount(3);
    await app.press(edit.getByRole("button", { name: "Done" }));
    await expect(edit).toBeHidden();

    await expect(headers(app)).toHaveText(["Open", "Title", "Status", "Due", "Points"]);
    await expect.poll(() => column(app, 5)).toEqual(["3", "8", "1"]);

    // Undo brings the column back, with the values it had.
    await app.press(page.getByRole("button", { name: "Undo" }));
    await expect(headers(app).last()).toHaveText("Shipped");
    await expect
      .poll(() => cell(app, 2, 6).locator("[aria-label]").getAttribute("aria-label"))
      .toBe("Yes");
  });

  test("a column is added by keyboard alone", async ({ app, page }) => {
    await app.view("Table");
    await app.pane().getByRole("button", { name: "New column" }).focus();
    await page.keyboard.press("Enter");
    const form = dialog(app, "New column");
    await expect(form.getByRole("textbox", { name: "Name" })).toBeFocused();

    await page.keyboard.type("Owner");
    await page.keyboard.press("Tab");
    await expect(form.getByRole("combobox", { name: "Holds" })).toBeFocused();
    await page.keyboard.press("Shift+Tab");
    await page.keyboard.press("Enter");
    await expect(form).toBeHidden();
    await expect(headers(app).last()).toHaveText("Owner");

    // Escape closes without adding anything, and focus comes back to the button.
    await app.pane().getByRole("button", { name: "New column" }).focus();
    await page.keyboard.press("Enter");
    await page.keyboard.type("Nothing");
    await page.keyboard.press("Escape");
    await expect(form).toBeHidden();
    await expect(app.pane().getByRole("button", { name: "New column" })).toBeFocused();
    await expect(headers(app).last()).toHaveText("Owner");

    // The new column is on the grid's keyboard path.
    await grid(app).focus();
    await page.keyboard.press("End");
    await page.keyboard.type("Sam");
    await page.keyboard.press("Enter");
    await expect.poll(() => cell(app, 0, 7).textContent()).toBe("Sam");
  });

  test("the column dialogs pass axe", async ({ app, page }) => {
    await app.view("Table");
    await app.press(app.pane().getByRole("button", { name: "New column" }));
    const form = dialog(app, "New column");
    await form
      .getByRole("combobox", { name: "Holds" })
      .selectOption({ label: "Choice from a list" });
    await expect(form.getByRole("textbox", { name: "Choices" })).toBeVisible();
    await expectAccessible(page);
    await app.press(form.getByRole("button", { name: "Cancel" }));

    await app.press(app.pane().getByRole("button", { name: "Edit columns" }));
    await expect(dialog(app, "Edit columns").getByRole("listitem")).toHaveCount(4);
    await expectAccessible(page);
  });
});
