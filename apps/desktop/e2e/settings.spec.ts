// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * Settings and Help (M7, ADR-0027): the mode, the theme and update checks change from the dialog
 * and the settings say so; another vault is made and opened from it; Help names the version and
 * where the docs and the issues are.
 */
import path from "node:path";
import { workspace } from "./data";
import { expect, expectAccessible, test, type App } from "./fixtures";

const ROOT = path.resolve(import.meta.dirname, "../../..");

async function openSettings(app: App) {
  await app.press(app.page.getByRole("button", { name: "Settings", exact: true }));
  const dialog = app.page.getByRole("dialog", { name: "Settings" });
  await expect(dialog).toBeVisible();
  return dialog;
}

test.beforeEach(async ({ seed, app }) => {
  await seed(workspace());
  await app.open();
});

test("the mode, the theme and update checks change from the dialog", async ({ app, page }) => {
  const dialog = await openSettings(app);
  await expect(dialog.getByText(/This vault:/)).toBeVisible();
  await expect(dialog.getByRole("radio", { name: /^Organizer/ })).toBeChecked();
  await expectAccessible(page, ".dbme-settings");

  await app.press(dialog.getByRole("radio", { name: /^Simple/ }));
  await app.press(dialog.getByRole("radio", { name: /^Dark/ }));
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
  await app.press(dialog.getByRole("checkbox", { name: /Check for updates/ }));
  await expect
    .poll(() => app.ipc<{ mode: string; theme: string; update_checks: boolean }>("settings_get"))
    .toMatchObject({ mode: "simple", theme: "dark", update_checks: true });

  // Help names the version and the places to go.
  await expect(dialog.getByText(/^DataBase\.Me \d+\.\d+\.\d+, vault format \d+\./)).toBeVisible();
  await expect(dialog.getByText("Documentation")).toBeVisible();
  await expect(
    dialog.getByText(/github\.com\/custodia-secured\/DataBase\.Me\/issues/),
  ).toBeVisible();

  await app.press(dialog.getByRole("button", { name: "Close" }));
  await expect(dialog).toBeHidden();
  // Simple mode shows in the pane at once.
  await expect(app.pane().getByRole("tab", { name: "Table" })).toHaveCount(0);
});

test("a new vault is made from the dialog, and the first one opened again", async ({
  app,
  page,
  session,
}) => {
  const before = await app.ipc<{ path: string }>("vault_info");
  const folder = path.join(ROOT, "target/e2e-data", session, "second");
  const dialog = await openSettings(app);
  await dialog.getByRole("textbox", { name: "Make a new vault in a folder" }).fill(folder);
  await app.press(dialog.getByRole("button", { name: "Make" }));
  await expect(dialog.getByText(/second[\\/]Main\.dbme/)).toBeVisible();
  const made = await app.ipc<{ path: string }>("vault_info");
  expect(made.path).not.toBe(before.path);
  await app.press(dialog.getByRole("button", { name: "Close" }));
  await expect(page.getByRole("heading", { name: "Nothing here yet" })).toBeVisible();

  const again = await openSettings(app);
  await again.getByRole("textbox", { name: "Open another vault (its folder)" }).fill(before.path);
  await app.press(again.getByRole("button", { name: "Open" }));
  await expect(again.getByText(/This vault:/)).toBeVisible();
  await expect
    .poll(() => app.ipc<{ path: string }>("vault_info"))
    .toMatchObject({
      path: before.path,
    });
  await app.press(again.getByRole("button", { name: "Close" }));
  await expect(await app.treeItem("Home")).toBeVisible();
  const settings = await app.ipc<{ last_vault: string }>("settings_get");
  expect(settings.last_vault).toBe(before.path);
});
