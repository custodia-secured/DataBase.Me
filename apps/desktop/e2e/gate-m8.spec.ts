// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * M8's gates (docs/milestones/M8.md, Task 1): each library on the gate page, under the app's real
 * CSP. The fixtures fail a test on any CSP violation, uncaught error or request that leaves the
 * machine; these tests add that the library did its work, and print the figures the gate records.
 */
import { expect, test } from "./fixtures";

interface Gate {
  done: boolean;
  failed?: string;
  [key: string]: unknown;
}

declare global {
  interface Window {
    __gate?: Record<string, unknown>;
  }
}

async function run(page: import("@playwright/test").Page, query: string, timeout = 120_000) {
  await page.goto(`/gate-m8.html?${query}`);
  await page.waitForFunction(() => window.__gate?.done === true, undefined, { timeout });
  const gate = (await page.evaluate(() => window.__gate)) as Gate;
  console.log(`gate ${query}: ${JSON.stringify(gate)}`);
  expect(gate.failed).toBeUndefined();
  return gate;
}

test("1d: Excalidraw", async ({ page }) => {
  const gate = await run(page, "gate=canvas");
  expect(gate.elements).toBe(2);
  expect(gate.canvases).toBeGreaterThan(0);
});
