// SPDX-License-Identifier: AGPL-3.0-or-later
import react from "@vitejs/plugin-react";
import { configDefaults, defineConfig } from "vitest/config";
import pkg from "./package.json" with { type: "json" };

export default defineConfig(({ mode }) => ({
  plugins: [react()],
  // `--mode e2e` builds the UI for the end-to-end tests (ADR-0018) into its own folder, so it can
  // never be mistaken for the app's bundle.
  build: {
    outDir: mode === "e2e" ? "dist-e2e" : "dist",
  },
  clearScreen: false,
  define: {
    __APP_VERSION__: JSON.stringify(pkg.version),
  },
  // The Tauri CLI starts this dev server (see src-tauri/tauri.conf.json) and expects it on a fixed
  // port, so a busy port must fail loudly instead of silently moving.
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**"] },
  },
  envPrefix: ["VITE_", "TAURI_ENV_"],
  test: {
    environment: "jsdom",
    // The Playwright specs in e2e/ run under Playwright, not vitest.
    exclude: [...configDefaults.exclude, "e2e/**"],
  },
}));
