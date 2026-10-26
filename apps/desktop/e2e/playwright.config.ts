// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * The UI's end-to-end tests (ADR-0018). Microsoft Edge stands in for WebView2, which is built on
 * it, and Playwright's WebKit for WKWebView and WebKitGTK; each runs at a desktop size and at a
 * 360 px phone size with touch. The e2e server runs the real command functions on real vaults
 * and sends the app's CSP.
 *
 * The budgets (`*-budget.spec.ts`) run last, one test at a time, so nothing else competes with
 * what they measure.
 *
 * Run `pnpm --filter @databaseme/desktop e2e`: it builds the e2e UI, then starts the server.
 */
import { defineConfig, devices } from "@playwright/test";
import path from "node:path";

const ROOT = path.resolve(import.meta.dirname, "../../..");
const PORT = 4310;
const CI = Boolean(process.env.CI);

const edge = { browserName: "chromium" as const, channel: "msedge" };
const desktop = { viewport: { width: 1280, height: 800 } };
const BUDGETS = /-budget\.spec\.ts$/;
const FUNCTIONAL = ["edge-desktop", "webkit-desktop", "edge-phone", "webkit-phone"];
const phone = {
  viewport: { width: 360, height: 740 },
  deviceScaleFactor: 2,
  isMobile: true,
  hasTouch: true,
};

export default defineConfig({
  testDir: ".",
  outputDir: path.join(ROOT, "target/e2e-results"),
  fullyParallel: true,
  forbidOnly: CI,
  // A flaky test is a bug to fix, not to retry.
  retries: 0,
  // A whole run takes about two minutes; a browser that hangs on exit must not hold it for hours.
  globalTimeout: 30 * 60_000,
  reporter: CI
    ? [["list"], ["html", { open: "never", outputFolder: path.join(ROOT, "target/e2e-report") }]]
    : "list",
  use: {
    baseURL: `http://127.0.0.1:${String(PORT)}`,
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
  },
  testIgnore: BUDGETS,
  projects: [
    { name: "edge-desktop", use: { ...devices["Desktop Edge"], ...edge, ...desktop } },
    { name: "webkit-desktop", use: { ...devices["Desktop Safari"], ...desktop } },
    { name: "edge-phone", use: { ...edge, ...phone } },
    { name: "webkit-phone", use: { browserName: "webkit", ...phone } },
    {
      name: "edge-budget",
      testMatch: BUDGETS,
      testIgnore: [],
      workers: 1,
      dependencies: FUNCTIONAL,
      use: { ...devices["Desktop Edge"], ...edge, ...desktop },
    },
    {
      name: "webkit-budget",
      testMatch: BUDGETS,
      testIgnore: [],
      workers: 1,
      dependencies: [...FUNCTIONAL, "edge-budget"],
      use: { ...devices["Desktop Safari"], ...desktop },
    },
  ],
  webServer: {
    command: `cargo run --quiet -p dbme-e2e-server -- --dist apps/desktop/dist-e2e --data target/e2e-data --port ${String(PORT)}`,
    cwd: ROOT,
    url: `http://127.0.0.1:${String(PORT)}/health`,
    reuseExistingServer: !CI,
    // The first run compiles the server.
    timeout: 600_000,
    stdout: "ignore",
    stderr: "pipe",
  },
});
