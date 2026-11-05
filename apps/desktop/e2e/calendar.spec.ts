// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * The Calendar view (M8, Task 5): items on their days, reading only a bound per day, moving an item by
 * drag, by keyboard and with the Move to control, the agenda on a phone, and its list alternative.
 */
import type { Locator, Page } from "@playwright/test";
import { expect, expectAccessible, test, type App } from "./fixtures";

const busy = Array.from({ length: 5 }, (_, n) => ({
  title: `Busy ${String(n + 1)}`,
  types: ["Task"],
  props: { due: "2026-11-10" },
}));

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
            props: { due: "2026-11-02", starts: "2026-11-20", estimate: 3 },
          },
          { title: "Beta", types: ["Task"], props: { due: "2026-11-04" } },
          { title: "Gamma", types: ["Task"], props: { due: "2026-11-04" } },
          { title: "Outside", types: ["Task"], props: { due: "2026-10-30" } },
          { title: "Later", types: ["Task"], props: { due: "2026-12-25" } },
          { title: "Someday", types: ["Task"] },
          ...busy,
        ],
      },
    ],
  };
}

const day = (app: App, date: string): Locator =>
  app.pane().locator(`.dbme-calendar [data-day="${date}"]`);

const items = (app: App, date: string): Locator => day(app, date).locator(".dbme-calendar__item");

const title = (app: App): Locator => app.pane().locator(".dbme-calendar__title");

test.beforeEach(async ({ seed, app, page }) => {
  await page.clock.setFixedTime(new Date("2026-11-04T12:00:00Z"));
  await seed(plan());
  await app.open();
  await app.view("Calendar");
  await expect(title(app)).toHaveText("November 2026");
  // The days fill in as they are read, and a busy one makes its row taller.
  for (const date of ["2026-11-02", "2026-11-04", "2026-11-10"]) {
    await expect(items(app, date).first()).toBeVisible();
  }
});

