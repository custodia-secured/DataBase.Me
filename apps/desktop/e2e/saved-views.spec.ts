// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * The bar the views share (M8, Task 3): the filter editor and saved views, on the Table and Gallery.
 * A filter narrows what a view lists, a saved view brings it back, and neither touches the objects.
 */
import type { Locator } from "@playwright/test";
import { workspace } from "./data";
import { expect, expectAccessible, test, type App } from "./fixtures";

function titles(app: App): Promise<string[]> {
  return app
    .pane()
    .getByRole("grid", { name: "Rows" })
    .locator('.dbme-grid__body [role=row] [aria-colindex="2"]')
    .allTextContents();
}

async function expectTitles(app: App, expected: string[]): Promise<void> {
  await expect.poll(() => titles(app)).toEqual(expected);
}

function bar(app: App): Locator {
  return app.pane();
}

function filterButton(app: App): Locator {
  return bar(app).getByRole("button", { name: /^Filter/ });
}

function filterDialog(app: App): Locator {
  return app.page.getByRole("dialog", { name: "Filter" });
}

/** Adds one condition to the open filter dialog; a null value is for a comparison with none. */
async function addCondition(
  app: App,
  field: string,
  comparison: string,
  value: string | null,
): Promise<void> {
  const dialog = filterDialog(app);
  await app.press(dialog.getByRole("button", { name: "Add condition" }));
  const row = dialog.getByRole("group").last();
  await row.getByLabel("Field").selectOption({ label: field });
  await row.getByLabel("Comparison").selectOption({ label: comparison });
  if (value === null) return;
  const input = row.getByLabel("Value");
  if ((await input.evaluate((element) => element.tagName)) === "SELECT") {
    await input.selectOption({ label: value });
  } else {
    await input.fill(value);
  }
}

async function apply(app: App): Promise<void> {
  await app.press(filterDialog(app).getByRole("button", { name: "Apply" }));
  await expect(filterDialog(app)).toBeHidden();
}

async function saveAs(app: App, name: string): Promise<void> {
  await app.press(bar(app).getByRole("button", { name: "Save view" }));
  const dialog = app.page.getByRole("dialog", { name: "Save view" });
  await dialog.getByLabel("Name").fill(name);
  await app.press(dialog.getByRole("button", { name: "Save", exact: true }));
  await expect(dialog).toBeHidden();
}

function picker(app: App): Locator {
  return bar(app).getByLabel("Saved view");
}

