// SPDX-License-Identifier: AGPL-3.0-or-later
/** The shell: the hierarchy sidebar, the path, the view tabs, the phone drawer, undo and redo. */
import type { Locator, Page } from "@playwright/test";
import { workspace } from "./data";
import { expect, expectAccessible, test, type App } from "./fixtures";

test.beforeEach(async ({ seed, app }) => {
  await seed(workspace());
  await app.open();
});

async function treeTitles(app: App): Promise<Locator> {
  return (await app.tree()).getByRole("treeitem");
}

test("opens the first page and shows the hierarchy", async ({ app }) => {
  await expect(app.title()).toHaveValue("Home");
  await expect(app.currentCrumb()).toHaveText("Home");
  await expect(await treeTitles(app)).toHaveText(["Home", "Inbox"]);
  await expect(await app.treeItem("Home")).toHaveAttribute("aria-selected", "true");
  await expectAccessible(app.page);
});

test("the tree works with the keyboard alone", async ({ app, page }) => {
  const tree = await app.tree();
  await tree.focus();
  const active = () => app.activeText(tree);
  await expect.poll(active).toBe("Home");
  await page.keyboard.press("ArrowDown");
  await expect.poll(active).toBe("Inbox");
  await page.keyboard.press("Home");
  await page.keyboard.press("ArrowRight");
  await expect(await app.treeItem("Home")).toHaveAttribute("aria-expanded", "true");
  await expect(await treeTitles(app)).toHaveText(["Home", "Alpha", "Beta", "Gamma", "Inbox"]);
  await page.keyboard.press("ArrowRight");
  await expect.poll(active).toBe("Alpha");
  await page.keyboard.press("ArrowDown");
  await expect.poll(active).toBe("Beta");
  // Alt+↓ moves the page below its next sibling, and focus stays with it.
  await page.keyboard.press("Alt+ArrowDown");
  await expect(await treeTitles(app)).toHaveText(["Home", "Alpha", "Gamma", "Beta", "Inbox"]);
  await expect.poll(active).toBe("Beta");
  await page.keyboard.press("End");
  await expect.poll(active).toBe("Inbox");
  await page.keyboard.press("ArrowUp");
  await page.keyboard.press("ArrowLeft");
  await expect.poll(active).toBe("Home");
  await page.keyboard.press("ArrowLeft");
  await expect(await app.treeItem("Home")).toHaveAttribute("aria-expanded", "false");
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("Enter");
  await expect(app.currentCrumb()).toHaveText("Inbox");
});

test("makes a page, names it, and moves it with Move to…, by keyboard", async ({ app, page }) => {
  const sidebar = await app.sidebar("keyboard");
  await sidebar.getByRole("button", { name: "New page", exact: true }).focus();
  await page.keyboard.press("Enter");
  await expect(app.currentCrumb()).toHaveText("Untitled");
  await app.title().focus();
  await page.keyboard.type("Plans");
  await page.keyboard.press("Enter");
  await expect(app.currentCrumb()).toHaveText("Plans");

  await (await app.sidebar("keyboard")).getByRole("button", { name: "Actions for Plans" }).focus();
  await page.keyboard.press("Enter");
  await expect(page.getByRole("menuitem", { name: "New page inside" })).toBeFocused();
  await page.keyboard.press("ArrowDown");
  await expect(page.getByRole("menuitem", { name: "Move to…" })).toBeFocused();
  await page.keyboard.press("Enter");

  const dialog = page.getByRole("dialog", { name: "Move “Plans” to…" });
  await expect(dialog).toBeVisible();
  await expect(dialog.getByRole("searchbox", { name: "Find a page by title" })).toBeFocused();
  await page.keyboard.type("Inbox");
  const choice = dialog.getByRole("button", { name: "Inbox", exact: true });
  await expect(choice).toBeVisible();
  await expectAccessible(page);
  await page.keyboard.press("Tab");
  await expect(dialog.getByRole("button", { name: "Top level" })).toBeFocused();
  await page.keyboard.press("Tab");
  await expect(choice).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(dialog).toBeHidden();
  await expect(app.path()).toHaveText(["Inbox", "Plans"]);
});

test("moves a page with Move to…, by touch or pointer", async ({ app, page }) => {
  await app.openPage("Inbox");
  await app.press((await app.sidebar()).getByRole("button", { name: "Actions for Inbox" }));
  await app.press(page.getByRole("menuitem", { name: "Move to…" }));
  const dialog = page.getByRole("dialog", { name: "Move “Inbox” to…" });
  await expect(dialog).toBeVisible();
  await dialog.getByRole("searchbox", { name: "Find a page by title" }).fill("Home");
  await app.press(dialog.getByRole("button", { name: "Home", exact: true }));
  await expect(dialog).toBeHidden();
  await expect(app.path()).toHaveText(["Home", "Inbox"]);
  await app.press((await app.treeItem("Home")).locator(".dbme-tree__toggle"));
  await expect(await treeTitles(app)).toHaveText(["Home", "Alpha", "Beta", "Gamma", "Inbox"]);
});

