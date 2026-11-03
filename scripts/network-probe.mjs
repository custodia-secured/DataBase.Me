#!/usr/bin/env node
// SPDX-License-Identifier: AGPL-3.0-or-later
// The zero-network check (M7, brief §2.2 and §8: "ZERO network requests on first launch with
// defaults"). Reads an strace log of the app's launch, made with
//   DBME_SMOKE_WRAP="strace -f -o <log> -e trace=network" node scripts/smoke-launch.mjs <binary>
// and lists every socket the app or the webview connected or sent to. Loopback addresses and Unix
// sockets (the display, D-Bus, the webview's own helper processes, Tauri's IPC) are the app
// talking to the machine it runs on and are allowed; any other address is a network request and
// fails the check. The whole list is printed either way, so the log is the record.
//
// Usage: node scripts/network-probe.mjs <strace-log>
import { readFileSync } from "node:fs";

const [logArg] = process.argv.slice(2);
if (!logArg) {
  console.error("usage: network-probe.mjs <strace-log>");
  process.exit(2);
}
const log = readFileSync(logArg, "utf8");

/** Every connect, sendto or sendmsg with an internet address, as strace prints it. */
const calls = [];
for (const line of log.split("\n")) {
  const match = /\b(connect|sendto|sendmsg)\(.*?sa_family=(AF_INET6?|AF_UNIX)(.*)/.exec(line);
  if (!match) continue;
  const [, call, family, rest] = match;
  if (family === "AF_UNIX") continue;
  // strace prints an IPv4 address as `sin_addr=inet_addr("1.2.3.4")` and an IPv6 one as
  // `inet_pton(AF_INET6, "::1", &sin6_addr)`.
  const address =
    /sin_addr=inet_addr\("([^"]+)"\)/.exec(rest)?.[1] ??
    /inet_pton\(AF_INET6, "([^"]+)"/.exec(rest)?.[1] ??
    "?";
  const port = /sin6?_port=htons\((\d+)\)/.exec(rest)?.[1] ?? "?";
  calls.push({ call, family, address, port, line: line.trim() });
}

const loopback = (address) =>
  address === "127.0.0.1" ||
  address.startsWith("127.") ||
  address === "::1" ||
  address === "::ffff:127.0.0.1" ||
  address === "0.0.0.0" ||
  address === "::";

const outside = calls.filter((entry) => !loopback(entry.address));
console.log(
  `${String(calls.length)} internet socket call(s) in the trace, ${String(outside.length)} outside the machine`,
);
for (const entry of calls) {
  const where = loopback(entry.address) ? "local" : "OUTSIDE";
  console.log(`  ${where}: ${entry.call} ${entry.family} ${entry.address}:${entry.port}`);
}
if (outside.length > 0) {
  console.error("✗ the app reached outside the machine on first launch with default settings");
  for (const entry of outside) console.error(`  ${entry.line}`);
  process.exit(1);
}
console.log("✓ no network request on first launch with default settings");
