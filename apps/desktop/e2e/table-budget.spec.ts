// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * The table budget of the handoff brief, §6: a 100k-row table scrolls at 60 fps. Enforced on
 * Edge, the stand-in for WebView2; WebKit's numbers are recorded only (see budgets.ts).
 */
import type { Locator } from "@playwright/test";
import { FRAME_MS, percentile, record, round } from "./budgets";
import { bulk } from "./data";
import { expect, test } from "./fixtures";

const ROWS = 100_000;

test.skip(({ isMobile }) => isMobile, "the budgets are set for the desktop reference machine");

interface FrameStats {
  frames: number;
  fps: number;
  p50: number;
  p95: number;
  p99: number;
  max: number;
  /** Share of frames that took longer than one and a half frames. */
  dropped: number;
  /** Share of frames that showed a row still being read. */
  blank: number;
}

function stats(intervals: number[], blankFrames: number): FrameStats {
  const sorted = [...intervals].sort((a, b) => a - b);
  const total = intervals.reduce((sum, value) => sum + value, 0);
  return {
    frames: intervals.length,
    fps: round((1000 * intervals.length) / total),
    p50: round(percentile(sorted, 50)),
    p95: round(percentile(sorted, 95)),
    p99: round(percentile(sorted, 99)),
    max: round(sorted.at(-1) ?? 0),
    dropped: round(intervals.filter((value) => value > FRAME_MS * 1.5).length / intervals.length),
    blank: round(blankFrames / intervals.length),
  };
}

/** Scrolls `grid` by `speed` px/s for `seconds`, one step per animation frame, timing frames. */
async function scrollFor(grid: Locator, speed: number, seconds: number): Promise<FrameStats> {
  const { intervals, blankFrames } = await grid.evaluate(
    (element, { speed, seconds }) =>
      new Promise<{ intervals: number[]; blankFrames: number }>((resolve) => {
        const intervals: number[] = [];
        let blankFrames = 0;
        let start = -1;
        let last = 0;
        const step = (now: number) => {
          if (start < 0) {
            start = now;
          } else {
            intervals.push(now - last);
            element.scrollTop += (speed * (now - last)) / 1000;
            if (element.querySelector(".dbme-grid__placeholder")) blankFrames++;
          }
          last = now;
          if (now - start < seconds * 1000) requestAnimationFrame(step);
          else resolve({ intervals, blankFrames });
        };
        requestAnimationFrame(step);
      }),
    { speed, seconds },
  );
  return stats(intervals, blankFrames);
}

test("a 100,000-row table scrolls at 60 fps", async ({ app, seed, browserName }, testInfo) => {
  test.setTimeout(900_000);
  const seeded = Date.now();
  await seed(bulk(ROWS));
  const seedSeconds = (Date.now() - seeded) / 1000;
  await app.open();
  await app.view("Table");
  await expect(app.pane().getByText(`${ROWS.toLocaleString("en-US")} rows`)).toBeVisible({
    timeout: 120_000,
  });
  const grid = app.pane().getByRole("grid", { name: "Rows" });
  await expect(grid.getByRole("gridcell", { name: "Task 000001", exact: true })).toBeVisible();

  // Jump to the middle, as a dragged scrollbar does, and time until the rows there are read.
  const jumped = Date.now();
  await grid.evaluate((element) => {
    element.scrollTop = element.scrollHeight / 2;
  });
  await expect(grid.locator(".dbme-grid__placeholder")).toHaveCount(0, { timeout: 30_000 });
  const fillMs = Date.now() - jumped;

  const reading = await scrollFor(grid, 2_000, 5);
  const flinging = await scrollFor(grid, 20_000, 3);
  const result = {
    rows: ROWS,
    seedSeconds: round(seedSeconds),
    jumpFillMs: fillMs,
    reading,
    flinging,
  };
  await record(testInfo, "table-scroll", result);

  if (browserName === "chromium") {
    for (const run of [reading, flinging]) {
      // 60 fps: the typical frame takes one frame, and at most 2% take more than one and a half.
      expect(run.p50, "median frame time, ms").toBeLessThanOrEqual(FRAME_MS * 1.1);
      expect(run.dropped, "share of dropped frames").toBeLessThanOrEqual(0.02);
    }
  }
});