test.describe("on a screen with room for a grid", () => {
  test.skip(({ isMobile }) => isMobile, "a phone shows an agenda");

  test("each item is on its day, today is marked, and the grid's edges are read too", async ({
    app,
    page,
  }) => {
    await expect(items(app, "2026-11-02")).toHaveText(["Alpha"]);
    await expect(items(app, "2026-11-04")).toHaveText(["Beta", "Gamma"]);
    await expect(items(app, "2026-10-30")).toHaveText(["Outside"]);
    await expect(day(app, "2026-11-04")).toHaveAttribute("aria-current", "date");
    await expect(day(app, "2026-11-04")).toContainText("Wednesday, November 4, 2026, 2 items");
    await expect(day(app, "2026-11-03")).not.toHaveAttribute("aria-current", "date");
    await expect(app.pane().getByRole("columnheader")).toHaveText([
      "Mon",
      "Tue",
      "Wed",
      "Thu",
      "Fri",
      "Sat",
      "Sun",
    ]);
    await expectAccessible(page, ".dbme-calendar");
  });

  test("a busy day shows its bound and says how many more, and they can be reached", async ({
    app,
    page,
  }) => {
    await expect(items(app, "2026-11-10")).toHaveCount(3);
    const more = day(app, "2026-11-10").getByRole("button", { name: "+2 more" });
    await app.press(more);
    const dialog = page.getByRole("dialog", { name: "Tuesday, November 10, 2026" });
    const rows = dialog.locator(".dbme-grid__body [role=row]");
    await expect(rows).toHaveCount(5);
    await expectAccessible(page, "dialog");
    await dialog.getByRole("grid").focus();
    await page.keyboard.press("ArrowDown");
    await page.keyboard.press("ArrowDown");
    await page.keyboard.press("ArrowDown");
    await page.keyboard.press("ArrowDown");
    await page.keyboard.press("Enter");
    await expect(dialog).toBeHidden();
    await expect(app.pane().getByLabel("Move “Busy 5” to")).toHaveValue("2026-11-10");
  });

  test("the buttons walk the months, and Today comes back", async ({ app }) => {
    await app.press(app.pane().getByRole("button", { name: "Next month" }));
    await expect(title(app)).toHaveText("December 2026");
    await expect(items(app, "2026-12-25")).toHaveText(["Later"]);
    await app.press(app.pane().getByRole("button", { name: "Previous month" }));
    await app.press(app.pane().getByRole("button", { name: "Previous month" }));
    await expect(title(app)).toHaveText("October 2026");
    await app.press(app.pane().getByRole("button", { name: "Today" }));
    await expect(title(app)).toHaveText("November 2026");
  });

  test("a week shows seven days and more of each", async ({ app, page }) => {
    await app.pane().getByLabel("Period").selectOption({ label: "Week" });
    await expect(title(app)).toHaveText("Nov 2 – 8, 2026");
    await expect(app.pane().locator(".dbme-calendar [data-day]")).toHaveCount(7);
    await expect(items(app, "2026-11-04")).toHaveText(["Beta", "Gamma"]);
    await app.press(app.pane().getByRole("button", { name: "Next week" }));
    await expect(title(app)).toHaveText("Nov 9 – 15, 2026");
    await expect(items(app, "2026-11-10")).toHaveCount(5);
    await expectAccessible(page, ".dbme-calendar");
  });

  test("the keyboard walks the grid and carries an item", async ({ app, page }) => {
    const today = day(app, "2026-11-04");
    await today.focus();
    await page.keyboard.press("ArrowLeft");
    await expect(day(app, "2026-11-03")).toBeFocused();
    await page.keyboard.press("ArrowRight");
    await expect(today).toBeFocused();
    await page.keyboard.press("Home");
    await expect(day(app, "2026-11-02")).toBeFocused();
    await page.keyboard.press("End");
    await expect(day(app, "2026-11-08")).toBeFocused();
    await page.keyboard.press("ArrowLeft");
    await page.keyboard.press("ArrowLeft");
    await page.keyboard.press("ArrowLeft");
    await page.keyboard.press("ArrowLeft");
    await expect(today).toBeFocused();

    // Into the cell, along its items, and back out.
    await page.keyboard.press("Enter");
    await expect(items(app, "2026-11-04").first()).toBeFocused();
    await page.keyboard.press("ArrowDown");
    await expect(items(app, "2026-11-04").nth(1)).toBeFocused();
    await page.keyboard.press("Escape");
    await expect(today).toBeFocused();

    // Carried a day, and a week, keeping the focus; each is one undo.
    await page.keyboard.press("Enter");
    await page.keyboard.press("Alt+ArrowRight");
    await expect(items(app, "2026-11-05")).toHaveText(["Beta"]);
    await expect(items(app, "2026-11-05").first()).toBeFocused();
    await expect(app.pane().locator(".dbme-calendar__selected").getByRole("status")).toHaveText(
      "Moved Beta to Thursday, November 5, 2026.",
    );
    await page.keyboard.press("Alt+ArrowDown");
    await expect(items(app, "2026-11-12")).toHaveText(["Beta"]);
    await page.keyboard.press("Control+z");
    await expect(items(app, "2026-11-05")).toHaveText(["Beta"]);
    await page.keyboard.press("Control+z");
    await expect(items(app, "2026-11-04")).toHaveText(["Beta", "Gamma"]);
  });

  test("the keyboard leaves the month when it has to", async ({ app, page }) => {
    await day(app, "2026-11-04").focus();
    await page.keyboard.press("PageDown");
    await expect(title(app)).toHaveText("December 2026");
    await expect(day(app, "2026-12-04")).toBeFocused();
    await page.keyboard.press("PageUp");
    await expect(title(app)).toHaveText("November 2026");
    await expect(day(app, "2026-11-04")).toBeFocused();
  });

  test("Enter on an item opens it", async ({ app, page }) => {
    await day(app, "2026-11-02").focus();
    await page.keyboard.press("Enter");
    await page.keyboard.press("Enter");
    await app.view("Document");
    await expect(app.title()).toHaveValue("Alpha");
  });

  test("an item can be dragged to another day, in one undo", async ({ app, page }) => {
    await drag(page, items(app, "2026-11-02").first(), day(app, "2026-11-19"));
    await expect(items(app, "2026-11-19")).toHaveText(["Alpha"]);
    await expect(items(app, "2026-11-02")).toHaveCount(0);
    await page.getByRole("button", { name: "Undo" }).click();
    await expect(items(app, "2026-11-02")).toHaveText(["Alpha"]);
  });

  test("dropping an item back on its own day changes nothing", async ({ app, page }) => {
    await drag(page, items(app, "2026-11-02").first(), day(app, "2026-11-02"));
    await expect(items(app, "2026-11-02")).toHaveText(["Alpha"]);
    await expect(app.pane().locator(".dbme-calendar__selected").getByRole("status")).toHaveText("");
  });

  test("it goes by any date, and by the event date", async ({ app, page }) => {
    await app.pane().getByLabel("By").selectOption({ label: "Starts" });
    await expect(items(app, "2026-11-20")).toHaveText(["Alpha"]);
    await expect(items(app, "2026-11-02")).toHaveCount(0);

    await app.pane().getByLabel("By").selectOption({ label: "Event date" });
    await expect(app.pane().locator(".dbme-calendar__item")).toHaveCount(0);
    await app.press(app.pane().getByRole("button", { name: "11 with no date" }));
    const dialog = page.getByRole("dialog", { name: "No event date" });
    await dialog.getByRole("grid").focus();
    await page.keyboard.press("Enter");
    await expect(dialog).toBeHidden();

    // The first by title is Alpha, which has no event date until now.
    await app.pane().getByLabel("Date “Alpha”").fill("2026-11-16");
    await app.press(app.pane().getByRole("button", { name: "Set date" }));
    await expect(items(app, "2026-11-16")).toHaveText(["Alpha"]);
    await expect(app.pane().getByRole("button", { name: "10 with no date" })).toBeVisible();

    await drag(page, items(app, "2026-11-16").first(), day(app, "2026-11-17"));
    await expect(items(app, "2026-11-17")).toHaveText(["Alpha"]);
    await page.getByRole("button", { name: "Undo" }).click();
    await expect(items(app, "2026-11-16")).toHaveText(["Alpha"]);
    await page.getByRole("button", { name: "Undo" }).click();
    await expect(app.pane().locator(".dbme-calendar__item")).toHaveCount(0);
  });

  test("it holds in dark", async ({ app, page }) => {
    await app.ipc("settings_set", { patch: { theme: "dark" } });
    await page.reload();
    await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
    await app.view("Calendar");
    await expect(items(app, "2026-11-02")).toHaveText(["Alpha"]);
    await expectAccessible(page, ".dbme-calendar");
  });
});

