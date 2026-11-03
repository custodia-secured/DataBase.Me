// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * The milestone's screenshots at both sizes (brief 301-303), written to `target/screenshots/`
 * and not committed. Nothing is asserted beyond what the flow needs; the pictures are the record.
 * Runs only when `DBME_SCREENSHOTS` is set, so the suite's time is not spent on it:
 *
 *   DBME_SCREENSHOTS=1 pnpm --filter @databaseme/desktop e2e screenshots --project=edge-desktop --project=edge-phone
 */
import { mkdirSync, rmSync, writeFileSync } from "node:fs";
import path from "node:path";
import { workspace } from "./data";
import { expect, test, type App } from "./fixtures";

const ROOT = path.resolve(import.meta.dirname, "../../..");
const OUT = path.join(ROOT, "target/screenshots");

function folderOf(session: string): string {
  return path.join(ROOT, "target/e2e-data", session, "vaults/Main.dbme/files");
}

async function shot(app: App, milestone: string, name: string) {
  const size = app.page.viewportSize()?.width === 360 ? "phone" : "desktop";
  mkdirSync(OUT, { recursive: true });
  await app.page.screenshot({
    path: path.join(OUT, `${milestone}-${size}-${name}.png`),
    fullPage: false,
  });
}

async function openFiles(app: App) {
  await app.press(app.page.getByRole("button", { name: /^Files folder/ }));
  const dialog = app.page.getByRole("dialog", { name: "Files folder" });
  await expect(dialog).toBeVisible();
  return dialog;
}

async function syncNow(app: App, dialog: ReturnType<App["page"]["getByRole"]>) {
  const button = dialog.getByRole("button", { name: "Sync now" });
  await app.press(button);
  await expect(button).toBeEnabled();
}

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

test.beforeEach(async ({ seed, app }) => {
  test.skip(!process.env.DBME_SCREENSHOTS, "set DBME_SCREENSHOTS=1 to take the screenshots");
  await seed(workspace());
  await app.open();
});

test("M7: the Document, Mind map and Table views in light and in dark", async ({ app }) => {
  await app.openPage("Home", "Alpha");
  const close = app.page.getByRole("button", { name: "Close pages" });
  if (await close.isVisible()) await app.press(close);
  await expect(app.pane().getByRole("button", { name: "Status: Doing" })).toBeVisible();
  await shot(app, "m7", "light-document");
  await app.openPage("Home");
  if (await close.isVisible()) await app.press(close);
  await app.view("Mind map");
  await expect(
    app.pane().getByRole("tree", { name: "Mind map" }).getByRole("treeitem"),
  ).toHaveCount(5);
  await shot(app, "m7", "light-mindmap");

  await runCommand(app, "Theme: dark");
  await expect(app.page.locator("html")).toHaveAttribute("data-theme", "dark");
  await app.view("Table");
  await expect(app.pane().getByText("3 rows")).toBeVisible();
  await shot(app, "m7", "dark-table");
  await app.openPage("Home", "Alpha");
  if (await close.isVisible()) await app.press(close);
  await app.view("Document");
  await expect(app.pane().getByRole("button", { name: "Status: Doing" })).toBeVisible();
  await shot(app, "m7", "dark-document");
});

test("the files folder: off, on and synced, a conflict, and a hold", async ({ app, session }) => {
  const dialog = await openFiles(app);
  await shot(app, "m5", "files-off");
  await app.press(dialog.getByRole("button", { name: "Turn on" }));
  await syncNow(app, dialog);
  await expect(dialog.getByRole("status").first()).toHaveText(/^On: \d+ synced\.$/);
  await shot(app, "m5", "files-on");

  // An unreadable file in the inbox.
  const folder = folderOf(session);
  writeFileSync(path.join(folder, "Broken.md"), "---\nid: [not: valid\n---\n");
  await syncNow(app, dialog);
  await expect(dialog.getByRole("list", { name: "Conflicts" }).getByRole("listitem")).toHaveCount(
    1,
  );
  await shot(app, "m5", "files-inbox");
  rmSync(path.join(folder, "Broken.md"));

  // Many files removed at once: the hold and its banner.
  for (let n = 0; n < 50; n++) writeFileSync(path.join(folder, `Note ${String(n)}.md`), "x\n");
  await syncNow(app, dialog);
  for (let n = 0; n < 50; n++) rmSync(path.join(folder, `Note ${String(n)}.md`));
  await syncNow(app, dialog);
  await expect(dialog.getByText("Many files changed at once")).toBeVisible();
  await shot(app, "m5", "files-hold");
  await app.press(dialog.getByRole("button", { name: "Close" }));
  const close = app.page.getByRole("button", { name: "Close pages" });
  if (await close.isVisible()) await app.press(close);
  await expect(app.page.getByRole("region", { name: "Files folder hold" })).toBeVisible();
  await shot(app, "m5", "hold-banner");
});

test("importing: the plan, the receipt, and where a page came from", async ({ app, session }) => {
  const drop = path.join(ROOT, "target/e2e-data", session, "drop");
  mkdirSync(drop, { recursive: true });
  const csv = path.join(drop, "people.csv");
  const note = path.join(drop, "notes.txt");
  writeFileSync(csv, "name,age,joined\nAda,36,2021-03-04\nLin,41,2019-11-30\n");
  writeFileSync(note, "A plain text file.\n");

  await app.press(app.page.getByRole("button", { name: "Import", exact: true }));
  const dialog = app.page.getByRole("dialog", { name: "Import" });
  await expect(dialog).toBeVisible();
  await dialog
    .getByRole("textbox", { name: "Files or folders to import, one per line" })
    .fill(`${csv}\n${note}\n`);
  await app.press(dialog.getByRole("button", { name: "Preview" }));
  await expect(dialog.getByRole("status")).toHaveText("4 objects from 2 sources.");
  await shot(app, "m6", "import-plan");
  await app.press(dialog.getByRole("button", { name: "Import 4 objects" }));
  await expect(dialog.getByRole("status")).toHaveText("Imported 4 objects.");
  await shot(app, "m6", "import-receipt");
  await app.press(dialog.getByRole("button", { name: "Open the receipt" }));
  await expect(dialog).toBeHidden();
  const receipt = (await app.currentCrumb().textContent()) ?? "";
  await app.openPage(receipt, "notes");
  const close = app.page.getByRole("button", { name: "Close pages" });
  if (await close.isVisible()) await app.press(close);
  await expect(app.pane().getByText(/^Imported from notes\.txt/)).toBeVisible();
  await shot(app, "m6", "origin");
});
