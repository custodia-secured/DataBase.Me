// SPDX-License-Identifier: AGPL-3.0-or-later
// network-probe.mjs on strace logs written here: Unix sockets and loopback pass, one address
// outside the machine fails, and both address shapes strace prints are read.
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { test } from "node:test";

const script = path.join(import.meta.dirname, "network-probe.mjs");

function probe(lines) {
  const dir = mkdtempSync(path.join(tmpdir(), "dbme-probe-"));
  const log = path.join(dir, "trace.txt");
  writeFileSync(log, lines.join("\n") + "\n");
  const result = spawnSync(process.execPath, [script, log], { encoding: "utf8" });
  return { status: result.status, out: result.stdout + result.stderr };
}

const LOCAL = [
  "1234 socket(AF_UNIX, SOCK_STREAM|SOCK_CLOEXEC, 0) = 5",
  '1234 connect(5, {sa_family=AF_UNIX, sun_path="/run/user/1000/bus"}, 20) = 0',
  '1235 connect(6, {sa_family=AF_INET, sin_port=htons(1420), sin_addr=inet_addr("127.0.0.1")}, 16) = 0',
  '1235 connect(7, {sa_family=AF_INET6, sin6_port=htons(80), sin6_flowinfo=htonl(0), inet_pton(AF_INET6, "::1", &sin6_addr), sin6_scope_id=0}, 28) = 0',
  '1236 sendto(8, "x", 1, 0, {sa_family=AF_INET, sin_port=htons(53), sin_addr=inet_addr("127.0.0.53")}, 16) = 1',
];

test("the machine's own sockets pass", () => {
  const { status, out } = probe(LOCAL);
  assert.equal(status, 0, out);
  assert.match(out, /3 internet socket call\(s\) in the trace, 0 outside the machine/);
  assert.match(out, /no network request/);
});

test("one connect to an address outside the machine fails, and is named", () => {
  const { status, out } = probe([
    ...LOCAL,
    '1237 connect(9, {sa_family=AF_INET, sin_port=htons(443), sin_addr=inet_addr("142.250.74.110")}, 16) = 0',
  ]);
  assert.equal(status, 1, out);
  assert.match(out, /1 outside the machine/);
  assert.match(out, /OUTSIDE: connect AF_INET 142\.250\.74\.110:443/);
});

test("an IPv6 address outside the machine fails too", () => {
  const { status, out } = probe([
    '1 sendmsg(3, {msg_name={sa_family=AF_INET6, sin6_port=htons(443), sin6_flowinfo=htonl(0), inet_pton(AF_INET6, "2606:4700::1111", &sin6_addr), sin6_scope_id=0}, msg_namelen=28}, 0) = 10',
  ]);
  assert.equal(status, 1, out);
  assert.match(out, /OUTSIDE: sendmsg AF_INET6 2606:4700::1111:443/);
});

test("an empty trace passes", () => {
  const { status, out } = probe([]);
  assert.equal(status, 0, out);
});
