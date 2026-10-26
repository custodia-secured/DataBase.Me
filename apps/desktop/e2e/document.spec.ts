// SPDX-License-Identifier: AGPL-3.0-or-later
/** The Document view: title, types, properties, the block editor, mentions and backlinks. */
import { workspace } from "./data";
import { expect, test, type App, type TreeIds } from "./fixtures";

interface BlockRow {
  id: string;
  parent_block_id: string | null;
  sort_key: string;
  kind: string;
  content_json: { inline?: { text?: string; mention?: string; bold?: true }[]; checked?: boolean };
}

interface ObjectRow {
  title: string;
  props_json: Record<string, unknown>;
}

let ids: TreeIds;

test.beforeEach(async ({ seed, app }) => {
  ids = await seed(workspace());
  await app.open();
});

function id(title: string): string {
  const found = ids.ids[title];
  if (found === undefined) throw new Error(`no seeded object ${title}`);
  return found;
}

/** The top-level blocks of an object as the vault holds them, in order. */
async function blocks(app: App, title: string): Promise<BlockRow[]> {
  const rows = await app.ipc<BlockRow[]>("blocks_list", { objectId: id(title) });
  return rows
    .filter((row) => row.parent_block_id === null)
    .sort((a, b) => (a.sort_key < b.sort_key ? -1 : a.sort_key > b.sort_key ? 1 : 0));
}

/** Leaves the to-do list Home's text ends with: a new item, then Enter again to lift it out. */
async function newLineAtEnd(app: App): Promise<void> {
  const keyboard = app.page.keyboard;
  await keyboard.press("ControlOrMeta+End");
  await keyboard.press("Enter");
  await keyboard.press("Enter");
}

test("writes and formats text by keyboard, and the vault keeps it", async ({ app, page }) => {
  await app.title().focus();
  await page.keyboard.press("Enter");
  await expect(app.editor()).toBeFocused();
  await app.focusEditor();
  await newLineAtEnd(app);
  await page.keyboard.type("Plain words and ");
  await page.keyboard.press("ControlOrMeta+b");
  await page.keyboard.type("strong ones");
  await page.keyboard.press("ControlOrMeta+b");
  await expect(app.editor().locator("strong")).toHaveText("strong ones");

  // The toolbar is one tab stop before the text; its arrow keys move between the buttons.
  await page.keyboard.press("Shift+Tab");
  const toolbar = app.pane().getByRole("toolbar", { name: "Formatting" });
  await expect(toolbar.getByRole("button", { name: "Bold" })).toBeFocused();
  await page.keyboard.press("ArrowRight");
  await page.keyboard.press("ArrowRight");
  await expect(toolbar.getByRole("button", { name: "Heading" })).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(toolbar.getByRole("button", { name: "Heading" })).toHaveAttribute(
    "aria-pressed",
    "true",
  );
  await expect(app.editor()).toBeFocused();

  await expect
    .poll(async () => {
      const last = (await blocks(app, "Home")).at(-1);
      return last && { kind: last.kind, inline: last.content_json.inline };
    })
    .toEqual({
      kind: "heading",
      inline: [{ text: "Plain words and " }, { text: "strong ones", bold: true }],
    });
});

test("[[ links to an object, and the link and its backlink both open", async ({ app, page }) => {
  await app.focusEditor();
  await newLineAtEnd(app);
  await page.keyboard.type("See [[Gam");
  const options = page.getByRole("listbox", { name: "Objects to mention" });
  await expect(options.getByRole("option", { name: "Gamma" })).toBeVisible();
  await expect(app.editor()).toHaveAttribute(
    "aria-controls",
    (await options.getAttribute("id")) ?? "",
  );
  await page.keyboard.press("Enter");
  await expect(options).toBeHidden();
  const mention = app.editor().locator(".dbme-mention", { hasText: "Gamma" });
  await expect(mention).toBeVisible();
  await expect
    .poll(async () => (await blocks(app, "Home")).at(-1)?.content_json.inline)
    .toEqual([{ text: "See " }, { mention: id("Gamma") }, { text: " " }]);

  await app.press(mention);
  await expect(app.currentCrumb()).toHaveText("Gamma");
  const linkedFrom = app.pane().getByRole("region", { name: /Linked from/ });
  await expect(linkedFrom.getByRole("heading")).toHaveText("Linked from (1)");
  await app.press(linkedFrom.getByRole("button", { name: /Home/ }));
  await expect(app.currentCrumb()).toHaveText("Home");
});

