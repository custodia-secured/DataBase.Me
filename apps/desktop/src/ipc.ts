// SPDX-License-Identifier: AGPL-3.0-or-later
import { invoke, isTauri } from "@tauri-apps/api/core";

/**
 * Tells the Rust shell that the UI has rendered. The shell logs the startup time and, in smoke-test
 * mode, exits with status 0 so CI can prove the app launched (see ADR-0008).
 *
 * Outside the Tauri webview (unit tests, a plain browser on the Vite dev server) there is no shell
 * to tell, so this does nothing.
 */
export async function notifyReady(): Promise<void> {
  if (!isTauri()) return;
  await invoke("app_ready");
}
