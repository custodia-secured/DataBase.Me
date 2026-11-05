// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * The Dashboard view (M8, Task 8): saved charts and views as tiles, how they are arranged by keyboard and
 * touch, one column on a phone, what a saved dashboard keeps, and a failure staying in its tile.
 */
import type { Locator } from "@playwright/test";
import { expect, expectAccessible, test, type App } from "./fixtures";

function plan(): unknown {
  const task = (title: string, props: Record<string, unknown>) => ({
    title,
    types: ["Task"],
    props,
  });
  return {
    kind: "tree",
    types: [
      {
        name: "Task",
        fields: [
          {
            key: "status",
            label: "Status",
            kind: "select",
            options: {
              options: [
                { id: "todo", label: "To do" },
                { id: "done", label: "Done" },
              ],
            },
          },
          { key: "estimate", label: "Estimate", kind: "number" },
        ],
      },
    ],
    objects: [
      {
        title: "Plan",
        children: [
          task("Alpha", { status: "done", estimate: 3 }),
          task("Beta", { status: "todo", estimate: 5 }),
        ],
      },
    ],
  };
}

/** Saves a view on the page, as Save view would, to give the dashboard something to show. */
async function save(
  app: App,
  owner: string,
  name: string,
  renderer: string,
  options: Record<string, unknown>,
): Promise<string> {
  return app.ipc<string>("view_save", {
    view: {
      name,
      renderer,
      query: {
        where: [{ field: "$parent_id", op: "eq", value: owner }],
        sort: { by: "manual", field: null, descending: false },
      },
      config: { options },
      owner_object_id: owner,
    },
  });
}

const tile = (app: App, name: string): Locator => app.pane().getByRole("region", { name });
const tiles = (app: App): Locator => app.pane().locator(".dbme-tile__title");
const add = async (app: App, label: string): Promise<void> => {
  await app.press(app.pane().getByRole("button", { name: "Add tile" }));
  const dialog = app.page.getByRole("dialog", { name: "Add tile" });
  await dialog.getByLabel("Saved chart or view").selectOption({ label });
  await app.press(dialog.getByRole("button", { name: "Add", exact: true }));
  await expect(dialog).toBeHidden();
};
const menu = async (app: App, name: string, item: string): Promise<void> => {
  await app.press(tile(app, name).getByRole("button", { name: `Change tile ${name}` }));
  await app.press(app.page.getByRole("menuitem", { name: item }));
};

