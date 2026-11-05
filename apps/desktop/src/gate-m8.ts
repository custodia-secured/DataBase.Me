// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * The page M8's gates run on (docs/milestones/M8.md, Task 1). Only the e2e build contains it, and
 * the e2e server serves it with the app's real CSP. It goes when the views that replace each gate
 * land.
 */
import { runGate } from "@databaseme/views/gates/m8";
import "./gate-m8.css";

declare global {
  interface Window {
    __gate?: Record<string, unknown>;
  }
}

const root = document.getElementById("gate-root");
if (!root) throw new Error("no gate root");
runGate(root).then(
  (result) => {
    window.__gate = { done: true, ...result };
  },
  (error: unknown) => {
    window.__gate = {
      done: true,
      failed: String(error),
      stack: error instanceof Error ? error.stack : undefined,
    };
  },
);