test.describe("on the Table", () => {
  test.beforeEach(async ({ seed, app }) => {
    await seed(workspace());
    await app.open();
    await app.view("Table");
    await expectTitles(app, ["Alpha", "Beta", "Gamma"]);
  });

  test("a filter narrows the rows, and clearing it brings them back", async ({ app, page }) => {
    await app.press(filterButton(app));
    await expect(filterDialog(app).getByText("Nothing is filtered out")).toBeVisible();
    await addCondition(app, "Status", "is", "Done");
    await expectAccessible(page, "dialog");
    await apply(app);
    await expect(filterButton(app)).toHaveText("Filter (1)");
    await expectTitles(app, ["Gamma"]);
    await expect(bar(app).getByText("1 row", { exact: true })).toBeVisible();

    // The dialog opens on what is applied.
    await app.press(filterButton(app));
    const row = filterDialog(app).getByRole("group");
    await expect(row.getByLabel("Field")).toHaveValue("status");
    await expect(row.getByLabel("Comparison")).toHaveValue("eq");
    await expect(row.getByLabel("Value")).toHaveValue("done");
    await app.press(row.getByRole("button", { name: "Remove condition 1" }));
    await apply(app);
    await expect(filterButton(app)).toHaveText("Filter");
    await expectTitles(app, ["Alpha", "Beta", "Gamma"]);
  });

  test("conditions all hold, on any kind of field", async ({ app }) => {
    await app.press(filterButton(app));
    await addCondition(app, "Estimate", "≥", "2");
    await addCondition(app, "Title", "contains", "et");
    await apply(app);
    await expect(filterButton(app)).toHaveText("Filter (2)");
    await expectTitles(app, ["Beta"]);

    await app.press(filterButton(app));
    await app.press(filterDialog(app).getByRole("button", { name: "Remove condition 2" }));
    await addCondition(app, "Shipped", "is", "No");
    await apply(app);
    await expectTitles(app, ["Alpha", "Beta"]);

    await app.press(filterButton(app));
    await app.press(filterDialog(app).getByRole("button", { name: "Remove condition 2" }));
    await app.press(filterDialog(app).getByRole("button", { name: "Remove condition 1" }));
    await addCondition(app, "Due", "is empty", null);
    await apply(app);
    await expectTitles(app, ["Beta", "Gamma"]);
  });

  test("a value that does not fit stops the filter and says why", async ({ app }) => {
    await app.press(filterButton(app));
    await addCondition(app, "Estimate", "<", "lots");
    await app.press(filterDialog(app).getByRole("button", { name: "Apply" }));
    await expect(filterDialog(app).getByText("Enter a number")).toBeVisible();
    await expect(filterDialog(app).getByLabel("Value")).toHaveAttribute("aria-invalid", "true");
    await filterDialog(app).getByLabel("Value").fill("2");
    await expect(filterDialog(app).getByText("Enter a number")).toBeHidden();
    await apply(app);
    await expectTitles(app, ["Gamma"]);
  });

  test("a filter does not change a thing in the vault", async ({ app, page }) => {
    await app.press(filterButton(app));
    await addCondition(app, "Status", "is", "Done");
    await apply(app);
    await expectTitles(app, ["Gamma"]);
    await page.reload();
    await app.view("Table");
    await expect(filterButton(app)).toHaveText("Filter");
    await expectTitles(app, ["Alpha", "Beta", "Gamma"]);
  });

  test("a saved view brings its filter and order back, and survives a reload", async ({
    app,
    page,
  }) => {
    await expect(picker(app)).toBeHidden();
    await app.press(filterButton(app));
    await addCondition(app, "Shipped", "is", "No");
    await apply(app);
    await app.press(
      app.pane().getByRole("grid", { name: "Rows" }).getByRole("columnheader", { name: "Title" }),
    );
    await saveAs(app, "Not shipped");
    await expect(picker(app)).toHaveValue(/.+/);
    await expectAccessible(page, ".dbme-view-bar");

    await picker(app).selectOption({ label: "None" });
    await expect(filterButton(app)).toHaveText("Filter");
    await expectTitles(app, ["Alpha", "Beta", "Gamma"]);

    await page.reload();
    await app.view("Table");
    await picker(app).selectOption({ label: "Not shipped" });
    await expect(filterButton(app)).toHaveText("Filter (1)");
    await expectTitles(app, ["Alpha", "Beta"]);
    await expect(
      app.pane().getByRole("grid", { name: "Rows" }).getByRole("columnheader", { name: "Title" }),
    ).toHaveAttribute("aria-sort", "ascending");
  });

  test("a saved view can be changed and deleted", async ({ app }) => {
    await app.press(filterButton(app));
    await addCondition(app, "Status", "is", "Done");
    await apply(app);
    await saveAs(app, "Finished");

    await app.press(filterButton(app));
    await filterDialog(app).getByLabel("Value").selectOption({ label: "Doing" });
    await apply(app);
    await expectTitles(app, ["Alpha"]);
    await app.press(bar(app).getByRole("button", { name: "Update view" }));
    await picker(app).selectOption({ label: "None" });
    await expectTitles(app, ["Alpha", "Beta", "Gamma"]);
    await picker(app).selectOption({ label: "Finished" });
    await expectTitles(app, ["Alpha"]);

    await app.press(bar(app).getByRole("button", { name: "Delete view" }));
    await expect(picker(app)).toBeHidden();
    await expect(bar(app).getByRole("button", { name: "Update view" })).toBeHidden();
    await expect(filterButton(app)).toHaveText("Filter");
    await expectTitles(app, ["Alpha", "Beta", "Gamma"]);
  });

  test("a saved view belongs to its object and its view", async ({ app }) => {
    await saveAs(app, "Everything here");
    await expect(picker(app)).toBeVisible();
    await app.view("Gallery");
    await expect(picker(app)).toBeHidden();
    await app.view("Table");
    await expect(picker(app)).toBeVisible();
    await app.openPage("Inbox");
    await app.view("Table");
    await expect(picker(app)).toBeHidden();
  });

  test("a saved view stays on its object, out of the sidebar's saved searches", async ({ app }) => {
    await saveAs(app, "Everything here");
    await expect(picker(app)).toBeVisible();
    const sidebar = await app.sidebar();
    await expect(sidebar.getByRole("region", { name: "Saved views" })).toBeHidden();
  });

  test("changing what the Table shows clears a filter from the last source", async ({ app }) => {
    await app.press(filterButton(app));
    await addCondition(app, "Status", "is", "Done");
    await apply(app);
    await bar(app).getByLabel("Show").selectOption({ label: "All Milestone" });
    await expect(filterButton(app)).toHaveText("Filter");
    await expectTitles(app, ["Alpha", "Beta", "Gamma"]);
  });
});

test.describe("on the Gallery", () => {
  test.beforeEach(async ({ seed, app }) => {
    await seed(workspace());
    await app.open();
    await app.view("Gallery");
  });

  function cards(app: App): Locator {
    return app.pane().getByRole("gridcell");
  }

  test("a filter and an order are saved together", async ({ app, page }) => {
    await expect(cards(app)).toHaveCount(3);
    await app.press(filterButton(app));
    await addCondition(app, "Title", "contains", "a");
    await addCondition(app, "Estimate", "<", "5");
    await apply(app);
    await expect(cards(app)).toHaveCount(2);
    await bar(app).getByLabel("Order").selectOption({ label: "Title" });
    await saveAs(app, "Small ones");
    await expectAccessible(page, ".dbme-view-bar");

    await picker(app).selectOption({ label: "None" });
    await expect(cards(app)).toHaveCount(3);
    await expect(bar(app).getByLabel("Order")).toHaveValue("0");

    await picker(app).selectOption({ label: "Small ones" });
    await expect(cards(app)).toHaveCount(2);
    await expect(bar(app).getByLabel("Order")).toHaveValue("1");
    await expect(cards(app).first()).toContainText("Alpha");
  });
});

test.describe("in Simple mode", () => {
  test("there is no filter and no saved view to reach", async ({ seed, app }) => {
    await seed(workspace());
    await app.open({ mode: "simple" });
    await app.view("Gallery");
    await expect(app.pane().getByRole("gridcell")).toHaveCount(3);
    await expect(filterButton(app)).toBeHidden();
    await expect(bar(app).getByRole("button", { name: "Save view" })).toBeHidden();
  });
});