test.describe("with saved charts and views", () => {
  let owner = "";

  test.beforeEach(async ({ seed, app }) => {
    await seed(plan());
    const [found] = await app.ipc<{ object: { id: string } }[]>("object_search", {
      text: "Plan",
      limit: 1,
    });
    if (!found) throw new Error("no page");
    owner = found.object.id;
    await save(app, owner, "By status", "charts", { x: "status" });
    await save(app, owner, "Effort", "charts", { x: "status", measure: "sum", of: "estimate" });
    await save(app, owner, "All tasks", "table", {});
    await app.open();
    await app.view("Dashboard");
    await expect(app.pane().getByText("A dashboard shows saved charts")).toBeVisible();
  });

  test("a saved chart shows as its chart, and any other view as a list", async ({ app, page }) => {
    await add(app, "By status (Chart)");
    await add(app, "All tasks (Table)");
    await expect(tile(app, "By status").locator(".dbme-charts__figure svg")).toBeVisible();
    await expect(tile(app, "By status").locator(".dbme-charts__figure")).toHaveAttribute(
      "aria-label",
      /^Count by Status/,
    );
    const rows = tile(app, "All tasks").locator(".dbme-grid__body [role=row]");
    await expect(rows).toHaveCount(2);
    await expect(rows.first()).toContainText("Alpha");
    await expectAccessible(page, ".dbme-dashboard");
  });

  test("what is already a tile is not offered again", async ({ app, page }) => {
    await add(app, "By status (Chart)");
    await app.press(app.pane().getByRole("button", { name: "Add tile" }));
    const dialog = page.getByRole("dialog", { name: "Add tile" });
    await expect(dialog.getByLabel("Saved chart or view").locator("option")).toHaveText([
      "All tasks (Table)",
      "Effort (Chart)",
    ]);
    await expectAccessible(page, "dialog");
  });

  test("tiles move and widen from their menus", async ({ app, isMobile }) => {
    await add(app, "By status (Chart)");
    await add(app, "Effort (Chart)");
    await add(app, "All tasks (Table)");
    await expect(tiles(app)).toHaveText(["By status", "Effort", "All tasks"]);

    await menu(app, "All tasks", "Move earlier");
    await expect(tiles(app)).toHaveText(["By status", "All tasks", "Effort"]);
    await menu(app, "By status", "Move later");
    await expect(tiles(app)).toHaveText(["All tasks", "By status", "Effort"]);

    await app.press(tile(app, "All tasks").getByRole("button", { name: "Change tile All tasks" }));
    await expect(app.page.getByRole("menuitem", { name: "Move earlier" })).toHaveAttribute(
      "aria-disabled",
      "true",
    );
    await app.page.keyboard.press("Escape");

    const before = await tile(app, "Effort").boundingBox();
    await menu(app, "Effort", "Make wider");
    await expect(tile(app, "Effort")).toHaveClass(/is-wide/);
    const after = await tile(app, "Effort").boundingBox();
    if (!before || !after) throw new Error("no boxes");
    if (!isMobile) expect(after.width).toBeGreaterThan(before.width * 1.5);
    await menu(app, "Effort", "Make narrower");
    await expect(tile(app, "Effort")).not.toHaveClass(/is-wide/);

    await menu(app, "By status", "Remove tile");
    await expect(tiles(app)).toHaveText(["All tasks", "Effort"]);
  });

  test("the grid has two columns with room for them, and one on a phone", async ({
    app,
    page,
    isMobile,
  }) => {
    await add(app, "By status (Chart)");
    await add(app, "Effort (Chart)");
    const one = await tile(app, "By status").boundingBox();
    const two = await tile(app, "Effort").boundingBox();
    if (!one || !two) throw new Error("no boxes");
    if (isMobile) {
      expect(two.x).toBeCloseTo(one.x, 0);
      expect(two.y).toBeGreaterThan(one.y);
      const wider = await page.evaluate(
        () => document.documentElement.scrollWidth > document.documentElement.clientWidth,
      );
      expect(wider).toBe(false);
    } else {
      expect(two.y).toBeCloseTo(one.y, 0);
      expect(two.x).toBeGreaterThan(one.x);
    }
    await expectAccessible(page, ".dbme-dashboard");
  });

  test("Show as list turns its charts into their tables", async ({ app, page }) => {
    await add(app, "Effort (Chart)");
    await expect(tile(app, "Effort").locator(".dbme-charts__figure")).toBeVisible();
    await app.pane().getByRole("checkbox", { name: "Show as list" }).check();
    const rows = tile(app, "Effort").locator("tbody tr");
    await expect(rows.first().locator("td")).toHaveText(["To do", "5"]);
    await expect(tile(app, "Effort").locator(".dbme-charts__figure")).toHaveCount(0);
    await expectAccessible(page, ".dbme-dashboard");
  });

  test("a saved dashboard keeps its tiles, and comes back", async ({ app }) => {
    await add(app, "Effort (Chart)");
    await add(app, "By status (Chart)");
    await menu(app, "By status", "Move earlier");
    await menu(app, "Effort", "Make wider");
    await app.press(app.pane().getByRole("button", { name: "Save view" }));
    const dialog = app.page.getByRole("dialog", { name: "Save view" });
    await dialog.getByLabel("Name").fill("Weekly");
    await app.press(dialog.getByRole("button", { name: "Save", exact: true }));
    await expect(dialog).toBeHidden();

    await app.pane().getByLabel("Saved view").selectOption({ label: "None" });
    await expect(tiles(app)).toHaveCount(0);
    await app.pane().getByLabel("Saved view").selectOption({ label: "Weekly" });
    await expect(tiles(app)).toHaveText(["By status", "Effort"]);
    await expect(tile(app, "Effort")).toHaveClass(/is-wide/);
  });

  test("a dashboard is not offered as a tile of a dashboard", async ({ app, page }) => {
    await save(app, owner, "Other dashboard", "dashboard", { tiles: [] });
    await page.reload();
    await app.view("Dashboard");
    // It is there, as a dashboard to open.
    await expect(
      app.pane().getByLabel("Saved view").locator("option").filter({ hasText: "Other dashboard" }),
    ).toHaveCount(1);
    await app.press(app.pane().getByRole("button", { name: "Add tile" }));
    const dialog = page.getByRole("dialog", { name: "Add tile" });
    await expect(dialog.getByLabel("Saved chart or view").locator("option")).toHaveText([
      "All tasks (Table)",
      "By status (Chart)",
      "Effort (Chart)",
    ]);
  });

  test("a failure in one tile stays in that tile", async ({ app, page }) => {
    // A measure the store refuses, as a file could hold.
    await save(app, owner, "Broken", "charts", { x: "status", measure: "median", of: "estimate" });
    await page.reload();
    await app.view("Dashboard");
    await add(app, "Broken (Chart)");
    await add(app, "By status (Chart)");
    await add(app, "All tasks (Table)");
    await expect(tile(app, "Broken")).toContainText("This chart could not be read.");
    await expect(tile(app, "By status").locator(".dbme-charts__figure svg")).toBeVisible();
    await expect(tile(app, "All tasks").locator(".dbme-grid__body [role=row]")).toHaveCount(2);
    await expect(page.locator(".app__errors [role=alert]")).toHaveCount(0);
  });

  test("a tile whose saved view has gone says so, and can be removed", async ({ app, page }) => {
    const id = await save(app, owner, "Short lived", "charts", { x: "status" });
    await page.reload();
    await app.view("Dashboard");
    await add(app, "Short lived (Chart)");
    await expect(tiles(app)).toHaveText(["Short lived"]);
    await app.press(app.pane().getByRole("button", { name: "Save view" }));
    const dialog = page.getByRole("dialog", { name: "Save view" });
    await dialog.getByLabel("Name").fill("Keeps it");
    await app.press(dialog.getByRole("button", { name: "Save", exact: true }));
    await expect(dialog).toBeHidden();

    await app.ipc("view_delete", { id });
    await page.reload();
    await app.view("Dashboard");
    await app.pane().getByLabel("Saved view").selectOption({ label: "Keeps it" });
    await expect(tile(app, "Saved view gone")).toContainText("no longer exists");
    await menu(app, "Saved view gone", "Remove tile");
    await expect(tiles(app)).toHaveCount(0);
  });

  test("it holds in dark", async ({ app, page }) => {
    await app.ipc("settings_set", { patch: { theme: "dark" } });
    await page.reload();
    await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
    await app.view("Dashboard");
    await add(app, "By status (Chart)");
    await add(app, "All tasks (Table)");
    await expect(tile(app, "By status").locator(".dbme-charts__figure svg")).toBeVisible();
    await expectAccessible(page, ".dbme-dashboard");
  });
});

test("with nothing saved, Add tile says what to do first", async ({ seed, app, page }) => {
  await seed(plan());
  await app.open();
  await app.view("Dashboard");
  await app.press(app.pane().getByRole("button", { name: "Add tile" }));
  const dialog = page.getByRole("dialog", { name: "Add tile" });
  await expect(dialog.getByText("There is nothing saved to add yet.")).toBeVisible();
  await expect(dialog.getByRole("button", { name: "Add", exact: true })).toBeDisabled();
  await expectAccessible(page, "dialog");
});
