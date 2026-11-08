// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * The Map view (M8, Task 10, ADR-0033), drawn under the real CSP: land from the app, clusters with counts
 * and no glyphs, setting a location by a click and by typing, a local map file read over IPC, and the list
 * for whoever cannot use a canvas or whose machine cannot draw one. The fixtures fail any test on a CSP
 * report, an uncaught error or a request leaving the machine.
 */
import { writeFileSync } from "node:fs";
import type { Locator } from "@playwright/test";
import { expect, expectAccessible, test, type App } from "./fixtures";

interface Pin {
  title: string;
  at?: [number, number];
}

function plan(pins: readonly Pin[]): unknown {
  return {
    kind: "tree",
    types: [{ name: "Place", fields: [{ key: "note", label: "Note", kind: "text" }] }],
    objects: [
      {
        title: "Trip",
        children: pins.map((pin) => ({
          title: pin.title,
          types: ["Place"],
          props: { note: pin.title },
        })),
      },
    ],
  };
}

const host = (app: App): Locator => app.pane().locator(".dbme-geo__host");
const count = (app: App): Locator => app.pane().locator(".dbme-geo__count");

/** Waits for the map to settle with what the test expects drawn. */
async function drawn(app: App, what: { points?: number; clusters?: number }): Promise<void> {
  await expect(host(app)).toHaveAttribute("data-idle", "true");
  if (what.points !== undefined) {
    await expect(host(app)).toHaveAttribute("data-points", String(what.points));
  }
  if (what.clusters !== undefined) {
    await expect(host(app)).toHaveAttribute("data-clusters", String(what.clusters));
  }
}

/** Gives items their locations through the store, which is what the UI's own actions end up calling. */
async function place(app: App, pins: readonly Pin[]): Promise<void> {
  const rows = await app.ipc<{ object: { id: string; title: string } }[]>("objects_query", {
    query: { where: [], limit: 1000 },
  });
  for (const pin of pins) {
    const row = rows.find((candidate) => candidate.object.title === pin.title);
    if (!row || !pin.at) continue;
    await app.ipc("object_set_geo", { id: row.object.id, lat: pin.at[0], lon: pin.at[1] });
  }
}

const geoOf = async (app: App, title: string): Promise<[number | null, number | null]> => {
  const rows = await app.ipc<{ object: { title: string; geo_lat: number; geo_lon: number } }[]>(
    "objects_query",
    { query: { where: [], limit: 1000 } },
  );
  const row = rows.find((candidate) => candidate.object.title === title);
  return [row?.object.geo_lat ?? null, row?.object.geo_lon ?? null];
};

async function open(app: App, seed: (plan: unknown) => Promise<unknown>, pins: readonly Pin[]) {
  await seed(plan(pins));
  await app.open();
  await place(app, pins);
  await app.page.reload();
  await app.openPage("Trip");
  await app.view("Map");
  await expect(host(app)).toBeVisible();
}

const SCATTERED: readonly Pin[] = [
  { title: "Reykjavik", at: [64.15, -21.94] },
  { title: "Nairobi", at: [-1.29, 36.82] },
  { title: "Auckland", at: [-36.85, 174.76] },
  { title: "Nowhere yet" },
];

test("it draws the land and the places, and says how many in words", async ({
  app,
  seed,
  page,
}) => {
  await open(app, seed, SCATTERED);
  await drawn(app, { points: 3, clusters: 0 });
  await expect(host(app)).toHaveAttribute("data-basemap", "land");
  expect(Number(await host(app).getAttribute("data-land"))).toBeGreaterThan(0);
  await expect(count(app)).toHaveText("3 places on the map. 1 item has no location.");
  await expect(host(app).locator("canvas")).toHaveAttribute(
    "aria-label",
    "Map. 3 places on the map. 1 item has no location.",
  );
  await expectAccessible(page, ".dbme-geo");
});

