// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * The Timeline view (M8, Task 6): one lane from days to decades, objects close up and counts further
 * out, a running event as long as it runs, what the keyboard does, its list alternative, and the lane
 * running down a phone.
 */
import type { Locator } from "@playwright/test";
import { expect, expectAccessible, test, type App } from "./fixtures";

function plan(): unknown {
  return {
    kind: "tree",
    types: [
      {
        name: "Task",
        fields: [
          { key: "due", label: "Due", kind: "date" },
          { key: "starts", label: "Starts", kind: "date" },
          { key: "estimate", label: "Estimate", kind: "number" },
        ],
      },
    ],
    objects: [
      {
        title: "Plan",
        children: [
          {
            title: "Alpha",
            types: ["Task"],
            props: { due: "2026-11-02", starts: "2026-03-10", estimate: 3 },
          },
          { title: "Beta", types: ["Task"], props: { due: "2026-11-02" } },
          { title: "Gamma", types: ["Task"], props: { due: "2026-11-06" } },
          { title: "Earlier", types: ["Task"], props: { due: "2026-06-15" } },
          { title: "Old", types: ["Task"], props: { due: "2019-03-15" } },
          { title: "Someday", types: ["Task"] },
        ],
      },
    ],
  };
}

const lane = (app: App): Locator => app.pane().getByRole("listbox", { name: /^Timeline/ });

const bar = (app: App, name: string): Locator =>
  lane(app).getByRole("option", { name: new RegExp(`^${name},`) });

const count = (app: App, key: string): Locator => lane(app).locator(`[data-bucket="${key}"]`);

const zoom = (app: App, label: string) => app.pane().getByLabel("Zoom").selectOption({ label });

test.beforeEach(async ({ seed, app, page }) => {
  await page.clock.setFixedTime(new Date("2026-11-04T12:00:00Z"));
  await seed(plan());
  await app.open();
  await app.view("Timeline");
  await expect(lane(app)).toBeVisible();
  await expect(count(app, "2026-11")).toBeVisible();
});

test("zoomed out it counts each bucket, with no objects drawn", async ({ app, page }) => {
  await expect(count(app, "2026-11")).toHaveAttribute("aria-label", "November 2026, 3 items");
  await expect(count(app, "2026-11")).toContainText("3");
  await expect(count(app, "2026-06")).toHaveAttribute("aria-label", "June 2026, 1 item");
  await expect(lane(app).locator(".dbme-timeline__bar")).toHaveCount(0);
  await expect(lane(app).locator(".dbme-timeline__today")).toHaveText("Today");
  await expect(app.pane().getByText("1 with no date")).toBeVisible();
  await expectAccessible(page, ".dbme-timeline");
});

test("it zooms from days to decades, keeping its place", async ({ app, page }) => {
  await zoom(app, "Years");
  await expect(count(app, "2026")).toHaveAttribute("aria-label", "2026, 4 items");
  await expect(count(app, "2019")).toHaveAttribute("aria-label", "2019, 1 item");
  await zoom(app, "Decades");
  await expect(count(app, "2026")).toBeVisible();
  await expect(lane(app).locator(".dbme-timeline__tick").filter({ hasText: "2020" })).toHaveCount(
    1,
  );
  await expectAccessible(page, ".dbme-timeline");

  // In from a lane that fits on screen, today is where it goes.
  await zoom(app, "Weeks");
  await app.press(app.pane().getByRole("button", { name: "Today" }));
  await expect(bar(app, "Alpha")).toBeVisible();
  await zoom(app, "Days");
  await expect(bar(app, "Alpha")).toHaveAttribute("data-day", "2026-11-02");
  await expect(lane(app).locator(".dbme-timeline__today")).toBeVisible();
  await expectAccessible(page, ".dbme-timeline");
});

test("only what is near the screen is drawn, however long the lane", async ({ app }) => {
  await zoom(app, "Days");
  await expect(bar(app, "Alpha")).toBeVisible();
  // 2018 to 2027 is over 3,600 days.
  const ticks = await lane(app).locator(".dbme-timeline__tick").count();
  expect(ticks).toBeGreaterThan(0);
  expect(ticks).toBeLessThan(60);
  // Old is years away, so it has not been read or drawn.
  await expect(bar(app, "Old")).toHaveCount(0);
  await app.press(app.pane().getByRole("button", { name: "Today" }));
  await expect(bar(app, "Alpha")).toBeVisible();
});

