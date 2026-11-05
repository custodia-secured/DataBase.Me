// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * The Charts view (M8, Task 7): drawn under the real CSP by the interpreter from rows the store counted,
 * its settings, the table that is its alternative, what a saved chart keeps, and both themes at both
 * sizes. The fixtures fail any test on a CSP report, an uncaught error or a request leaving the machine.
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
                { id: "doing", label: "Doing" },
                { id: "done", label: "Done" },
              ],
            },
          },
          { key: "due", label: "Due", kind: "date" },
          { key: "estimate", label: "Estimate", kind: "number" },
          { key: "shipped", label: "Shipped", kind: "boolean" },
        ],
      },
    ],
    objects: [
      {
        title: "Plan",
        children: [
          task("Alpha", { status: "done", due: "2026-09-02", estimate: 3, shipped: true }),
          task("Beta", { status: "done", due: "2026-09-20", estimate: 5, shipped: true }),
          task("Gamma", { status: "done", due: "2026-11-06", estimate: 1, shipped: false }),
          task("Delta", { status: "doing", due: "2026-11-07", estimate: 8, shipped: false }),
          task("Epsilon", { status: "todo", estimate: 2, shipped: false }),
          task("Zeta", { shipped: false }),
        ],
      },
    ],
  };
}

const figure = (app: App): Locator => app.pane().locator(".dbme-charts__figure");
const marks = (app: App): Locator => figure(app).locator("svg .mark-rect path");
const heading = (app: App): Locator => app.pane().locator(".dbme-charts__title");
const choose = (app: App, label: string, option: string) =>
  app.pane().getByLabel(label, { exact: true }).selectOption({ label: option });

/** On a phone the settings start folded away. */
async function openSettings(app: App): Promise<void> {
  const summary = app.pane().locator(".dbme-charts__settings summary");
  const open = await app.pane().locator(".dbme-charts__settings").getAttribute("open");
  if (open === null) await app.press(summary);
}

async function table(app: App): Promise<Locator> {
  await app.pane().getByRole("checkbox", { name: "Show as list" }).check();
  const region = app.pane().getByRole("region", { name: /^Data for/ });
  await expect(region).toBeVisible();
  return region.locator("tbody tr");
}

test.beforeEach(async ({ seed, app, page }) => {
  await page.clock.setFixedTime(new Date("2026-11-04T12:00:00Z"));
  await seed(plan());
  await app.open();
  await app.view("Charts");
  await expect(figure(app)).toBeVisible();
  await openSettings(app);
});

test("it draws from the first field with choices, and says so in words", async ({ app, page }) => {
  await expect(heading(app)).toHaveText("Count by Status");
  await expect(marks(app)).toHaveCount(4);
  await expect(figure(app)).toHaveAttribute(
    "aria-label",
    "Count by Status, as bars. 4 groups. The highest is Done, at 3.",
  );
  await expect(figure(app).locator("svg")).toContainText("Done");
  await expectAccessible(page, ".dbme-charts");
});

test("a bar never runs wider than 24 pixels", async ({ app }) => {
  await expect(marks(app).first()).toBeVisible();
  await choose(app, "Group by", "Due");
  await expect(heading(app)).toHaveText("Count by Due");
  await expect(marks(app).first()).toBeVisible();
  const box = await marks(app).first().boundingBox();
  if (!box) throw new Error("no box");
  expect(box.width).toBeLessThanOrEqual(24.5);
});

test("the table has every value the picture is drawn from", async ({ app, page }) => {
  const rows = await table(app);
  await expect(rows).toHaveCount(4);
  await expect(rows.first().locator("td")).toHaveText(["Done", "3"]);
  await expect(rows.last().locator("td")).toHaveText(["No value", "1"]);
  await expect(app.pane().getByRole("columnheader")).toHaveText(["Status", "Count"]);
  await expect(figure(app)).toHaveCount(0);
  await expectAccessible(page, ".dbme-charts");
});

test("a measure comes from the store, and leaves out what it has no number for", async ({
  app,
}) => {
  await choose(app, "Measure", "Sum");
  await expect(heading(app)).toHaveText("Sum of Estimate by Status");
  const rows = await table(app);
  await expect(rows.first().locator("td")).toHaveText(["Done", "9"]);
  await expect(rows.nth(1).locator("td")).toHaveText(["Doing", "8"]);
  await choose(app, "Measure", "Average");
  await expect(heading(app)).toHaveText("Average of Estimate by Status");
  await expect(rows.first().locator("td")).toHaveText(["Doing", "8"]);
  await expect(rows.nth(1).locator("td")).toHaveText(["Done", "3"]);
  await expect(rows.last().locator("td")).toHaveText(["No value", "None"]);
});

test("along time it has every period, the empty ones too, and offers a line", async ({ app }) => {
  await expect(app.pane().getByLabel("Chart", { exact: true }).locator("option")).toHaveText([
    "Bars",
    "Points",
  ]);
  await choose(app, "Group by", "Due");
  await expect(app.pane().getByLabel("Chart", { exact: true }).locator("option")).toHaveText([
    "Bars",
    "Line",
    "Area",
    "Points",
  ]);
  await expect(marks(app)).toHaveCount(3);
  await expect(figure(app).locator("svg")).toContainText("Oct 2026");

  await choose(app, "Chart", "Line");
  await expect(figure(app).locator("svg .mark-line path")).toHaveCount(1);
  await choose(app, "Chart", "Area");
  await expect(figure(app).locator("svg .mark-area path")).toHaveCount(1);
  await choose(app, "Chart", "Points");
  await expect(figure(app).locator("svg .mark-symbol path").first()).toBeVisible();

  // What has no date is not placed, and the table says only what the store has.
  const rows = await table(app);
  await expect(rows).toHaveCount(2);
  await expect(rows.first().locator("td")).toHaveText(["Sep 2026", "2"]);
});

