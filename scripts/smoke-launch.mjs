#!/usr/bin/env node
// SPDX-License-Identifier: AGPL-3.0-or-later
// Launch test (ADR-0008): starts the built desktop app in smoke-test mode and waits for it to report
// that the web UI rendered and ran its launch checks over IPC (create a vault in a temporary folder,
// create an object, undo it, receive both vault-changed events). The app then exits on its own:
// status 0 if the checks passed, 1 if not. Fails on a timeout, a crash, failed checks or a non-zero
// exit.
//
// Usage: node scripts/smoke-launch.mjs <path-to-dbme-desktop binary> [timeout-seconds, default 120]
// On Linux CI, run it under a virtual display: xvfb-run -a node scripts/smoke-launch.mjs ...
import { spawn } from "node:child_process";
import { existsSync } from "node:fs";

const [binaryArg, timeoutArg = "120"] = process.argv.slice(2);
if (!binaryArg) {
  console.error("usage: smoke-launch.mjs <binary> [timeout-seconds]");
  process.exit(2);
}
const binary =
  process.platform === "win32" && !binaryArg.endsWith(".exe") ? `${binaryArg}.exe` : binaryArg;
if (!existsSync(binary)) {
  console.error(`binary not found: ${binary}`);
  process.exit(2);
}
const timeoutMs = Number(timeoutArg) * 1000;

// DBME_SMOKE_WRAP, when set, is a command the app is launched through (M7: `strace -f -o
// trace.txt -e trace=network` for the zero-network test), split on spaces.
const wrap = (process.env.DBME_SMOKE_WRAP ?? "").split(" ").filter(Boolean);
const [command, ...prefixArgs] = wrap.length > 0 ? [...wrap, binary] : [binary];

const started = Date.now();
const child = spawn(command, prefixArgs, {
  env: { ...process.env, DBME_SMOKE_TEST: "1" },
  stdio: ["ignore", "pipe", "pipe"],
});
let stdout = "";
child.stdout.on("data", (chunk) => (stdout += String(chunk)));
child.stderr.on("data", (chunk) => process.stderr.write(chunk));

const timer = setTimeout(() => {
  console.error(`✗ no ready signal after ${timeoutArg}s; killing the app`);
  child.kill();
  process.exitCode = 1;
}, timeoutMs);

child.on("error", (error) => {
  clearTimeout(timer);
  console.error(`✗ could not start ${binary}: ${error.message}`);
  process.exitCode = 1;
});

child.on("exit", (code, signal) => {
  clearTimeout(timer);
  if (process.exitCode === 1) return;
  const ready = /DBME_SMOKE_OK startup_ms=(\d+)/.exec(stdout);
  if (code === 0 && ready) {
    console.log(`✓ app launched, rendered and exited cleanly`);
    console.log(
      `  startup to first render: ${ready[1]} ms (wall clock incl. exit: ${Date.now() - started} ms)`,
    );
    for (const note of stdout.matchAll(/DBME_SMOKE_NOTE (.*)/g)) console.log(`  note: ${note[1]}`);
    return;
  }
  const failed = /DBME_SMOKE_FAILED (.*)/.exec(stdout);
  if (failed) console.error(`✗ the UI's launch checks failed: ${failed[1]}`);
  console.error(`✗ app exited with code ${code ?? "none"}${signal ? ` (signal ${signal})` : ""}`);
  console.error(stdout);
  process.exitCode = 1;
});
