// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * Onboarding (M7, ADR-0028): on first launch the six steps choose where the vault goes, the
 * files folder, the mode, the theme and whether update checks are wanted, then the vault opens;
 * a typed folder puts the vault there; "Set up again" runs it with the vault open. The e2e build
 * has no folder picker, so the folders are typed, under the session's own folder.
 */
import path from "node:path";
import { expect, expectAccessible, test, type App } from "./fixtures";

const ROOT = path.resolve(import.meta.dirname, "../../..");

interface Settings {
  mode: string;
  theme: string;
  update_checks: boolean;
  last_vault: string | null;
  onboarded: boolean;
}

function next(app: App) {
  return app.press(app.page.getByRole("button", { name: "Next" }));
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

test("first launch: the default vault, the files folder inside it, Simple, dark", async ({
  app,
  page,
}) => {
  await app.open({ onboarding: true });
  await expect(page.getByRole("status")).toHaveText("Step 1 of 6");
  await expectAccessible(page);
  await next(app);

  const defaultChoice = page.getByRole("radio", { name: /In the app's own folder/ });
  await expect(defaultChoice).toBeChecked();
  await expect(page.getByText(/vaults[\\/]Main\.dbme/)).toBeVisible();
  await next(app);
  await expect(page.getByRole("radio", { name: /Inside the vault/ })).toBeChecked();
  await next(app);
  await expect(page.getByRole("radio", { name: /^Simple/ })).toBeChecked();
  await next(app);
  await app.press(page.getByRole("radio", { name: /^Dark/ }));
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
  await next(app);
  await expect(page.getByRole("checkbox", { name: /Check for updates/ })).not.toBeChecked();
  await expectAccessible(page);
  await app.press(page.getByRole("button", { name: "Finish" }));

  // The vault is open and empty; the choices are saved; the files folder is on.
  await expect(page.getByRole("heading", { name: "Nothing here yet" })).toBeVisible();
  const settings = await app.ipc<Settings>("settings_get");
  expect(settings).toMatchObject({
    mode: "simple",
    theme: "dark",
    update_checks: false,
    onboarded: true,
  });
  expect(settings.last_vault).toMatch(/Main\.dbme$/);
  const status = await app.ipc<{ enabled: boolean }>("mirror_status");
  expect(status.enabled).toBe(true);
  // The reload goes straight to the vault.
  await page.reload();
  await expect(page.getByRole("heading", { name: "Nothing here yet" })).toBeVisible();
});

test("a folder of my own, the files folder off, Organizer", async ({ app, page, session }) => {
  const folder = path.join(ROOT, "target/e2e-data", session, "mine");
  await app.open({ onboarding: true });
  await next(app);
  await app.press(page.getByRole("radio", { name: /In a folder I choose/ }));
  await page.getByRole("textbox", { name: "Folder" }).fill(folder);
  await next(app);
  await app.press(page.getByRole("radio", { name: /Off for now/ }));
  await next(app);
  await app.press(page.getByRole("radio", { name: /^Organizer/ }));
  await next(app);
  await next(app);
  await app.press(page.getByRole("button", { name: "Finish" }));
  await expect(page.getByRole("heading", { name: "Nothing here yet" })).toBeVisible();

  const info = await app.ipc<{ path: string }>("vault_info");
  expect(info.path.replace(/\\/g, "/")).toBe(`${folder.replace(/\\/g, "/")}/Main.dbme`);
  const settings = await app.ipc<Settings>("settings_get");
  expect(settings.mode).toBe("organizer");
  expect(settings.last_vault).toBe(info.path);
  const status = await app.ipc<{ enabled: boolean }>("mirror_status");
  expect(status.enabled).toBe(false);
});

test("Set up again runs the steps with the vault open and keeps it", async ({
  app,
  page,
  seed,
}) => {
  await seed({ kind: "tree", objects: [{ title: "Kept" }] });
  await app.open();
  const before = await app.ipc<{ path: string }>("vault_info");
  await runCommand(app, "Set up again");
  await expect(page.getByRole("heading", { name: "Welcome to DataBase.Me" })).toBeVisible();
  await next(app);
  await expect(page.getByText("Your vault is at")).toBeVisible();
  await expect(page.getByRole("radio", { name: /In a folder I choose/ })).toHaveCount(0);
  for (let step = 0; step < 4; step++) await next(app);
  await app.press(page.getByRole("button", { name: "Finish" }));
  await expect(await app.treeItem("Kept")).toBeVisible();
  const after = await app.ipc<{ path: string }>("vault_info");
  expect(after.path).toBe(before.path);
});