test("the period can be chosen", async ({ app }) => {
  await choose(app, "Group by", "Due");
  await choose(app, "Each", "Quarters");
  await expect(figure(app).locator("svg")).toContainText("Q3 2026");
  await expect(figure(app).locator("svg")).toContainText("Q4 2026");
  await choose(app, "Each", "Years");
  await expect(marks(app)).toHaveCount(1);
  await expect(figure(app).locator("svg")).toContainText("2026");
});

test("a split stacks and has a legend naming each series", async ({ app, page }) => {
  await choose(app, "Group by", "Due");
  await choose(app, "Split by", "Status");
  await expect(heading(app)).toHaveText("Count by Due, split by Status");
  const legend = figure(app).locator("svg .role-legend");
  await expect(legend).toContainText("Done");
  await expect(legend).toContainText("Doing");
  await expectAccessible(page, ".dbme-charts");
  const rows = await table(app);
  await expect(app.pane().getByRole("columnheader")).toHaveText(["Due", "Status", "Count"]);
  await expect(rows.first().locator("td")).toHaveText(["Sep 2026", "Done", "2"]);
});

test("a filter narrows what is counted, and does not repaint what it leaves", async ({ app }) => {
  await choose(app, "Group by", "Due");
  await choose(app, "Split by", "Status");
  const swatch = (name: string) =>
    figure(app)
      .locator("svg .role-legend-symbol path")
      .nth(name === "Doing" ? 0 : 1);
  await expect(figure(app).locator("svg .role-legend")).toContainText("Doing");
  const before = await swatch("Done").getAttribute("fill");
  await app.press(app.pane().getByRole("button", { name: /^Filter/ }));
  const dialog = app.page.getByRole("dialog", { name: "Filter" });
  await app.press(dialog.getByRole("button", { name: "Add condition" }));
  await dialog.getByLabel("Field").selectOption({ label: "Status" });
  await dialog.getByLabel("Value").selectOption({ label: "Done" });
  await app.press(dialog.getByRole("button", { name: "Apply" }));
  await expect(figure(app).locator("svg .role-legend")).not.toContainText("Doing");
  const after = await figure(app)
    .locator("svg .role-legend-symbol path")
    .first()
    .getAttribute("fill");
  expect(after).toBe(before);
});

test("a tooltip gives the values under the pointer", async ({ app, page, isMobile }) => {
  test.skip(isMobile, "a phone has no pointer to hover; the table has the values");
  const box = await marks(app).first().boundingBox();
  if (!box) throw new Error("no box");
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  const tip = page.locator(".dbme-charts__tip");
  await expect(tip).toContainText("Status");
  await expect(tip).toContainText("Done");
  await expect(tip).toContainText("Count");
  await page.mouse.move(2, 2);
  await expect(tip).toHaveCount(0);
});

test("a saved chart keeps its settings, and its spec without the numbers", async ({ app }) => {
  await choose(app, "Group by", "Due");
  await choose(app, "Chart", "Line");
  await app.press(app.pane().getByRole("button", { name: "Save view" }));
  const dialog = app.page.getByRole("dialog", { name: "Save view" });
  await dialog.getByLabel("Name").fill("Due over time");
  await app.press(dialog.getByRole("button", { name: "Save", exact: true }));
  await expect(dialog).toBeHidden();

  await app.pane().getByLabel("Saved view").selectOption({ label: "None" });
  await expect(heading(app)).toHaveText("Count by Status");
  await app.pane().getByLabel("Saved view").selectOption({ label: "Due over time" });
  await expect(heading(app)).toHaveText("Count by Due");
  await expect(figure(app).locator("svg .mark-line path")).toHaveCount(1);

  const views =
    await app.ipc<{ name: string; renderer: string; config_json: { spec: unknown } }[]>(
      "views_list",
    );
  const saved = views.find((view) => view.name === "Due over time");
  expect(saved?.renderer).toBe("charts");
  const spec = JSON.stringify(saved?.config_json.spec);
  expect(spec).toContain("vega-lite");
  expect(spec).toContain('"name":"rows"');
  expect(spec).not.toContain("values");
});

test("nothing to chart says so", async ({ app }) => {
  await choose(app, "Group by", "Event date");
  await expect(app.pane().getByText("There is nothing to chart here yet.")).toBeVisible();
  await expect(figure(app)).toHaveCount(0);
});

test("it holds in dark, in colours of its own", async ({ app, page }) => {
  const light = await marks(app).first().getAttribute("fill");
  await app.ipc("settings_set", { patch: { theme: "dark" } });
  await page.reload();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
  await app.view("Charts");
  await expect(marks(app).first()).toBeVisible();
  expect(await marks(app).first().getAttribute("fill")).not.toBe(light);
  await expectAccessible(page, ".dbme-charts");
  const rows = await table(app);
  await expect(rows.first()).toBeVisible();
  await expectAccessible(page, ".dbme-charts");
});

test("on a phone the settings fold away and the page does not go sideways", async ({
  app,
  page,
  isMobile,
}) => {
  test.skip(!isMobile, "the phone layout");
  const box = await figure(app).boundingBox();
  if (!box) throw new Error("no box");
  expect(box.width).toBeLessThanOrEqual(360);
  await expect(marks(app).first()).toBeVisible();
  await expectAccessible(page, ".dbme-charts");
  const wider = await page.evaluate(
    () => document.documentElement.scrollWidth > document.documentElement.clientWidth,
  );
  expect(wider).toBe(false);
});
