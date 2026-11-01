// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * The mind map's budget (handoff brief §8: 2,000 nodes interactive; figures in ADR-0019). Pan, zoom
 * and a node drag at 60 fps by the table's measure, an edit on screen within 100 ms, and every node
 * drawn within 1 s of the view switch. Enforced on Edge; WebKit's numbers are recorded only.
 *
 * Input is sent from inside the page, one step per animation frame, so the frame times measure the
 * map rather than how fast the test can send events.
 */
import type { Locator, Page } from "@playwright/test";
import { FRAME_MS, percentile, record, round } from "./budgets";
import { mindMapTree } from "./data";
import { expect, test } from "./fixtures";

const NODES = 2_011;
const SECONDS = 3;
const OPEN_BUDGET_MS = 1_000;
const EDIT_BUDGET_MS = 100;

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
}

function stats(intervals: number[]): FrameStats {
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
  };
}

/**
 * Holds a mouse button down on `target` and moves in a circle, one step per frame. The press lands
 * at `from`, or at the middle of `target`.
 */
async function circleFor(
  target: Locator,
  radius: number,
  from?: { x: number; y: number },
): Promise<FrameStats> {
  const intervals = await target.evaluate(
    (element, { radius, seconds, from }) =>
      new Promise<number[]>((resolve) => {
        const box = element.getBoundingClientRect();
        const cx = from?.x ?? box.left + box.width / 2;
        const cy = from?.y ?? box.top + box.height / 2;
        const at = (x: number, y: number): MouseEventInit => ({
          bubbles: true,
          cancelable: true,
          view: window,
          clientX: x,
          clientY: y,
          button: 0,
          buttons: 1,
        });
        element.dispatchEvent(new MouseEvent("mousedown", at(cx, cy)));
        const intervals: number[] = [];
        let start = -1;
        let last = 0;
        const step = (now: number) => {
          if (start < 0) start = now;
          else intervals.push(now - last);
          last = now;
          const turn = ((now - start) / 1000) * 2 * Math.PI;
          const move = at(cx + radius * Math.sin(turn), cy + radius * (1 - Math.cos(turn)));
          window.dispatchEvent(new MouseEvent("mousemove", move));
          if (now - start < seconds * 1000) {
            requestAnimationFrame(step);
          } else {
            window.dispatchEvent(new MouseEvent("mouseup", at(cx, cy)));
            resolve(intervals);
          }
        };
        requestAnimationFrame(step);
      }),
    { radius, seconds: SECONDS, from },
  );
  return stats(intervals);
}

/** Wheels the map in and out, one step per frame. */
async function zoomFor(pane: Locator): Promise<FrameStats> {
  const intervals = await pane.evaluate(
    (element, seconds) =>
      new Promise<number[]>((resolve) => {
        const box = element.getBoundingClientRect();
        const intervals: number[] = [];
        let start = -1;
        let last = 0;
        const step = (now: number) => {
          if (start < 0) start = now;
          else intervals.push(now - last);
          last = now;
          const elapsed = (now - start) / 1000;
          element.dispatchEvent(
            new WheelEvent("wheel", {
              bubbles: true,
              cancelable: true,
              view: window,
              clientX: box.left + box.width / 2,
              clientY: box.top + box.height / 2,
              deltaY: Math.floor(elapsed) % 2 === 0 ? -40 : 40,
            }),
          );
          if (elapsed < seconds) requestAnimationFrame(step);
          else resolve(intervals);
        };
        requestAnimationFrame(step);
      }),
    SECONDS,
  );
  return stats(intervals);
}

interface EditTiming {
  __edit?: Promise<number>;
}

/** From the key that reorders the active node to the first frame that draws it in its new place. */
async function editToFrame(page: Page, tree: Locator, id: string, key: string): Promise<number> {
  // Installed and finished before the key is pressed, or the key could get there first.
  await tree.evaluate(
    (element, { id }) => {
      const node = element.querySelector(`.react-flow__node[data-id="${id}"]`);
      if (!(node instanceof HTMLElement)) throw new Error(`node ${id} is not drawn`);
      const before = node.style.transform;
      (window as unknown as EditTiming).__edit = new Promise<number>((resolve) => {
        let pressed = -1;
        element.addEventListener(
          "keydown",
          (event) => {
            pressed = event.timeStamp;
          },
          { capture: true, once: true },
        );
        const step = (now: number) => {
          // The active node stays put on screen, so its place on the map is what shows the edit.
          if (pressed >= 0 && node.style.transform !== before) resolve(now - pressed);
          else requestAnimationFrame(step);
        };
        requestAnimationFrame(step);
      });
    },
    { id },
  );
  await page.keyboard.press(key);
  return page.evaluate(() => (window as unknown as EditTiming).__edit ?? -1);
}

