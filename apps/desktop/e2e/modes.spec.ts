// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * Modes (M7, ADR-0027; brief §2.9): Simple hides types, fields, the Table view and the type
 * picker, and offers the daily journal; Organizer shows everything; the switch is a setting and
 * changes nothing in the vault.
 */
import { workspace } from "./data";
import { expect, expectAccessible, test, type App } from "./fixtures";

async function runCommand(app: App, label: string) {
  if (app.phone) await app.press(app.page.getByRole("button", { name: "Search", exact: true }));
  else await app.page.keyboard.press("Control+k");
  const box = app.page.getByRole("combobox", { name: "Search or type > for a command" });
  await expect(box).toBeFocused();
  await box.fill(`> ${label}`);
  const option = app.page.getByRole("option", { name: label, exact: true });
  await expect(option).toBeVisible();
  await app.press(option);
}

/** The local date as the journal titles it. */
function today(): string {
  const now = new Date();
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${String(now.getFullYear())}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`;
}

test("Simple hides types, fields and the Table view, and Organizer shows them again", async ({
  seed,
  app,
  page,
}) => {
  await seed(workspace());
  await app.open({ mode: "simple" });
  const tabs = app.pane().getByRole("tablist", { name: "Views" });
  await expect(tabs.getByRole("tab")).toHaveCount(4);
  await expect(tabs.getByRole("tab", { name: "Table" })).toHaveCount(0);

  // A typed page shows no types line and no properties.
  await app.openPage("Home", "Alpha");
  await expect(app.title()).toHaveValue("Alpha");
  await expect(app.pane().getByRole("list", { name: "Types" })).toHaveCount(0);
  await expect(app.pane().getByRole("button", { name: "Status: Doing" })).toHaveCount(0);
  await expectAccessible(page);

  // The gallery lists the pages inside, with no type picker.
  await app.view("Gallery");
  await expect(app.pane().getByRole("combobox", { name: "Show" })).toHaveCount(0);

  // Organizer brings it all back, and the vault is as it was.
  await runCommand(app, "Mode: Organizer");
  await expect(tabs.getByRole("tab", { name: "Table" })).toBeVisible();
  await expect(app.pane().getByRole("combobox", { name: "Show" })).toBeVisible();
  await app.view("Document");
  await expect(app.pane().getByRole("list", { name: "Types" })).toBeVisible();
  await expect(app.pane().getByRole("button", { name: "Status: Doing" })).toBeVisible();
  const settings = await app.ipc<{ mode: string }>("settings_get");
  expect(settings.mode).toBe("organizer");

  // Back to Simple with the Table view chosen: the pane shows the first view it has.
  await app.view("Table");
  await runCommand(app, "Mode: Simple");
  await expect(tabs.getByRole("tab", { name: "Document" })).toHaveAttribute(
    "aria-selected",
    "true",
  );
});

test("Simple leaves out everything in More views, and Organizer brings it back", async ({
  seed,
  app,
}) => {
  await seed(workspace());
  await app.open({ mode: "simple" });
  const pane = app.pane();
  await expect(pane.getByRole("button", { name: "More views" })).toHaveCount(0);

  await runCommand(app, "Mode: Organizer");
  await app.press(pane.getByRole("button", { name: "More views" }));
  for (const name of [
    "Kanban",
    "Calendar",
    "Timeline",
    "Charts",
    "Dashboard",
    "Map",
    "Graph",
    "Canvas",
  ]) {
    await expect(app.page.getByRole("menuitem", { name, exact: true }).first()).toBeVisible();
  }
});

test("a drawing in hand is saved when Simple takes the Canvas away", async ({ seed, app }) => {
  await seed(workspace());
  await app.open();
  await app.openPage("Home", "Alpha");
  await app.view("Canvas");
  await app.page.waitForFunction(() => "__dbmeCanvas" in window);
  await app.page.evaluate(() => {
    const { __dbmeCanvas: api } = window as unknown as {
      __dbmeCanvas: { updateScene: (scene: unknown) => void };
    };
    api.updateScene({
      captureUpdate: "IMMEDIATELY",
      elements: [
        {
          id: "kept",
          type: "rectangle",
          x: 1,
          y: 2,
          width: 30,
          height: 40,
          angle: 0,
          strokeColor: "#1e1e1e",
          backgroundColor: "transparent",
          fillStyle: "solid",
          strokeWidth: 2,
          strokeStyle: "solid",
          roughness: 1,
          opacity: 100,
          groupIds: [],
          frameId: null,
          index: "a1",
          roundness: null,
          seed: 1,
          version: 2,
          versionNonce: 2,
          isDeleted: false,
          boundElements: null,
          updated: 1,
          link: null,
          locked: false,
        },
      ],
    });
  });
  await runCommand(app, "Mode: Simple");
  await expect(app.title()).toHaveValue("Alpha");
  await runCommand(app, "Mode: Organizer");
  await app.view("Canvas");
  await expect(app.pane().locator(".dbme-canvas__count")).toHaveText(
    "0 cards and 1 drawn element.",
  );
});

test("Today opens the day's page under Journal, and only makes it once", async ({ seed, app }) => {
  await seed(workspace());
  await app.open({ mode: "simple" });
  await runCommand(app, "Today");
  await expect(app.currentCrumb()).toHaveText(today());
  const first = (
    await app.ipc<{ title: string; id: string }[]>("object_children", {
      parent: null,
    })
  ).find((page) => page.title === "Journal");
  expect(first).toBeDefined();
  await app.openPage("Home");
  await runCommand(app, "Today");
  await expect(app.currentCrumb()).toHaveText(today());
  const days = await app.ipc<{ title: string }[]>("object_children", { parent: first?.id });
  expect(days.filter((page) => page.title === today())).toHaveLength(1);
});
