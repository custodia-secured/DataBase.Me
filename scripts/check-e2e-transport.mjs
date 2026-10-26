// SPDX-License-Identifier: AGPL-3.0-or-later
// Proves the end-to-end test transport (packages/client/src/e2e.ts, ADR-0018) is absent from a
// production build of the UI, which must only ever talk to the Rust shell.
//
//   node scripts/check-e2e-transport.mjs absent apps/desktop/dist
//   node scripts/check-e2e-transport.mjs present apps/desktop/dist-e2e
//
// The transport is found by a string a minifier keeps: its error message for a missing session.
// Checking that the e2e build has it shows the marker still identifies the transport.

import { readdirSync, readFileSync } from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";

export const MARKER = "the e2e build needs ?session=";

/**
 * @param {{ name: string, text: string }[]} files the build's scripts
 * @returns {string[]} the names of those that contain the transport
 */
export function filesWithTransport(files) {
  return files.filter((file) => file.text.includes(MARKER)).map((file) => file.name);
}

/** @param {string} dir */
function scripts(dir) {
  return readdirSync(dir, { recursive: true, encoding: "utf8" })
    .filter((name) => name.endsWith(".js"))
    .map((name) => ({ name, text: readFileSync(path.join(dir, name), "utf8") }));
}

function main() {
  const [expect, dir] = process.argv.slice(2);
  if ((expect !== "absent" && expect !== "present") || !dir) {
    console.error("usage: check-e2e-transport.mjs absent|present <build dir>");
    process.exit(2);
  }
  const files = scripts(dir);
  if (files.length === 0) {
    console.error(`no scripts in ${dir}: build the UI first`);
    process.exit(2);
  }
  const found = filesWithTransport(files);
  if (expect === "absent" && found.length > 0) {
    console.error(`the e2e transport is in the build: ${found.join(", ")}`);
    process.exit(1);
  }
  if (expect === "present" && found.length === 0) {
    console.error(`the e2e transport's marker is not in ${dir}: update MARKER`);
    process.exit(1);
  }
  console.log(`the e2e transport is ${expect} in ${dir} (${String(files.length)} scripts)`);
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) main();
