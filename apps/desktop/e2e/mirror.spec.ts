// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * The files folder (ADR-0021, M5): turning it on, a page reaching its file, a file reaching the
 * app, the bulk-change hold and its banner, and the Conflicts inbox. The e2e server has no
 * worker, so the UI's "Sync now" drives both directions (M5 Decision 25); the test reaches the
 * folder on disk, under the server's data folder, with Node.
 */
import type { Locator } from "@playwright/test";
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import path from "node:path";
import { workspace } from "./data";
import { expect, expectAccessible, test, type App } from "./fixtures";

const ROOT = path.resolve(import.meta.dirname, "../../..");

/** The session's files folder: `files/` inside its default vault. */
function folderOf(session: string): string {
  return path.join(ROOT, "target/e2e-data", session, "vaults/Main.dbme/files");
}

async function openFiles(app: App) {
  await app.press(app.page.getByRole("button", { name: /^Files folder/ }));
  const dialog = app.page.getByRole("dialog", { name: "Files folder" });
  await expect(dialog).toBeVisible();
  return dialog;
}

/** Presses "Sync now" and waits for the sync to finish: the button is enabled again. */
async function syncNow(app: App, dialog: Locator) {
  const button = dialog.getByRole("button", { name: "Sync now" });
  await app.press(button);
  await expect(button).toBeEnabled();
}

/** The status line once nothing is waiting: "On: n synced." */
const SETTLED = /^On: \d+ synced\.$/;

/** On a phone, a look at the tree leaves the Pages drawer open, and a modal drawer blocks the rest. */
async function closeDrawer(app: App) {
  const close = app.page.getByRole("button", { name: "Close pages" });
  if (await close.isVisible()) await app.press(close);
}

test.beforeEach(async ({ seed, app }) => {
  await seed(workspace());
  await app.open();
});

test("turns the folder on, writes the pages, and reads a file back", async ({ app, session }) => {
  const dialog = await openFiles(app);
  await expect(dialog.getByRole("status")).toContainText("Off");
  await app.press(dialog.getByRole("button", { name: "Turn on" }));
  await expect(dialog.getByRole("status").first()).toContainText("On:");
  await expectAccessible(app.page);

  // The first sync writes every page.
  await syncNow(app, dialog);
  await expect(dialog.getByRole("status").first()).toHaveText(SETTLED);
  const folder = folderOf(session);
  expect(readFileSync(path.join(folder, "Home.md"), "utf8")).toContain("id:");
  expect(existsSync(path.join(folder, "Home", "Alpha.md"))).toBe(true);

  // A file written by hand becomes a page on the next sync, and the tree shows it.
  writeFileSync(path.join(folder, "From a file.md"), "Written outside the app.\n");
  await syncNow(app, dialog);
  await app.press(dialog.getByRole("button", { name: "Close" }));
  await expect(await app.treeItem("From a file")).toBeVisible();
  await app.openPage("From a file");
  await expect(app.editor()).toContainText("Written outside the app.");
});

test("fifty files deleted at once are held, and the banner restores them", async ({
  app,
  session,
}) => {
  const folder = folderOf(session);
  mkdirSync(folder, { recursive: true });
  for (let n = 0; n < 50; n++) writeFileSync(path.join(folder, `Note ${String(n)}.md`), "x\n");
  const dialog = await openFiles(app);
  await app.press(dialog.getByRole("button", { name: "Turn on" }));
  await syncNow(app, dialog);
  await expect(dialog.getByRole("status").first()).toHaveText(SETTLED);
  expect(existsSync(path.join(folder, "Home.md"))).toBe(true);
  await app.press(dialog.getByRole("button", { name: "Close" }));
  await expect(await app.treeItem("Note 0")).toBeVisible();
  expect(
    await app.ipc<number>("objects_count", { source: { kind: "children", parent: null } }),
  ).toBe(52);

  for (let n = 0; n < 50; n++) rmSync(path.join(folder, `Note ${String(n)}.md`));
  await closeDrawer(app);
  await syncNow(app, await openFiles(app));
  const banner = app.page.getByRole("region", { name: "Files folder hold" });
  await expect(app.page.getByRole("dialog").getByText("Many files changed at once")).toBeVisible();
  await app.press(app.page.getByRole("button", { name: "Close" }));
  await expect(banner).toBeVisible();
  await expectAccessible(app.page);
  await expect(await app.treeItem("Note 0")).toBeVisible();
  await closeDrawer(app);

  await app.press(banner.getByRole("button", { name: "Restore the folder" }));
  await expect(banner).toBeHidden();
  await expect.poll(() => existsSync(path.join(folder, "Note 0.md"))).toBe(true);
  await expect(await app.treeItem("Note 0")).toBeVisible();
});

test("a file that cannot be read is in the inbox until it is dismissed", async ({
  app,
  session,
}) => {
  const dialog = await openFiles(app);
  await app.press(dialog.getByRole("button", { name: "Turn on" }));
  await syncNow(app, dialog);
  await expect(dialog.getByRole("status").first()).toHaveText(SETTLED);
  const folder = folderOf(session);
  expect(existsSync(path.join(folder, "Home.md"))).toBe(true);
  await expect(dialog.getByText("Nothing to resolve.")).toBeVisible();

  writeFileSync(path.join(folder, "Broken.md"), "---\nid: [not: valid\n---\n");
  await syncNow(app, dialog);
  const entry = dialog.getByRole("list", { name: "Conflicts" }).getByRole("listitem");
  await expect(entry).toHaveCount(1);
  await expect(entry).toContainText("File could not be read");
  await expect(entry).toContainText("Broken.md");
  await expectAccessible(app.page);
  await app.press(entry.getByRole("button", { name: "Done" }));
  await expect(dialog.getByText("Nothing to resolve.")).toBeVisible();
  await expect(app.page.getByRole("button", { name: "Files folder", exact: true })).toBeVisible();
});
