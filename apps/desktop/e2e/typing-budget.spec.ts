// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * The typing budget of the handoff brief, §6: a key press shows in under 16 ms, in a long page.
 * Enforced on Edge only, as the flags below are Chromium's (see budgets.ts).
 */
import { percentile, record, round } from "./budgets";
import { longDocument } from "./data";
import { expect, test } from "./fixtures";

const PARAGRAPHS = 1_000;

// Without the frame-rate cap a frame starts as soon as the page is idle, so the time from a key
// press to the end of the frame that shows it is the work the key press cost. Launch options
// force a worker of their own, so Playwright takes them only at the top of a file.
test.use({ launchOptions: { args: ["--disable-frame-rate-limit", "--disable-gpu-vsync"] } });
test.skip(
  ({ browserName, isMobile }) => browserName !== "chromium" || isMobile,
  "the flags are Chromium's, and the budgets are set for the desktop reference machine",
);

test("typing in a long page takes under 16 ms a key", async ({ app, seed, page }, testInfo) => {
  test.setTimeout(300_000);
  await seed(longDocument(PARAGRAPHS));
  await app.open();
  await expect(app.title()).toHaveValue("Long read");
  await expect(app.editor().locator("p")).toHaveCount(PARAGRAPHS);
  await app.focusEditor();
  await page.keyboard.press("ControlOrMeta+End");
  await app.editor().evaluate((editor) => {
    const samples: number[] = [];
    (window as unknown as { __typing: number[] }).__typing = samples;
    editor.addEventListener(
      "keydown",
      (event) => {
        const start = event.timeStamp;
        // The task after the next frame runs once that frame, with the key in it, is done.
        requestAnimationFrame(() => {
          setTimeout(() => samples.push(performance.now() - start), 0);
        });
      },
      { capture: true },
    );
  });

  const text = "The quick brown fox jumps over the lazy dog. ".repeat(5);
  await page.keyboard.type(text, { delay: 40 });
  await expect(app.editor()).toContainText(text.trim());
  const samples = await page.evaluate(() => (window as unknown as { __typing: number[] }).__typing);
  const sorted = [...samples].sort((a, b) => a - b);
  const result = {
    paragraphs: PARAGRAPHS,
    keys: samples.length,
    p50: round(percentile(sorted, 50)),
    p95: round(percentile(sorted, 95)),
    p99: round(percentile(sorted, 99)),
    max: round(sorted.at(-1) ?? 0),
  };
  await record(testInfo, "typing", result);

  expect(result.keys, "one sample a key").toBe(text.length);
  expect(result.p95, "95th percentile key to frame, ms").toBeLessThan(16);
});
