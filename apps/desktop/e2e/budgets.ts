// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * Shared by the M2 performance budgets of the handoff brief, §6: a 100k-row table at 60 fps, and
 * typing latency under 16 ms. Results go to target/budgets/ and to the test report.
 *
 * Edge stands in for WebView2 on the reference platform, so the budgets are enforced there.
 * Playwright's WebKit build for Windows is a test port, not the WKWebView the app ships on, so
 * its numbers are recorded but not enforced (ADR-0018).
 */
import type { TestInfo } from "@playwright/test";
import { mkdirSync, writeFileSync } from "node:fs";
import path from "node:path";

export const FRAME_MS = 1000 / 60;

export function percentile(sorted: number[], p: number): number {
  return sorted[Math.min(sorted.length - 1, Math.floor((sorted.length * p) / 100))] ?? 0;
}

export function round(value: number): number {
  return Math.round(value * 100) / 100;
}

/** Saves a result where the milestone report reads it. */
export async function record(testInfo: TestInfo, name: string, result: unknown): Promise<void> {
  const dir = path.resolve(import.meta.dirname, "../../../target/budgets");
  mkdirSync(dir, { recursive: true });
  const body = JSON.stringify(result, null, 2);
  writeFileSync(path.join(dir, `${testInfo.project.name}-${name}.json`), `${body}\n`);
  await testInfo.attach(name, { body, contentType: "application/json" });
  console.log(`${testInfo.project.name} ${name}: ${JSON.stringify(result)}`);
}