test("what is close together clusters, and the buttons zoom and show it all again", async ({
  app,
  seed,
  page,
}) => {
  // A count is an image the page draws when the map asks for it, and a canvas shows nothing else.
  const missing: string[] = [];
  page.on("console", (message) => {
    if (message.text().includes("could not be loaded")) missing.push(message.text());
  });
  const pins: Pin[] = Array.from({ length: 300 }, (_, at) => ({
    title: `Stop ${String(at)}`,
    at: [47 + (at % 17) / 100, 8 + (at % 23) / 100],
  }));
  pins.push({ title: "Far away", at: [-40, 170] });
  await open(app, seed, pins);
  await expect(count(app)).toHaveText("301 places on the map.");
  await drawn(app, { clusters: 1, points: 1 });
  expect(missing).toEqual([]);
  const before = Number(await host(app).getAttribute("data-zoom"));

  await app.press(app.pane().getByRole("button", { name: "Zoom in" }));
  await expect
    .poll(async () => Number(await host(app).getAttribute("data-zoom")))
    .toBeGreaterThan(before);
  await app.press(app.pane().getByRole("button", { name: "Show all places" }));
  await expect
    .poll(async () => Number(await host(app).getAttribute("data-zoom")))
    .toBeCloseTo(before, 1);
});

test("a click on a cluster goes in", async ({ app, seed }) => {
  // One tight group alone: fitted, it is at the middle of the map.
  const pins: Pin[] = Array.from({ length: 12 }, (_, at) => ({
    title: `Stop ${String(at)}`,
    at: [47 + at / 2000, 8 + at / 2000],
  }));
  await open(app, seed, pins);
  await drawn(app, { clusters: 1, points: 0 });
  const before = Number(await host(app).getAttribute("data-zoom"));
  const box = await host(app).boundingBox();
  if (!box) throw new Error("no map");
  await app.page.mouse.click(box.x + box.width / 2, box.y + box.height / 2);
  await expect
    .poll(async () => Number(await host(app).getAttribute("data-zoom")))
    .toBeGreaterThan(before);
});

test("a click on a place selects it, and it can be opened from there", async ({ app, seed }) => {
  await open(app, seed, [{ title: "Reykjavik", at: [64.15, -21.94] }]);
  await drawn(app, { points: 1 });
  await app.press(host(app).locator("canvas"));
  const chosen = app.pane().locator(".dbme-geo__selected");
  await expect(chosen).toContainText("Reykjavik");
  await expect(chosen).toContainText("64.15, -21.94");
  await app.press(chosen.getByRole("button", { name: "Open" }));
  await expect(app.currentCrumb()).toHaveText("Reykjavik");
});

test("a location can be typed, and what is not one is refused and says why", async ({
  app,
  seed,
  page,
}) => {
  await open(app, seed, [{ title: "Reykjavik" }]);
  await expect(count(app)).toHaveText("0 places on the map. 1 item has no location.");
  await app.press(app.pane().getByRole("button", { name: "Place on the map" }));
  const dialog = page.getByRole("dialog", { name: "Place on the map" });
  await expect(dialog.getByLabel("Item")).toContainText("Reykjavik (no location)");
  await expectAccessible(page, "dialog");

  await dialog.getByLabel("Latitude").fill("120");
  await dialog.getByLabel("Longitude").fill("-21.94");
  await app.press(dialog.getByRole("button", { name: "Save" }));
  await expect(dialog.getByRole("alert")).toHaveText("A latitude is a number from -90 to 90.");
  expect(await geoOf(app, "Reykjavik")).toEqual([null, null]);

  await dialog.getByLabel("Latitude").fill("64,15");
  await app.press(dialog.getByRole("button", { name: "Save" }));
  await expect(dialog).toBeHidden();
  await expect(count(app)).toHaveText("1 place on the map.");
  expect(await geoOf(app, "Reykjavik")).toEqual([64.15, -21.94]);
  await expect(app.pane().locator(".dbme-geo__selected")).toContainText("Reykjavik");
});

