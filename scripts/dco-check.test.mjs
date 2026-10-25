// SPDX-License-Identifier: AGPL-3.0-or-later
import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { after, before, describe, it } from "node:test";
import { fileURLToPath } from "node:url";
import { checkCommit, findSignoffs } from "./dco-check.mjs";

const script = join(dirname(fileURLToPath(import.meta.url)), "dco-check.mjs");

describe("findSignoffs", () => {
  it("finds every Signed-off-by line, case-insensitively", () => {
    const message =
      "Add thing\n\nBody.\n\nsigned-off-by: Ada <ada@example.org>\nSigned-off-by: Bo <bo@example.org>\n";
    assert.deepEqual(findSignoffs(message), [
      { name: "Ada", email: "ada@example.org" },
      { name: "Bo", email: "bo@example.org" },
    ]);
  });

  it("ignores a sign-off without an email or quoted mid-line", () => {
    assert.deepEqual(findSignoffs("Signed-off-by: Ada\nsee Signed-off-by: X <x@y.z>"), []);
  });
});

describe("checkCommit", () => {
  it("passes when a sign-off matches the author email", () => {
    const result = checkCommit({
      authorEmail: "Ada@Example.org",
      message: "x\n\nSigned-off-by: Ada <ada@example.org>",
    });
    assert.deepEqual(result, { ok: true });
  });

  it("fails without a sign-off", () => {
    assert.equal(checkCommit({ authorEmail: "ada@example.org", message: "x" }).ok, false);
  });

  it("fails when the sign-off belongs to someone else", () => {
    const result = checkCommit({
      authorEmail: "ada@example.org",
      message: "x\n\nSigned-off-by: Bo <bo@example.org>",
    });
    assert.equal(result.ok, false);
  });
});

describe("dco-check.mjs against a real repository", () => {
  let repo = "";
  const git = (...args) => execFileSync("git", args, { cwd: repo, encoding: "utf8" }).trim();
  const run = (...args) =>
    spawnSync(process.execPath, [script, ...args], { cwd: repo, encoding: "utf8" });
  const commit = (file, signoff) => {
    writeFileSync(join(repo, file), file);
    git("add", file);
    git("commit", "-q", "-m", `add ${file}`, ...(signoff ? ["--signoff"] : []));
    return git("rev-parse", "HEAD");
  };

  before(() => {
    repo = mkdtempSync(join(tmpdir(), "dbme-dco-"));
    git("init", "-q", "-b", "main");
    git("config", "user.name", "Test Author");
    git("config", "user.email", "author@example.org");
    git("config", "commit.gpgsign", "false");
  });

  after(() => rmSync(repo, { recursive: true, force: true }));

  it("passes a range of signed commits and fails as soon as one is unsigned", () => {
    const base = commit("a.txt", true);
    commit("b.txt", true);
    const signedRange = run(`${base}..HEAD`);
    assert.equal(signedRange.status, 0, signedRange.stdout + signedRange.stderr);

    const unsigned = commit("c.txt", false);
    const mixedRange = run(`${base}..HEAD`);
    assert.equal(mixedRange.status, 1);
    assert.match(mixedRange.stdout, new RegExp(unsigned.slice(0, 12)));
    assert.match(mixedRange.stdout, /2 commit\(s\) checked, 1 not signed off/);
  });

  it("checks the whole history with --all", () => {
    const result = run("--all");
    assert.equal(result.status, 1);
    assert.match(result.stdout, /3 commit\(s\) checked, 1 not signed off/);
  });

  it("rejects bad usage with status 2", () => {
    assert.equal(run().status, 2);
    assert.equal(run("not-a-range").status, 2);
    assert.equal(run("nope..HEAD").status, 2);
  });
});