/** Drags with the mouse, and lets go only once the day shows it would take the item. */
async function drag(page: Page, item: Locator, target: Locator): Promise<void> {
  const from = await item.boundingBox();
  const to = await target.boundingBox();
  if (!from || !to) throw new Error("no boxes");
  const start = { x: from.x + from.width / 2, y: from.y + from.height / 2 };
  await page.mouse.move(start.x, start.y);
  await page.mouse.down();
  await page.mouse.move(start.x + 12, start.y, { steps: 4 });
  await page.mouse.move(to.x + to.width / 2, to.y + to.height - 8, { steps: 12 });
  await expect(target).toHaveClass(/is-over/);
  await page.mouse.up();
}

test("the Move to control moves the selected item, at any size", async ({ app, page }) => {
  await app.press(items(app, "2026-11-02").first());
  const control = app.pane().getByLabel("Move “Alpha” to");
  await expect(control).toHaveValue("2026-11-02");
  await expect(app.pane().getByRole("button", { name: "Move", exact: true })).toBeDisabled();
  await control.fill("2026-11-20");
  await app.press(app.pane().getByRole("button", { name: "Move", exact: true }));
  await expect(items(app, "2026-11-20")).toHaveText(["Alpha"]);
  await expect(items(app, "2026-11-02")).toHaveCount(0);
  await expectAccessible(page, ".dbme-calendar__selected");

  // Moved off the screen, the calendar goes with it.
  await app.pane().getByLabel("Move “Alpha” to").fill("2027-01-15");
  await app.press(app.pane().getByRole("button", { name: "Move", exact: true }));
  await expect(title(app)).toHaveText("January 2027");
  await expect(items(app, "2027-01-15")).toHaveText(["Alpha"]);
});

test("a second press on an item opens it", async ({ app }) => {
  const alpha = items(app, "2026-11-02").first();
  await app.press(alpha);
  await expect(alpha).toHaveAttribute("aria-pressed", "true");
  await app.press(alpha);
  await app.view("Document");
  await expect(app.title()).toHaveValue("Alpha");
});

test("New item goes on its day in one undo", async ({ app, page }) => {
  await app.press(app.pane().getByRole("button", { name: "New item" }));
  const dialog = page.getByRole("dialog", { name: "New item" });
  await expect(dialog.getByRole("button", { name: "Add item" })).toBeDisabled();
  await expect(dialog.getByLabel("Due")).toHaveValue("2026-11-04");
  await expectAccessible(page, "dialog");
  await dialog.getByLabel("Title").fill("Delta");
  await dialog.getByLabel("Due").fill("2026-11-06");
  await app.press(dialog.getByRole("button", { name: "Add item" }));
  await expect(dialog).toBeHidden();
  await expect(items(app, "2026-11-06")).toHaveText(["Delta"]);

  await page.getByRole("button", { name: "Undo" }).click();
  await expect(items(app, "2026-11-06")).toHaveCount(0);
});