test("a 2,011-node map stays interactive", async ({ app, page, seed, browserName }, testInfo) => {
  test.setTimeout(300_000);
  const { ids } = await seed<{ ids: Record<string, string> }>(mindMapTree());
  await app.open();
  await expect(app.title()).toHaveValue("Mind");

  const opened = Date.now();
  await app.view("Mind map");
  // A map this big opens on its root, at a zoom it can be read at.
  await expect(app.pane().getByRole("treeitem", { name: "Mind", exact: true })).toBeVisible({
    timeout: 30_000,
  });
  await page.evaluate(() => new Promise((done) => requestAnimationFrame(done)));
  const openMs = Date.now() - opened;

  // Fitted to the view, every box is on screen, on the overview's canvas.
  await app.pane().getByRole("button", { name: "Fit the map" }).click();
  const overview = app.pane().locator(".dbme-map__overview");
  await expect(overview).toHaveAttribute("data-drawn", String(NODES), { timeout: 30_000 });

  const pane = app.pane().locator(".react-flow__pane");
  const pan = await circleFor(pane, 200);
  const zoom = await zoomFor(pane);

  // Fit again, then down to the root's first child, which the keyboard brings to reading zoom.
  await app.pane().getByRole("button", { name: "Fit the map" }).click();
  const tree = app.pane().getByRole("tree", { name: "Mind map" });
  await tree.focus();
  await page.keyboard.press("ArrowRight");
  const moved = ids["1"] ?? "";
  await expect(app.pane().getByRole("treeitem", { name: "1", exact: true })).toHaveAttribute(
    "aria-selected",
    "true",
  );
  // The keyboard brought the map to reading zoom: here nodes come and go as they pan into view.
  const panNear = await circleFor(pane, 400);
  const zoomNear = await zoomFor(pane);

  // A drag from reading zoom: 1.2 is a leaf-side neighbour of the node the keyboard is on.
  await app.pane().getByRole("button", { name: "Fit the map" }).click();
  await tree.focus();
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("ArrowUp");
  const near = app.pane().locator(`.react-flow__node[data-id="${moved}"]`);
  const dragNear = await circleFor(near, 200);

  // A drag far away, on the canvas: zoom out until the map is boxes, and press the next box down.
  for (let out = 0; out < 8; out++) {
    await app.pane().getByRole("button", { name: "Zoom out" }).click();
  }
  await expect(overview).toBeVisible();
  const box = await near.boundingBox();
  if (!box) throw new Error("the active node is not drawn");
  const dragFar = await circleFor(pane, 200, {
    x: box.x + box.width / 2,
    y: box.y + box.height * 1.75,
  });

  await tree.focus();
  await page.keyboard.press("ArrowLeft");
  await page.keyboard.press("ArrowRight");
  const edits: number[] = [];
  for (const key of [
    "Alt+ArrowDown",
    "Alt+ArrowUp",
    "Alt+ArrowDown",
    "Alt+ArrowUp",
    "Alt+ArrowDown",
  ]) {
    edits.push(await editToFrame(page, tree, moved, key));
  }
  const sortedEdits = [...edits].sort((a, b) => a - b);
  const edit = {
    runs: edits.map(round),
    median: round(percentile(sortedEdits, 50)),
    max: round(sortedEdits.at(-1) ?? 0),
  };

  const result = { nodes: NODES, openMs, pan, panNear, zoom, zoomNear, dragNear, dragFar, edit };
  await record(testInfo, "mind-map", result);

  if (browserName === "chromium") {
    expect(openMs, "every node drawn after the view switch, ms").toBeLessThanOrEqual(
      OPEN_BUDGET_MS,
    );
    for (const [name, run] of Object.entries({ pan, panNear, zoom, zoomNear, dragNear, dragFar })) {
      expect(run.p50, `${name}: median frame time, ms`).toBeLessThanOrEqual(FRAME_MS * 1.1);
      expect(run.dropped, `${name}: share of dropped frames`).toBeLessThanOrEqual(0.02);
    }
    expect(edit.median, "an edit on screen, ms").toBeLessThanOrEqual(EDIT_BUDGET_MS);
  }
});