test("two objects at one moment take a lane each", async ({ app, isMobile }) => {
  await zoom(app, "Days");
  const alpha = await bar(app, "Alpha").boundingBox();
  const beta = await bar(app, "Beta").boundingBox();
  if (!alpha || !beta) throw new Error("no boxes");
  if (isMobile) {
    expect(alpha.y).toBeCloseTo(beta.y, 0);
    expect(Math.abs(alpha.x - beta.x)).toBeGreaterThan(alpha.width - 1);
  } else {
    expect(alpha.x).toBeCloseTo(beta.x, 0);
    expect(Math.abs(alpha.y - beta.y)).toBeGreaterThan(alpha.height - 1);
  }
});

test("a first press selects and offers what comes next, at any size", async ({ app, page }) => {
  await zoom(app, "Weeks");
  await app.press(bar(app, "Alpha"));
  await expect(bar(app, "Alpha")).toHaveAttribute("aria-selected", "true");
  await expect(app.pane().locator(".dbme-timeline__selected")).toContainText(
    "Alpha, Monday, November 2, 2026",
  );
  await expectAccessible(page, ".dbme-timeline__selected");
  await app.press(app.pane().getByRole("button", { name: "Open" }));
  await app.view("Document");
  await expect(app.title()).toHaveValue("Alpha");
});

test("a second press on a bucket zooms in on it", async ({ app }) => {
  await app.press(count(app, "2026-06"));
  await expect(app.pane().locator(".dbme-timeline__selected")).toContainText("June 2026, 1 item");
  await app.press(count(app, "2026-06"));
  await expect(app.pane().getByLabel("Zoom")).toHaveValue("week");
  await expect(bar(app, "Earlier")).toBeVisible();
});

test("it goes by any date", async ({ app }) => {
  await app.pane().getByLabel("By").selectOption({ label: "Starts" });
  await expect(app.pane().getByText("5 with no date")).toBeVisible();
  await expect(count(app, "2026-11")).toHaveCount(0);
  await zoom(app, "Years");
  await expect(count(app, "2026")).toHaveAttribute("aria-label", "2026, 1 item");
});

test("an event is as long as it runs, and stays drawn after its start has gone by", async ({
  app,
  isMobile,
}) => {
  test.skip(isMobile, "the same rule runs down a phone; the length is measured across");
  const search = async (text: string) => {
    const [found] = await app.ipc<{ object: { id: string } }[]>("object_search", {
      text,
      limit: 1,
    });
    if (!found) throw new Error(`no ${text}`);
    return found.object.id;
  };
  const start = Date.parse("2026-10-01T00:00:00Z");
  await app.ipc("object_set_event", {
    id: await search("Alpha"),
    start,
    end: Date.parse("2026-12-20T00:00:00Z"),
  });
  await app.ipc("object_set_event", { id: await search("Beta"), start, end: null });

  await app.pane().getByLabel("By").selectOption({ label: "Event date" });
  await zoom(app, "Weeks");
  await app.press(app.pane().getByRole("button", { name: "Earlier" }));
  await expect(bar(app, "Beta")).toBeVisible();
  const long = await bar(app, "Alpha").boundingBox();
  const short = await bar(app, "Beta").boundingBox();
  if (!long || !short) throw new Error("no boxes");
  // Eighty days at 120 px a week, and the least a bar can be.
  expect(long.width).toBeGreaterThan(1300);
  expect(short.width).toBeLessThan(130);

  // Far enough on that Alpha began before what is read, and it is still running there.
  await zoom(app, "Days");
  await app.press(app.pane().getByRole("button", { name: "Today" }));
  await expect(bar(app, "Alpha")).toBeVisible();
  await expect(bar(app, "Beta")).toHaveCount(0);
});

test("the keyboard goes along the lane, zooms and opens", async ({ app, page, isMobile }) => {
  test.skip(isMobile, "the phone has no keys; its way is a press and the buttons");
  await lane(app).focus();
  await page.keyboard.press("ArrowRight");
  await expect(lane(app)).toHaveAttribute("aria-activedescendant", /.+/);
  // The first thing wholly on screen, then back along the lane.
  const selected = lane(app).locator('[aria-selected="true"]');
  await expect(selected).toHaveCount(1);
  await expect(selected).toHaveAttribute("data-bucket", "2026-11");
  await page.keyboard.press("ArrowLeft");
  await expect(selected).toHaveAttribute("data-bucket", "2026-06");

  // In on the bucket, with Enter.
  await page.keyboard.press("Enter");
  await expect(app.pane().getByLabel("Zoom")).toHaveValue("week");
  await expect(bar(app, "Earlier")).toBeVisible();
  await page.keyboard.press("ArrowRight");
  await expect(lane(app).locator('[aria-selected="true"]')).toHaveAttribute(
    "data-day",
    "2026-06-15",
  );

  // In and out with the plus and minus keys.
  await page.keyboard.press("+");
  await expect(app.pane().getByLabel("Zoom")).toHaveValue("day");
  await page.keyboard.press("-");
  await expect(app.pane().getByLabel("Zoom")).toHaveValue("week");
  await page.keyboard.press("-");
  await expect(app.pane().getByLabel("Zoom")).toHaveValue("month");

  // To the ends, and Enter on an object opens it.
  await page.keyboard.press("Home");
  await expect(
    lane(app).locator(".dbme-timeline__tick").filter({ hasText: "Jan 2018" }),
  ).toBeVisible();
  await page.keyboard.press("End");
  await expect(
    lane(app).locator(".dbme-timeline__tick").filter({ hasText: "Dec 2027" }),
  ).toBeVisible();
  await app.press(app.pane().getByRole("button", { name: "Today" }));
  await zoom(app, "Weeks");
  await lane(app).focus();
  await app.press(bar(app, "Alpha"));
  await page.keyboard.press("Enter");
  await app.view("Document");
  await expect(app.title()).toHaveValue("Alpha");
});

