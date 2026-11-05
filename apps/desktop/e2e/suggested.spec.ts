// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * "Suggested" in the Views menu (M8, Task 9): what it offers follows what is under the page, it reads
 * nothing until the menu opens, it never switches a view by itself, and it offers only what the mode shows.
 * The fixtures fail any test on a CSP report, an uncaught error or a request leaving the machine.
 */
import type { Locator } from "@playwright/test";
import { expect, expectAccessible, test, type App } from "./fixtures";

function plan(): unknown {
  const task = (title: string, status: string) => ({ title, types: ["Task"], props: { status } });
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
                { id: "doing", label: "Doing" },
                { id: "done", label: "Done" },
              ],
            },
          },
        ],
      },
    ],
    objects: [
      {
        title: "Board",
        children: [
          task("Alpha", "todo"),
          task("Beta", "doing"),
          task("Gamma", "done"),
          task("Delta", "todo"),
        ],
      },
      { title: "Empty page" },
    ],
  };
}

async function openMenu(app: App): Promise<Locator> {
  await app.press(app.pane().getByRole("button", { name: "More views" }));
  const menu = app.page.getByRole("menu");
  await expect(menu).toBeVisible();
  return menu;
}

const section = (menu: Locator): Locator => menu.getByRole("group", { name: "Suggested" });

test.beforeEach(async ({ seed }) => {
  await seed(plan());
});

test("it offers what suits the contents, with why, and only switches when chosen", async ({
  app,
}) => {
  await app.open();
  await app.openPage("Board");
  const tab = (name: string) => app.pane().getByRole("tab", { name });
  await expect(tab("Document")).toHaveAttribute("aria-selected", "true");

  const menu = await openMenu(app);
  const kanban = section(menu).getByRole("menuitem", { name: "Kanban" });
  await expect(kanban).toBeVisible();
  await expect(kanban).toContainText("a few states");
  // The list above it stays as it was.
  await expect(menu.getByRole("menuitem").first()).toHaveText("Kanban");
  await expect(tab("Document")).toHaveAttribute("aria-selected", "true");

  await app.press(kanban);
  await expect(tab("Kanban")).toHaveAttribute("aria-selected", "true");
  await expect(app.pane().getByText("Alpha")).toBeVisible();

  // What is already showing is not offered again.
  const again = await openMenu(app);
  await expect(section(again).getByRole("menuitem").first()).toBeVisible();
  await expect(section(again).getByRole("menuitem", { name: "Kanban" })).toHaveCount(0);
});

test("nothing is read until the menu opens, and nothing once it has shut", async ({
  app,
  page,
}) => {
  let profiles = 0;
  page.on("request", (request) => {
    if (request.postData()?.includes('"cmd":"data_profile"') === true) profiles += 1;
  });
  await app.open();
  await app.openPage("Board");

  // Every change to the vault refreshes what is being read, so one is made while it is shut.
  const edit = async (title: string) => {
    await app.title().fill(title);
    await app.title().blur();
    await expect(app.currentCrumb()).toHaveText(title);
  };
  await edit("Board one");
  expect(profiles).toBe(0);

  const menu = await openMenu(app);
  await expect(section(menu).getByRole("menuitem").first()).toBeVisible();
  const opened = profiles;
  expect(opened).toBeGreaterThan(0);

  await page.keyboard.press("Escape");
  await expect(menu).toBeHidden();
  await edit("Board two");
  expect(profiles).toBe(opened);
});

test("a page with nothing under it offers nothing", async ({ app, page }) => {
  await app.open();
  await app.openPage("Empty page");
  const read = page.waitForResponse(
    (response) => response.request().postData()?.includes('"cmd":"data_profile"') === true,
  );
  const menu = await openMenu(app);
  await expect(menu.getByRole("menuitem", { name: "Kanban" })).toBeVisible();
  // Asked after the answer came and had a frame to show in, so that this is not just too early to tell.
  await read;
  await page.evaluate(() => new Promise(requestAnimationFrame));
  await page.evaluate(() => new Promise(requestAnimationFrame));
  await expect(section(menu)).toHaveCount(0);
});

test("it offers nothing the mode hides", async ({ app }) => {
  await app.open({ mode: "simple" });
  await app.openPage("Board");
  // Everything the data here asks for is hidden in Simple, and what it shows are all tabs.
  await expect(app.pane().getByRole("tab", { name: "Outline" })).toBeVisible();
  await expect(app.pane().getByRole("tab", { name: "Table" })).toHaveCount(0);
  await expect(app.pane().getByRole("button", { name: "More views" })).toHaveCount(0);
});

test("it works by keyboard alone", async ({ app, page }) => {
  test.skip(app.phone, "a phone has no keyboard");
  await app.open();
  await app.openPage("Board");
  await app.pane().getByRole("button", { name: "More views" }).focus();
  await page.keyboard.press("Enter");
  const kanban = section(page.getByRole("menu")).getByRole("menuitem", { name: "Kanban" });
  await expect(kanban).toBeVisible();
  // Reached with the focus still in the menu: what arrived did not take it.
  await page.keyboard.press("End");
  await page.keyboard.press("ArrowUp");
  await expect(kanban).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(app.pane().getByRole("tab", { name: "Kanban" })).toHaveAttribute(
    "aria-selected",
    "true",
  );
});

test("the open menu is accessible in both themes and fits the window", async ({ app, page }) => {
  await app.open();
  await app.openPage("Board");
  const check = async () => {
    const menu = await openMenu(app);
    await expect(section(menu).getByRole("menuitem").first()).toBeVisible();
    const box = await menu.boundingBox();
    expect(box).not.toBeNull();
    expect(box?.x ?? -1).toBeGreaterThanOrEqual(0);
    expect((box?.x ?? 0) + (box?.width ?? 0)).toBeLessThanOrEqual(page.viewportSize()?.width ?? 0);
    await expectAccessible(page, ".dbme-menu");
  };
  await check();

  await page.keyboard.press("Escape");
  await app.ipc("settings_set", { patch: { theme: "dark" } });
  await page.reload();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
  await app.openPage("Board");
  await check();
});