test("a location can be set by a click, and the click can be called off", async ({
  app,
  seed,
  page,
}) => {
  await open(app, seed, [{ title: "Reykjavik" }]);
  const pickOnMap = async () => {
    await app.press(app.pane().getByRole("button", { name: "Place on the map" }));
    await app.press(page.getByRole("button", { name: "Pick on the map" }));
    await expect(app.pane().locator(".dbme-geo__picking")).toContainText(
      "Click the map to place Reykjavik.",
    );
  };
  await pickOnMap();
  await app.press(app.pane().locator(".dbme-geo__picking").getByRole("button", { name: "Cancel" }));
  await expect(app.pane().locator(".dbme-geo__picking")).toHaveCount(0);
  expect(await geoOf(app, "Reykjavik")).toEqual([null, null]);

  await pickOnMap();
  await app.press(host(app).locator("canvas"));
  await expect(count(app)).toHaveText("1 place on the map.");
  const [lat, lon] = await geoOf(app, "Reykjavik");
  expect(lat).not.toBeNull();
  expect(lon).not.toBeNull();
  await expect(app.pane().locator(".dbme-geo__picking")).toHaveCount(0);
  await drawn(app, { points: 1 });
});

test("a location can be removed, and undone", async ({ app, seed }) => {
  await open(app, seed, [{ title: "Reykjavik", at: [64.15, -21.94] }]);
  await drawn(app, { points: 1 });
  await app.press(host(app).locator("canvas"));
  const chosen = app.pane().locator(".dbme-geo__selected");
  await app.press(chosen.getByRole("button", { name: "Remove location" }));
  await expect(count(app)).toHaveText("0 places on the map. 1 item has no location.");
  await expect(chosen).toHaveCount(0);
  await app.press(app.page.getByRole("button", { name: "Undo" }));
  await expect(count(app)).toHaveText("1 place on the map.");
  expect(await geoOf(app, "Reykjavik")).toEqual([64.15, -21.94]);
});

test("the list has every item with its coordinates, by keyboard alone", async ({
  app,
  seed,
  page,
}) => {
  await open(app, seed, SCATTERED);
  await app.pane().getByRole("checkbox", { name: "Show as list" }).check();
  const grid = app.pane().getByRole("grid", { name: "Places" });
  await expect(grid).toContainText("Reykjavik");
  await expect(grid).toContainText("64.15, -21.94");
  await expect(grid).toContainText("Nowhere yet");
  await expect(grid).toContainText("No location");
  await expectAccessible(page, ".dbme-geo");
  if (app.phone) return;
  await grid.focus();
  await page.keyboard.press("Enter");
  await expect(app.currentCrumb()).toHaveText("Reykjavik");
});

test("a machine that cannot draw a map gets the list, and it says so", async ({
  app,
  seed,
  page,
}) => {
  await page.addInitScript(() => {
    // eslint-disable-next-line @typescript-eslint/unbound-method -- it is called on its canvas below
    const original = HTMLCanvasElement.prototype.getContext;
    HTMLCanvasElement.prototype.getContext = function (
      this: HTMLCanvasElement,
      kind: string,
      ...rest: unknown[]
    ) {
      if (kind.startsWith("webgl")) return null;
      return (original as (...args: unknown[]) => RenderingContext | null).call(
        this,
        kind,
        ...rest,
      );
    } as typeof HTMLCanvasElement.prototype.getContext;
  });
  await seed(plan([{ title: "Reykjavik" }]));
  await app.open();
  await app.openPage("Trip");
  await app.view("Map");
  await expect(app.pane().getByText("This device cannot draw the map")).toBeVisible();
  await expect(app.pane().getByRole("grid", { name: "Places" })).toContainText("Reykjavik");
});

/** The smallest archive the format allows: one 1x1 PNG at zoom 0, uncompressed. */
function tinyPmtiles(): Buffer {
  const png = Buffer.from(
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==",
    "base64",
  );
  const directory = Buffer.from([1, 0, 1, png.length, 1]);
  const header = Buffer.alloc(127);
  header.write("PMTiles", 0, "latin1");
  header.writeUInt8(3, 7);
  const u64 = (at: number, value: number) => {
    header.writeBigUInt64LE(BigInt(value), at);
  };
  const dataAt = 127 + directory.length;
  u64(8, 127); // root directory
  u64(16, directory.length);
  u64(24, dataAt); // metadata
  u64(32, 0);
  u64(40, dataAt); // leaf directories
  u64(48, 0);
  u64(56, dataAt); // tile data
  u64(64, png.length);
  u64(72, 1); // addressed tiles
  u64(80, 1); // tile entries
  u64(88, 1); // tile contents
  header.writeUInt8(1, 96); // clustered
  header.writeUInt8(1, 97); // internal compression: none
  header.writeUInt8(1, 98); // tile compression: none
  header.writeUInt8(2, 99); // tile type: PNG
  header.writeUInt8(0, 100); // min zoom
  header.writeUInt8(0, 101); // max zoom
  header.writeInt32LE(-1800000000, 102);
  header.writeInt32LE(-850000000, 106);
  header.writeInt32LE(1800000000, 110);
  header.writeInt32LE(850000000, 114);
  return Buffer.concat([header, directory, png]);
}