test("a filter narrows what is counted", async ({ app }) => {
  await app.press(app.pane().getByRole("button", { name: /^Filter/ }));
  const dialog = app.page.getByRole("dialog", { name: "Filter" });
  await app.press(dialog.getByRole("button", { name: "Add condition" }));
  await dialog.getByLabel("Field").selectOption({ label: "Title" });
  await dialog.getByLabel("Value").fill("Gam");
  await dialog.getByLabel("Comparison").selectOption({ label: "starts with" });
  await app.press(dialog.getByRole("button", { name: "Apply" }));
  await expect(count(app, "2026-11")).toHaveAttribute("aria-label", "November 2026, 1 item");
  await expect(count(app, "2026-06")).toHaveCount(0);
});

test("objects with no date say so", async ({ app }) => {
  await app.pane().getByLabel("By").selectOption({ label: "Event date" });
  await expect(app.pane().getByText("Nothing here has a event date yet")).toBeVisible();
});

test("Show as list is the same objects with their dates written out", async ({
  app,
  page,
  isMobile,
}) => {
  await app.pane().getByRole("checkbox", { name: "Show as list" }).check();
  const grid = app.pane().getByRole("grid", { name: "Dated items" });
  await expect(grid.getByRole("columnheader")).toHaveText(["Title", "Due", "Starts", "Estimate"]);
  const rows = grid.locator(".dbme-grid__body [role=row]");
  await expect(rows).toHaveCount(5);
  await expect(rows.first()).toContainText("Old");
  await expect(rows.first().locator('[aria-colindex="2"]')).toHaveText("Friday, March 15, 2019");
  await expectAccessible(page, ".dbme-timeline");
  if (!isMobile) {
    await grid.focus();
    await page.keyboard.press("Enter");
    await app.view("Document");
    await expect(app.title()).toHaveValue("Old");
  }
});

test("a saved timeline keeps its zoom", async ({ app }) => {
  await zoom(app, "Years");
  await app.press(app.pane().getByRole("button", { name: "Save view" }));
  const dialog = app.page.getByRole("dialog", { name: "Save view" });
  await dialog.getByLabel("Name").fill("By year");
  await app.press(dialog.getByRole("button", { name: "Save", exact: true }));
  await expect(dialog).toBeHidden();
  await app.pane().getByLabel("Saved view").selectOption({ label: "None" });
  await expect(app.pane().getByLabel("Zoom")).toHaveValue("month");
  await app.pane().getByLabel("Saved view").selectOption({ label: "By year" });
  await expect(app.pane().getByLabel("Zoom")).toHaveValue("year");
  await expect(count(app, "2026")).toBeVisible();
});

test("on a phone the lane runs down the screen and the page does not go sideways", async ({
  app,
  page,
  isMobile,
}) => {
  test.skip(!isMobile, "the phone layout");
  await expect(lane(app)).toHaveAttribute("aria-orientation", "vertical");
  await zoom(app, "Days");
  const alpha = await bar(app, "Alpha").boundingBox();
  const gamma = await bar(app, "Gamma").boundingBox();
  if (!alpha || !gamma) throw new Error("no boxes");
  expect(gamma.y).toBeGreaterThan(alpha.y);
  await expectAccessible(page, ".dbme-timeline");
  const wider = await page.evaluate(
    () => document.documentElement.scrollWidth > document.documentElement.clientWidth,
  );
  expect(wider).toBe(false);
});

test("it holds in dark", async ({ app, page }) => {
  await app.ipc("settings_set", { patch: { theme: "dark" } });
  await page.reload();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
  await app.view("Timeline");
  await expect(count(app, "2026-11")).toBeVisible();
  await expectAccessible(page, ".dbme-timeline");
  await zoom(app, "Days");
  await expect(bar(app, "Alpha")).toBeVisible();
  await expectAccessible(page, ".dbme-timeline");
});