test("a seeded mention shows as a backlink with its snippet", async ({ app }) => {
  await app.openPage("Home", "Alpha");
  const linkedFrom = app.pane().getByRole("region", { name: /Linked from/ });
  await expect(linkedFrom.getByRole("button")).toHaveText(["HomeStart with …."]);
});

test("a mention shows its title again when the view comes back", async ({ app }) => {
  const mention = app.editor().locator(".dbme-mention");
  await expect(mention).toHaveText("Alpha");
  // The title is read once; the new editor must take it from what was read.
  await app.view("Table");
  await app.view("Document");
  await expect(mention).toHaveText("Alpha");
});

test("properties edit in place and save", async ({ app, page }) => {
  await app.openPage("Home", "Alpha");
  const pane = app.pane();

  await app.press(pane.getByRole("button", { name: "Status: Doing" }));
  const status = pane.getByRole("combobox", { name: "Status" });
  await expect(status).toBeFocused();
  if (app.phone) {
    await status.selectOption({ label: "Done" });
  } else {
    await page.keyboard.press("ArrowDown");
    await page.keyboard.press("Enter");
  }
  await expect(pane.getByRole("button", { name: "Status: Done" })).toBeVisible();

  await app.press(pane.getByRole("button", { name: "Estimate: 3" }));
  await expect(pane.getByRole("spinbutton", { name: "Estimate" })).toBeFocused();
  await page.keyboard.type("5");
  await page.keyboard.press("Enter");
  await expect(pane.getByRole("button", { name: "Estimate: 5" })).toBeVisible();

  await app.press(pane.getByRole("checkbox", { name: "Shipped" }));
  await expect(pane.getByRole("checkbox", { name: "Shipped" })).toBeChecked();

  await expect
    .poll(async () => (await app.ipc<ObjectRow>("object_get", { id: id("Alpha") })).props_json)
    .toEqual({ status: "done", due: "2026-11-02", estimate: 5, shipped: true });
});

test("a type can be added and removed", async ({ app }) => {
  await app.openPage("Inbox");
  const pane = app.pane();
  await pane.getByRole("combobox", { name: "Add a type" }).selectOption({ label: "Milestone" });
  const types = pane.getByRole("list", { name: "Types" });
  await expect(types.getByRole("listitem")).toHaveText(["Milestone"]);
  await expect(pane.getByRole("button", { name: "Status: empty" })).toBeVisible();
  await app.press(types.getByRole("button", { name: "Remove type Milestone" }));
  await expect(types.getByRole("listitem")).toHaveCount(0);
  await expect(pane.getByRole("button", { name: "Status: empty" })).toBeHidden();
});

test("the toolbar and to-do boxes work by touch or pointer", async ({ app }) => {
  const editor = app.editor();
  await app.press(editor.getByText("Welcome"));
  const heading = app.pane().getByRole("button", { name: "Heading" });
  await expect(heading).toHaveAttribute("aria-pressed", "true");
  await app.press(heading);
  await expect(heading).toHaveAttribute("aria-pressed", "false");
  await expect(editor.locator("h1, h2")).toHaveCount(0);

  const todo = editor.getByRole("checkbox", { name: /Write the plan/ });
  await app.press(todo);
  await expect(todo).toBeChecked();

  await expect
    .poll(async () => {
      const rows = await blocks(app, "Home");
      return [rows[0]?.kind, rows.at(-1)?.content_json.checked];
    })
    .toEqual(["paragraph", true]);
});