test("the view tabs follow the arrow keys", async ({ app, page }) => {
  const tabs = app.pane().getByRole("tablist", { name: "Views" });
  await tabs.getByRole("tab", { name: "Document" }).focus();
  await page.keyboard.press("ArrowRight");
  await expect(tabs.getByRole("tab", { name: "Outline" })).toBeFocused();
  await expect(tabs.getByRole("tab", { name: "Outline" })).toHaveAttribute("aria-selected", "true");
  await expect(app.pane().getByRole("region", { name: "Outline" })).toBeVisible();
  await page.keyboard.press("End");
  await expect(tabs.getByRole("tab", { name: "Gallery" })).toHaveAttribute("aria-selected", "true");
  await page.keyboard.press("ArrowRight");
  await expect(tabs.getByRole("tab", { name: "Document" })).toHaveAttribute(
    "aria-selected",
    "true",
  );
  await page.keyboard.press("ArrowLeft");
  await page.keyboard.press("Home");
  await expect(tabs.getByRole("tab", { name: "Document" })).toBeFocused();
});

test("undo and redo reverse a change", async ({ app, page }) => {
  await app.title().focus();
  await page.keyboard.press("End");
  await page.keyboard.type(" base");
  await page.keyboard.press("Enter");
  await expect(app.currentCrumb()).toHaveText("Home base");

  // The shortcuts work anywhere but in a text field.
  await app.pane().getByRole("tab", { name: "Document" }).focus();
  await page.keyboard.press("ControlOrMeta+z");
  await expect(app.title()).toHaveValue("Home");
  await page.keyboard.press("ControlOrMeta+Shift+z");
  await expect(app.title()).toHaveValue("Home base");

  await app.press(page.getByRole("button", { name: "Undo" }));
  await expect(app.title()).toHaveValue("Home");
  await app.press(page.getByRole("button", { name: "Redo" }));
  await expect(app.title()).toHaveValue("Home base");
});

test.describe("on a phone", () => {
  test.skip(({ isMobile }) => !isMobile, "the drawer exists below 768 px");

  test("the pages are in a drawer that closes on a choice or Escape", async ({ app, page }) => {
    await expect(page.getByRole("tree", { name: "Pages" })).toBeHidden();
    await app.openPage("Inbox");
    const drawer = page.getByRole("dialog", { name: "Pages" });
    await expect(drawer).toBeHidden();
    await app.sidebar();
    await expectAccessible(page);
    await page.keyboard.press("Escape");
    await expect(drawer).toBeHidden();
    await app.sidebar();
    await app.press(drawer.getByRole("button", { name: "Close pages" }));
    await expect(drawer).toBeHidden();
  });
});

/** The centre of an element on the page. */
async function centre(target: Locator): Promise<{ x: number; y: number }> {
  const box = await target.boundingBox();
  if (!box) throw new Error("the element is not on screen");
  return { x: box.x + box.width / 2, y: box.y + box.height / 2 };
}

/** A long press on `from`, then a drag to `to`, with real touch events (Chromium only). */
async function touchDrag(page: Page, from: { x: number; y: number }, to: { x: number; y: number }) {
  const cdp = await page.context().newCDPSession(page);
  type TouchType = "touchStart" | "touchMove" | "touchEnd";
  const touch = (type: TouchType, point?: { x: number; y: number }) =>
    cdp.send("Input.dispatchTouchEvent", { type, touchPoints: point ? [point] : [] });
  await touch("touchStart", from);
  await page.waitForTimeout(450);
  const steps = 12;
  for (let step = 1; step <= steps; step++) {
    await touch("touchMove", {
      x: from.x + ((to.x - from.x) * step) / steps,
      y: from.y + ((to.y - from.y) * step) / steps,
    });
  }
  await touch("touchEnd");
  await cdp.detach();
}

test("dragging a page onto another puts it inside", async ({
  app,
  page,
  isMobile,
  browserName,
}) => {
  test.skip(
    isMobile && browserName === "webkit",
    "Playwright cannot send touch drags to WebKit; Move to… is the touch path tested there",
  );
  await app.openPage("Home");
  await app.press((await app.treeItem("Home")).locator(".dbme-tree__toggle"));
  const from = await centre(await app.treeItem("Gamma"));
  const to = await centre(await app.treeItem("Inbox"));
  if (isMobile) {
    await touchDrag(page, from, to);
  } else {
    await page.mouse.move(from.x, from.y);
    await page.mouse.down();
    await page.mouse.move(from.x, from.y + 8, { steps: 4 });
    await page.mouse.move(to.x, to.y, { steps: 12 });
    await page.mouse.up();
  }
  await expect(await treeTitles(app)).toHaveText([
    "Home",
    "Alpha",
    "Beta",
    "Inbox",
    "Idea one",
    "Idea two",
    "Gamma",
  ]);
});
