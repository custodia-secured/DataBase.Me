// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * The usability scenario of the MVP's acceptance (brief §8; M7, ADR-0028), in the brief's five
 * steps and the order a person would take them: capture (a note typed on today's page in Simple
 * mode), organize (Organizer, a type and a field, moved under a page), find (the palette by a
 * word in the text), edit a file outside the app (the files folder on, the page's file changed
 * on disk, the change in the app), export (the reader files and the full export written). The
 * same steps are written for a person in docs/user-guide/first-ten-minutes.md.
 */
import type { Locator } from "@playwright/test";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { workspace } from "./data";
import { expect, expectAccessible, test, type App } from "./fixtures";

const ROOT = path.resolve(import.meta.dirname, "../../..");

/** The local date as the journal titles it. */
function today(): string {
  const now = new Date();
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${String(now.getFullYear())}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`;
}

/** Opens the palette the way the project does: Ctrl+K on a desktop, the Search button by touch. */
async function openPalette(app: App): Promise<Locator> {
  if (app.phone) await app.press(app.page.getByRole("button", { name: "Search", exact: true }));
  else await app.page.keyboard.press("Control+k");
  const box = app.page.getByRole("combobox", { name: "Search or type > for a command" });
  await expect(box).toBeFocused();
  return box;
}

async function runCommand(app: App, label: string): Promise<void> {
  const box = await openPalette(app);
  await box.fill(`> ${label}`);
  const option = app.page.getByRole("option", { name: label, exact: true });
  await expect(option).toBeVisible();
  await app.press(option);
}

/** On a phone the sidebar is a drawer, which must close before the page behind it is used. */
async function closeDrawer(app: App): Promise<void> {
  const close = app.page.getByRole("button", { name: "Close pages" });
  if (await close.isVisible()) await app.press(close);
}

/** Presses "Sync now" and waits for the sync to finish: the button is enabled again. */
async function syncNow(app: App, dialog: Locator): Promise<void> {
  const button = dialog.getByRole("button", { name: "Sync now" });
  await app.press(button);
  await expect(button).toBeEnabled();
  await expect(dialog.getByRole("status").first()).toHaveText(/^On: \d+ synced\.$/);
}

test("capture, organize, find, edit a file outside the app, export", async ({
  seed,
  app,
  page,
  session,
}) => {
  // A vault with a few pages and one type, as after a first week; Simple mode, as onboarding
  // suggests it.
  await seed(workspace());
  await app.open({ mode: "simple" });
  const date = today();

  // 1. Capture: today's page, and a line typed on it.
  await runCommand(app, "Today");
  await expect(app.currentCrumb()).toHaveText(date);
  await closeDrawer(app);
  await app.focusEditor();
  await page.keyboard.type("Call the plumber about the kitchen tap");
  const journal = (
    await app.ipc<{ title: string; id: string }[]>("object_children", { parent: null })
  ).find((item) => item.title === "Journal");
  expect(journal).toBeDefined();
  const todayPage = (
    await app.ipc<{ title: string; id: string }[]>("object_children", { parent: journal?.id })
  ).find((item) => item.title === date);
  expect(todayPage).toBeDefined();
  await expect
    .poll(async () => {
      const rows = await app.ipc<{ content_json: { inline?: { text?: string }[] } }[]>(
        "blocks_list",
        { objectId: todayPage?.id },
      );
      return rows.flatMap((row) => row.content_json.inline ?? []).map((span) => span.text);
    })
    .toEqual(["Call the plumber about the kitchen tap"]);

  // 2. Organize: Organizer shows the types; the page gets a type and a field, and moves under
  // Inbox.
  await runCommand(app, "Mode: Organizer");
  const pane = app.pane();
  await pane.getByRole("combobox", { name: "Add a type" }).selectOption({ label: "Milestone" });
  await expect(pane.getByRole("list", { name: "Types" }).getByRole("listitem")).toHaveText([
    "Milestone",
  ]);
  await app.press(pane.getByRole("button", { name: "Status: empty" }));
  await pane.getByRole("combobox", { name: "Status" }).selectOption({ label: "To do" });
  await expect(pane.getByRole("button", { name: "Status: To do" })).toBeVisible();

  // The page's row in the tree is under Journal, folded until Journal is opened.
  await app.openPage("Journal", date);
  await app.press((await app.sidebar()).getByRole("button", { name: `Actions for ${date}` }));
  await app.press(page.getByRole("menuitem", { name: "Move to…" }));
  const move = page.getByRole("dialog", { name: `Move “${date}” to…` });
  await expect(move).toBeVisible();
  await move.getByRole("searchbox", { name: "Find a page by title" }).fill("Inbox");
  await app.press(move.getByRole("button", { name: "Inbox", exact: true }));
  await expect(move).toBeHidden();
  await expect(app.path()).toHaveText(["Inbox", date]);
  await closeDrawer(app);

  // 3. Find: a word from the text, from anywhere.
  await app.openPage("Home");
  await closeDrawer(app);
  const box = await openPalette(app);
  await box.fill("plumber");
  const hit = page.getByRole("option", { name: new RegExp(date) });
  await expect(hit).toContainText("plumber");
  await expect(hit.locator("mark")).toHaveText("plumber");
  await expectAccessible(page, ".dbme-dialog--palette");
  await app.press(hit);
  await expect(app.currentCrumb()).toHaveText(date);

  // 4. Edit a file outside the app: the files folder on, the page's file changed on disk, the
  // change in the app.
  await app.press(page.getByRole("button", { name: /^Files folder/ }));
  const files = page.getByRole("dialog", { name: "Files folder" });
  await expect(files).toBeVisible();
  await app.press(files.getByRole("button", { name: "Turn on" }));
  await expect(files.getByRole("status").first()).toContainText("On:");
  await syncNow(app, files);
  const file = path.join(
    ROOT,
    "target/e2e-data",
    session,
    "vaults/Main.dbme/files/Inbox",
    `${date}.md`,
  );
  expect(readFileSync(file, "utf8")).toContain("Call the plumber about the kitchen tap");
  writeFileSync(file, `${readFileSync(file, "utf8").trimEnd()}\n\nAlso the bathroom tap.\n`);
  await syncNow(app, files);
  await app.press(files.getByRole("button", { name: "Close" }));
  await expect(app.editor()).toContainText("Also the bathroom tap.");
  await expect(app.editor()).toContainText("Call the plumber about the kitchen tap");

  // 5. Export: the reader files and, with the full export, the change log, from Settings.
  await app.press(page.getByRole("button", { name: "Settings", exact: true }));
  const settings = page.getByRole("dialog", { name: "Settings" });
  await expect(settings).toBeVisible();
  const out = path.join(ROOT, "target/e2e-data", session, "export");
  await settings.getByRole("textbox", { name: /^Export this vault to a folder/ }).fill(out);
  await expect(settings.getByRole("checkbox", { name: /^Full export/ })).toBeChecked();
  await app.press(settings.getByRole("button", { name: "Export", exact: true }));
  await expect(
    settings.getByText(/^Exported: \d+ types, \d+ views, \d+ rows, \d+ ops, \d+ files\.$/),
  ).toBeVisible();
  await expectAccessible(page, ".dbme-settings");
  expect(existsSync(path.join(out, "schema/types.json"))).toBe(true);
  expect(readFileSync(path.join(out, "data/Milestone.csv"), "utf8")).toContain(date);
  expect(existsSync(path.join(out, "ops.jsonl"))).toBe(true);
  expect(existsSync(path.join(out, "vault_meta.json"))).toBe(true);
});
