// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * Importing (ADR-0026, M6): the Import dialog's preview, apply, receipt and undo, and the
 * provenance line in the Document view. The e2e server has no native drop, so the tests type
 * paths under the session's data folder, as M5's files-folder tests reach their folder on disk.
 */
import type { Locator } from "@playwright/test";
import { mkdirSync, writeFileSync } from "node:fs";
import path from "node:path";
import { workspace } from "./data";
import { expect, expectAccessible, test, type App, type TreeIds } from "./fixtures";

const ROOT = path.resolve(import.meta.dirname, "../../..");

/** A folder of the session's own, which the server lets the UI read from. */
function dropFolder(session: string): string {
  const folder = path.join(ROOT, "target/e2e-data", session, "drop");
  mkdirSync(folder, { recursive: true });
  return folder;
}

async function openImport(app: App): Promise<Locator> {
  await app.press(app.page.getByRole("button", { name: "Import", exact: true }));
  const dialog = app.page.getByRole("dialog", { name: "Import" });
  await expect(dialog).toBeVisible();
  return dialog;
}

function pathsBox(dialog: Locator): Locator {
  return dialog.getByRole("textbox", { name: "Files or folders to import, one per line" });
}

/** On a phone, a look at the tree leaves the Pages drawer open, and a modal drawer blocks the rest. */
async function closeDrawer(app: App) {
  const close = app.page.getByRole("button", { name: "Close pages" });
  if (await close.isVisible()) await app.press(close);
}

let ids: TreeIds;

test.beforeEach(async ({ seed, app }) => {
  ids = await seed(workspace());
  await app.open();
});

test("previews, imports a table and a file, opens the receipt and shows provenance", async ({
  app,
  session,
}) => {
  const folder = dropFolder(session);
  const csv = path.join(folder, "people.csv");
  const note = path.join(folder, "notes.txt");
  writeFileSync(csv, "name,age,joined\nAda,36,2021-03-04\nLin,41,2019-11-30\n");
  writeFileSync(note, "A plain text file.\n");
  const before = await app.ipc<number>("objects_count", {
    source: { kind: "children", parent: null },
  });

  const dialog = await openImport(app);
  await pathsBox(dialog).fill(`${csv}\n${note}\n`);
  await app.press(dialog.getByRole("button", { name: "Preview" }));
  const sources = dialog.getByRole("list", { name: "Sources" });
  await expect(sources.getByRole("listitem")).toHaveCount(2);
  await expect(sources).toContainText("people.csv");
  await expect(sources).toContainText("people.csv (CSV): 2 rows of type people, under a Dataset");
  await expect(sources).toContainText("notes.txt");
  await expect(sources).toContainText("notes.txt (file): a Document");
  await expect(dialog.getByRole("status")).toHaveText("4 objects from 2 sources.");
  await expectAccessible(app.page, ".dbme-import");

  // Nothing is written by the preview.
  expect(
    await app.ipc<number>("objects_count", { source: { kind: "children", parent: null } }),
  ).toBe(before);

  await app.press(dialog.getByRole("button", { name: "Import 4 objects" }));
  await expect(dialog.getByRole("status")).toHaveText("Imported 4 objects.");
  await expectAccessible(app.page, ".dbme-import");
  await app.press(dialog.getByRole("button", { name: "Open the receipt" }));
  await expect(dialog).toBeHidden();

  // The receipt is a new top-level page named for the day, with the run in its text.
  await expect(app.currentCrumb()).toHaveText(/^Import \d{4}-\d{2}-\d{2}$/);
  const receipt = (await app.currentCrumb().textContent()) ?? "";
  await expect(app.editor()).toContainText("4 objects");
  expect(
    await app.ipc<number>("objects_count", { source: { kind: "children", parent: null } }),
  ).toBe(before + 1);

  // The file's page says where it came from and carries the file.
  await app.openPage(receipt, "notes");
  await closeDrawer(app);
  const document = app.pane().getByRole("article", { name: "notes" });
  await expect(
    document.getByText(/^Imported from notes\.txt on .+ by the file importer$/),
  ).toBeVisible();
  const files = document.getByRole("list", { name: "Attachments" });
  await expect(files.getByRole("listitem")).toHaveCount(1);
  await expect(files).toContainText("notes.txt");

  // The table's rows are objects of the new type under the file's page.
  await app.openPage(receipt, "people");
  await closeDrawer(app);
  await app.view("Table");
  const grid = app.pane().getByRole("grid", { name: "Rows" });
  await expect(grid).toContainText("Ada");
  await expect(grid).toContainText("Lin");
  const rows = await app.ipc<{ object: { id: string } }[]>("object_search", {
    text: "Ada",
    limit: 5,
  });
  expect(rows).toHaveLength(1);
});

test("a second import of the same file is left out, and a missing path is refused", async ({
  app,
  session,
}) => {
  const folder = dropFolder(session);
  const note = path.join(folder, "again.txt");
  writeFileSync(note, "Twice.\n");
  const dialog = await openImport(app);
  await pathsBox(dialog).fill(note);
  await app.press(dialog.getByRole("button", { name: "Preview" }));
  await app.press(dialog.getByRole("button", { name: "Import 1 object" }));
  await expect(dialog.getByRole("status")).toHaveText("Imported 1 object.");

  // Typing again drops the receipt; the new preview knows the file.
  const missing = path.join(folder, "nowhere.txt");
  await pathsBox(dialog).fill(`${note}\n${missing}`);
  await app.press(dialog.getByRole("button", { name: "Preview" }));
  await expect(dialog.getByRole("status")).toHaveText("Nothing to import.");
  const duplicates = dialog.getByRole("list", { name: "Already in the vault" });
  await expect(duplicates.getByRole("listitem")).toHaveCount(1);
  await expect(duplicates).toContainText("again.txt is already here as again");
  const refused = dialog.getByRole("list", { name: "Could not be read" });
  await expect(refused.getByRole("listitem")).toHaveCount(1);
  await expect(refused).toContainText("nowhere.txt could not be read");
  await expect(dialog.getByRole("button", { name: /^Import/ })).toBeDisabled();
  await expectAccessible(app.page, ".dbme-import");
});

test("an import lands under the open page when asked, and undo takes it back", async ({
  app,
  session,
}) => {
  const folder = dropFolder(session);
  const note = path.join(folder, "under.txt");
  writeFileSync(note, "Under Home.\n");
  await app.openPage("Home");
  await closeDrawer(app);
  const homeId = ids.ids.Home;
  expect(homeId).toBeDefined();
  const childrenBefore = await app.ipc<number>("objects_count", {
    source: { kind: "children", parent: homeId },
  });

  const dialog = await openImport(app);
  await app.press(dialog.getByRole("checkbox", { name: /^Put them under Home/ }));
  await pathsBox(dialog).fill(note);
  await app.press(dialog.getByRole("button", { name: "Preview" }));
  await app.press(dialog.getByRole("button", { name: "Import 1 object" }));
  await expect(dialog.getByRole("status")).toHaveText("Imported 1 object.");
  expect(
    await app.ipc<number>("objects_count", { source: { kind: "children", parent: homeId } }),
  ).toBe(childrenBefore + 1);

  await app.press(dialog.getByRole("button", { name: "Undo the import" }));
  await expect(dialog.getByRole("button", { name: "Preview" })).toBeVisible();
  expect(
    await app.ipc<number>("objects_count", { source: { kind: "children", parent: homeId } }),
  ).toBe(childrenBefore);
});
