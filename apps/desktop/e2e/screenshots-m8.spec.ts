// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * M8's screenshots (Task 14): the eight views behind More views, with the same trip in each, at both sizes in
 * both themes, written to `target/screenshots/` and not committed. Nothing is asserted beyond what the flow
 * needs; the pictures are the record. Runs only when `DBME_SCREENSHOTS` is set:
 *
 *   DBME_SCREENSHOTS=1 npx playwright test -c e2e screenshots-m8 --project=edge-desktop --project=edge-phone
 */
import { mkdirSync } from "node:fs";
import path from "node:path";
import { expect, test, type App } from "./fixtures";

const OUT = path.resolve(import.meta.dirname, "../../../target/screenshots");
const DAY = 86_400_000;

const STOPS = [
  { title: "Reykjavik", status: "done", cost: 410, at: [64.15, -21.94], day: -3, days: 2 },
  { title: "Tromsø", status: "done", cost: 290, at: [69.65, 18.96], day: -1, days: 1 },
  { title: "Nairobi", status: "doing", cost: 820, at: [-1.29, 36.82], day: 1, days: 3 },
  { title: "Zanzibar", status: "doing", cost: 360, at: [-6.16, 39.2], day: 4, days: 2 },
  { title: "Hanoi", status: "todo", cost: 540, at: [21.03, 105.85], day: 6, days: 2 },
  { title: "Kyoto", status: "todo", cost: 760, at: [35.01, 135.77], day: 8, days: 4 },
  { title: "Osaka", status: "todo", cost: 230, at: [34.69, 135.5], day: 12, days: 1 },
] as const;

function plan(): unknown {
  return {
    kind: "tree",
    types: [
      {
        name: "Stop",
        fields: [
          {
            key: "status",
            label: "Status",
            kind: "select",
            options: {
              options: [
                { id: "todo", label: "To book" },
                { id: "doing", label: "Booked" },
                { id: "done", label: "Been" },
              ],
            },
          },
          { key: "cost", label: "Cost", kind: "number" },
        ],
      },
    ],
    objects: [
      {
        title: "Trip",
        children: STOPS.map((stop) => ({
          title: stop.title,
          types: ["Stop"],
          props: { status: stop.status, cost: stop.cost },
        })),
      },
    ],
  };
}

async function shot(app: App, theme: string, name: string): Promise<void> {
  const size = app.phone ? "phone" : "desktop";
  mkdirSync(OUT, { recursive: true });
  // Anything still arriving or drawing has its moment.
  await app.page.waitForTimeout(700);
  await app.page.screenshot({ path: path.join(OUT, `m8-${size}-${theme}-${name}.png`) });
}

async function runCommand(app: App, label: string): Promise<void> {
  if (app.phone) await app.press(app.page.getByRole("button", { name: "Search", exact: true }));
  else await app.page.keyboard.press("Control+k");
  const box = app.page.getByRole("combobox", { name: "Search or type > for a command" });
  await expect(box).toBeFocused();
  await box.fill(`> ${label}`);
  const option = app.page.getByRole("option", { name: label, exact: true });
  await expect(option).toBeVisible();
  await app.press(option);
}

test("M8: the views behind More views, at this size, in both themes", async ({ app, seed }) => {
  test.skip(!process.env.DBME_SCREENSHOTS, "set DBME_SCREENSHOTS=1 to take the screenshots");
  test.setTimeout(180_000);
  const { ids } = await seed<{ ids: Record<string, string> }>(plan());
  await app.open();
  const owner = ids.Trip ?? "";
  const now = Date.now();
  for (const stop of STOPS) {
    const id = ids[stop.title];
    await app.ipc("object_set_geo", { id, lat: stop.at[0], lon: stop.at[1] });
    const start = now + stop.day * DAY;
    await app.ipc("object_set_event", { id, start, end: start + stop.days * DAY });
  }
  await app.ipc("relation_create", { src: ids.Reykjavik, dst: ids["Tromsø"] });
  await app.ipc("relation_create", { src: ids.Nairobi, dst: ids.Zanzibar });
  await app.ipc("relation_create", { src: ids.Kyoto, dst: ids.Osaka });
  await app.ipc("relation_create", { src: ids.Hanoi, dst: ids.Kyoto });
  await app.ipc("view_save", {
    view: {
      name: "Cost by status",
      renderer: "charts",
      query: {
        where: [{ field: "$parent_id", op: "eq", value: owner }],
        sort: { by: "manual", field: null, descending: false },
      },
      config: {
        options: { mark: "bar", x: "status", measure: "count", series: null },
      },
      owner_object_id: owner,
    },
  });

  for (const theme of ["light", "dark"]) {
    await runCommand(app, `Theme: ${theme}`);
    await app.page.reload();
    await app.openPage("Trip");
    const close = app.page.getByRole("button", { name: "Close pages" });
    if (await close.isVisible()) await app.press(close);

    await app.view("Kanban");
    await shot(app, theme, "kanban");
    await app.view("Calendar");
    await shot(app, theme, "calendar");
    await app.view("Timeline");
    await shot(app, theme, "timeline");
    await app.view("Charts");
    await shot(app, theme, "charts");

    await app.view("Dashboard");
    await app.press(app.pane().getByRole("button", { name: "Add tile" }));
    const addTile = app.page.getByRole("dialog", { name: "Add tile" });
    await addTile
      .getByLabel("Saved chart or view")
      .selectOption({ label: "Cost by status (Chart)" });
    await app.press(addTile.getByRole("button", { name: "Add", exact: true }));
    await shot(app, theme, "dashboard");

    await app.view("Map");
    await expect(app.pane().locator(".dbme-geo__host")).toHaveAttribute("data-idle", "true");
    await shot(app, theme, "map");

    await app.view("Graph");
    const showList = app.pane().getByRole("checkbox", { name: "Show as list" });
    if (app.phone) await shot(app, theme, "graph-list");
    if (await showList.isChecked()) await app.press(showList);
    await expect(app.pane().locator(".dbme-graph__host")).toHaveAttribute("data-drawn", "true");
    await shot(app, theme, "graph");

    await app.view("Canvas");
    await app.page.waitForFunction(() => "__dbmeCanvas" in window);
    if (theme === "light") {
      for (const title of ["Reykjavik", "Nairobi", "Kyoto"]) {
        await app.press(app.pane().getByRole("button", { name: "Add card" }));
        const dialog = app.page.getByRole("dialog", { name: "Add card" });
        await dialog.getByLabel("Page").selectOption({ label: title });
        await app.press(dialog.getByRole("button", { name: "Add", exact: true }));
        await expect(app.pane().locator(".dbme-canvas__card", { hasText: title })).toBeVisible();
      }
    }
    await shot(app, theme, "canvas");
    const asList = app.pane().getByRole("checkbox", { name: "Show as list" });
    await app.press(asList);
    await shot(app, theme, "canvas-list");
    await app.press(asList);
  }
});
