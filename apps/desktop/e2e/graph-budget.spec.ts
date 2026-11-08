// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * The graph budget of the handoff brief, §6 and §8: a 100k-node graph renders on WebGL. It checks Sigma's own
 * count and that ink reached the canvas, everywhere. The brief gives the time no figure, and CI draws in
 * software (M8 gate 1c: a first painted frame of 22 to 36 s there, about 3 s on a machine with a GPU), so the
 * time is printed everywhere, and gated only on Edge away from CI.
 */
import { record } from "./budgets";
import { graphLoad } from "./data";
import { expect, test } from "./fixtures";
import "./sigma-handle";

const NODES = 100_000;
const FIRST_FRAME_BUDGET_MS = 10_000;

test.skip(({ isMobile }) => isMobile, "the budgets are set for the desktop reference machine");

test("a 100,000-node graph renders", async ({ app, page, seed, browserName }, testInfo) => {
  // Seeding 100,000 objects and about 200,000 relations comes first, then a slow first frame on CI.
  test.setTimeout(600_000);
  const started = Date.now();
  await seed(graphLoad(NODES));
  const seedMs = Date.now() - started;
  await app.open();
  await expect(app.title()).toHaveValue("Big graph");

  const opened = Date.now();
  await app.view("Graph");
  const host = app.pane().locator(".dbme-graph__host");
  await expect(host).toHaveAttribute("data-drawn", "true", { timeout: 300_000 });
  const firstFrameMs = Date.now() - opened;

  // Paused, so that what this reads is the drawing and not a layout step on top of it.
  const pause = app.pane().getByRole("button", { name: "Pause layout" });
  if ((await pause.count()) > 0) await pause.click({ timeout: 120_000 });

  const seen = await page.evaluate(() => {
    const sigma = window.__dbmeSigma;
    if (!sigma) throw new Error("no graph");
    const began = performance.now();
    sigma.refresh();
    const refreshMs = performance.now() - began;
    const gl = sigma.getWebGLContext();
    const x = Math.floor(gl.drawingBufferWidth / 2) - 128;
    const y = Math.floor(gl.drawingBufferHeight / 2) - 128;
    const pixels = new Uint8Array(4 * 256 * 256);
    gl.readPixels(x, y, 256, 256, gl.RGBA, gl.UNSIGNED_BYTE, pixels);
    return {
      nodes: sigma.getGraph().order,
      edges: sigma.getGraph().size,
      refreshMs: Math.round(refreshMs),
      ink: pixels.some((value) => value !== 0),
    };
  });

  await record(testInfo, "graph", { ...seen, seedMs, firstFrameMs });
  expect(seen.nodes).toBe(NODES);
  expect(seen.edges).toBeGreaterThan(NODES);
  expect(seen.ink, "something reached the canvas").toBe(true);
  if (browserName === "chromium" && !process.env.CI) {
    expect(firstFrameMs, "the view switch to a painted frame, ms").toBeLessThanOrEqual(
      FIRST_FRAME_BUDGET_MS,
    );
  }
});