test("a filter narrows every day", async ({ app }) => {
  await app.press(app.pane().getByRole("button", { name: /^Filter/ }));
  const dialog = app.page.getByRole("dialog", { name: "Filter" });
  await app.press(dialog.getByRole("button", { name: "Add condition" }));
  await dialog.getByLabel("Field").selectOption({ label: "Title" });
  await dialog.getByLabel("Value").fill("Bet");
  await dialog.getByLabel("Comparison").selectOption({ label: "starts with" });
  await app.press(dialog.getByRole("button", { name: "Apply" }));
  await expect(items(app, "2026-11-04")).toHaveText(["Beta"]);
  await expect(items(app, "2026-11-02")).toHaveCount(0);
});

test("Show as list is the same items with their dates written out", async ({
  app,
  page,
  isMobile,
}) => {
  await app.pane().getByRole("checkbox", { name: "Show as list" }).check();
  const grid = app.pane().getByRole("grid", { name: "Dated items" });
  await expect(grid.getByRole("columnheader")).toHaveText(["Title", "Due", "Starts", "Estimate"]);
  const alpha = grid.locator(".dbme-grid__body [role=row]").filter({ hasText: "Alpha" });
  await expect(alpha.locator('[aria-colindex="2"]')).toHaveText("Monday, November 2, 2026");
  await expect(alpha.locator('[aria-colindex="4"]')).toHaveText("3");
  const rows = grid.locator(".dbme-grid__body [role=row]");
  await expect(rows.filter({ hasText: "Later" })).toHaveCount(0);
  // What the grid shows of the next and last months is there too; a phone shows the month alone.
  await expect(rows.filter({ hasText: "Outside" })).toHaveCount(isMobile ? 0 : 1);
  await expectAccessible(page, ".dbme-calendar");
  if (!isMobile) {
    await grid.focus();
    await page.keyboard.press("Enter");
    await app.view("Document");
    await expect(app.title()).toHaveValue("Outside");
  }
});

test("a saved calendar keeps what it goes by", async ({ app, isMobile }) => {
  await app.pane().getByLabel("By").selectOption({ label: "Starts" });
  await app.press(app.pane().getByRole("button", { name: "Save view" }));
  const dialog = app.page.getByRole("dialog", { name: "Save view" });
  await dialog.getByLabel("Name").fill("By start");
  await app.press(dialog.getByRole("button", { name: "Save", exact: true }));
  await expect(dialog).toBeHidden();
  await app.pane().getByLabel("Saved view").selectOption({ label: "None" });
  await expect(app.pane().getByLabel("By")).toHaveValue("due");
  await app.pane().getByLabel("Saved view").selectOption({ label: "By start" });
  await expect(app.pane().getByLabel("By")).toHaveValue("starts");
  if (!isMobile) await expect(items(app, "2026-11-20")).toHaveText(["Alpha"]);
});

test.describe("on a phone", () => {
  test.skip(({ isMobile }) => !isMobile, "the agenda");

  test("the month is an agenda of the days with something on them", async ({ app, page }) => {
    await expect(app.pane().locator(".dbme-calendar__agenda-date")).toHaveText([
      "Monday, November 2, 2026",
      "Wednesday, November 4, 2026",
      "Tuesday, November 10, 2026",
    ]);
    await expect(items(app, "2026-11-04")).toHaveText(["Beta", "Gamma"]);
    await expect(items(app, "2026-11-10")).toHaveCount(5);
    await expect(app.pane().getByRole("grid")).toHaveCount(0);
    await expect(app.pane().getByLabel("Period")).toHaveCount(0);
    await expectAccessible(page, ".dbme-calendar");
    const wider = await page.evaluate(
      () => document.documentElement.scrollWidth > document.documentElement.clientWidth,
    );
    expect(wider).toBe(false);
  });

  test("a month with nothing dated says so", async ({ app }) => {
    await app.press(app.pane().getByRole("button", { name: "Previous month" }));
    await app.press(app.pane().getByRole("button", { name: "Previous month" }));
    await expect(title(app)).toHaveText("September 2026");
    await expect(app.pane().getByText("Nothing is dated in September 2026.")).toBeVisible();
  });

  test("it holds in dark", async ({ app, page }) => {
    await app.ipc("settings_set", { patch: { theme: "dark" } });
    await page.reload();
    await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
    await app.view("Calendar");
    await expect(items(app, "2026-11-02")).toHaveText(["Alpha"]);
    await expectAccessible(page, ".dbme-calendar");
  });
});
