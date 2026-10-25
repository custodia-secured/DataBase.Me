#!/usr/bin/env node
// SPDX-License-Identifier: AGPL-3.0-or-later
// Checks the licence of every installed JS dependency against scripts/license-policy.json
// (ADR-0009). Runtime dependencies, the ones that end up inside the app, must use an allowed
// licence. Build and test tooling may also use the dev-only licences listed in the policy.
// Exits 1 and lists every offending package when something is not allowed.
//
// Usage: node scripts/check-js-licenses.mjs   (run after `pnpm install`)
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { expressionAllowed } from "./spdx.mjs";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const policy = JSON.parse(readFileSync(join(root, "scripts", "license-policy.json"), "utf8"));
const allowed = new Set(policy.allowed);
const devAllowed = new Set([...allowed, ...Object.keys(policy.allowedForDevOnly)]);
const exceptions = policy.packageExceptions;

/** Runs pnpm: the one that launched this script when there is one (corepack), else from PATH. */
function pnpm(args) {
  const options = { cwd: root, encoding: "utf8", maxBuffer: 64 * 1024 * 1024 };
  const launcher = process.env.npm_execpath;
  if (launcher && /\.[cm]?js$/.test(launcher)) {
    return execFileSync(process.execPath, [launcher, ...args], options);
  }
  if (launcher && /pnpm[^/\\]*$/i.test(launcher)) return execFileSync(launcher, args, options);
  return execFileSync("pnpm", args, { ...options, shell: process.platform === "win32" });
}

/** @returns {Map<string, string>} "name@version" -> licence expression */
function listLicenses(scope) {
  const out = pnpm(["-r", "licenses", "list", "--json", scope]);
  const byLicense = JSON.parse(out);
  const packages = new Map();
  for (const [license, entries] of Object.entries(byLicense)) {
    for (const entry of entries) {
      for (const version of entry.versions) {
        packages.set(`${entry.name}@${version}`, entry.license ?? license);
      }
    }
  }
  return packages;
}

function check(packages, allowList, label) {
  const failures = [];
  for (const [pkg, license] of packages) {
    const name = pkg.slice(0, pkg.lastIndexOf("@"));
    if (Object.hasOwn(exceptions, name)) continue;
    if (!expressionAllowed(license, (id) => allowList.has(id))) failures.push(`${pkg}: ${license}`);
  }
  console.log(`${label}: ${packages.size} packages checked, ${failures.length} not allowed`);
  for (const failure of failures) console.log(`  ✗ ${failure}`);
  return failures.length;
}

const prod = listLicenses("--prod");
const dev = listLicenses("--dev");
for (const pkg of prod.keys()) dev.delete(pkg);

const failed = check(prod, allowed, "runtime dependencies") + check(dev, devAllowed, "dev tooling");
if (failed > 0) {
  console.log("\nSee docs/adr/0009-dependency-licence-policy.md for how to handle a new licence.");
  process.exit(1);
}