test.describe("a map file", () => {
  test("it draws from a local file over IPC, and goes back from Settings", async ({
    app,
    seed,
    page,
  }, info) => {
    const file = info.outputPath("tiny.pmtiles");
    writeFileSync(file, tinyPmtiles());
    await open(app, seed, [{ title: "Reykjavik", at: [64.15, -21.94] }]);
    await expect(host(app)).toHaveAttribute("data-basemap", "land");

    let read = 0;
    page.on("request", (request) => {
      if (request.postData()?.includes('"cmd":"map_file_read"') === true) read += 1;
    });
    await app.press(page.getByRole("button", { name: "Settings" }));
    const settings = page.getByRole("dialog", { name: "Settings" });
    await settings.getByLabel("A PMTiles file").fill(file);
    await app.press(settings.getByRole("button", { name: "Use this file" }));
    await expect(settings).toContainText("tiny.pmtiles");
    await expectAccessible(page, "dialog");
    await page.keyboard.press("Escape");

    await expect(host(app)).toHaveAttribute("data-basemap", "file");
    await expect(host(app)).toHaveAttribute("data-tiles", "1");
    await drawn(app, { points: 1 });
    // The header and directory in one read, and its tile in another.
    expect(read).toBeGreaterThan(1);

    await app.press(page.getByRole("button", { name: "Settings" }));
    await app.press(settings.getByRole("button", { name: "Use the built-in map" }));
    await page.keyboard.press("Escape");
    await expect(host(app)).toHaveAttribute("data-basemap", "land");
  });

  test("what is no map file is refused, and says so", async ({ app, seed, page }, info) => {
    const file = info.outputPath("notes.txt");
    writeFileSync(file, "not a map");
    await open(app, seed, [{ title: "Reykjavik" }]);
    await app.press(page.getByRole("button", { name: "Settings" }));
    const settings = page.getByRole("dialog", { name: "Settings" });
    await settings.getByLabel("A PMTiles file").fill(file);
    await app.press(settings.getByRole("button", { name: "Use this file" }));
    await expect(settings.getByRole("alert")).toContainText("is not a PMTiles version 3 file");
  });

  test("a file that goes bad leaves the built-in map, and says so", async ({ app, seed }, info) => {
    const file = info.outputPath("gone.pmtiles");
    writeFileSync(file, tinyPmtiles());
    await seed(plan([{ title: "Reykjavik" }]));
    await app.open();
    await app.ipc("map_file_set", { path: file });
    writeFileSync(file, "not a map any more");
    await app.page.reload();
    await app.openPage("Trip");
    await app.view("Map");
    await expect(app.pane().getByText("The map file could not be read")).toBeVisible();
    await expect(host(app)).toHaveAttribute("data-basemap", "land");
  });
});

test("it holds in dark, in colours of its own", async ({ app, seed, page }) => {
  await open(app, seed, SCATTERED);
  await drawn(app, { points: 3 });
  await app.ipc("settings_set", { patch: { theme: "dark" } });
  await page.reload();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
  await app.view("Map");
  await drawn(app, { points: 3 });
  await expectAccessible(page, ".dbme-geo");
});

test("it fits a phone", async ({ app, seed, page }) => {
  await open(app, seed, SCATTERED);
  await drawn(app, { points: 3 });
  const width = page.viewportSize()?.width ?? 0;
  const box = await app.pane().locator(".dbme-geo").boundingBox();
  expect((box?.x ?? 0) + (box?.width ?? 0)).toBeLessThanOrEqual(width);
  const mapBox = await host(app).boundingBox();
  expect(mapBox?.height ?? 0).toBeGreaterThan(200);
});
