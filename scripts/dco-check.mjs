#!/usr/bin/env node
// SPDX-License-Identifier: AGPL-3.0-or-later
// Developer Certificate of Origin check (ADR-0007). Every non-merge commit in the range must carry
// a "Signed-off-by: Name <email>" line whose email matches the commit author's email.
//
// Usage:
//   node scripts/dco-check.mjs <base>..<head>   check the commits in a range (pull requests, pushes)
//   node scripts/dco-check.mjs --all [<rev>]    check every commit reachable from <rev> (default HEAD)
// Exit status: 0 all signed, 1 at least one commit is not signed off, 2 usage or git error.
import { execFileSync } from "node:child_process";
import { pathToFileURL } from "node:url";

const FIELD = "\x1f";
const RECORD = "\x1e";

/**
 * Returns the sign-offs in a commit message.
 * @param {string} message
 * @returns {{ name: string, email: string }[]}
 */
export function findSignoffs(message) {
  const pattern = /^Signed-off-by:[ \t]*(.*?)[ \t]*<([^<>\s]+@[^<>\s]+)>[ \t]*$/gim;
  return [...message.matchAll(pattern)].map((m) => ({ name: m[1] ?? "", email: m[2] ?? "" }));
}

/**
 * Decides whether one commit satisfies the DCO.
 * @param {{ authorEmail: string, message: string }} commit
 * @returns {{ ok: true } | { ok: false, reason: string }}
 */
export function checkCommit(commit) {
  const signoffs = findSignoffs(commit.message);
  if (signoffs.length === 0) return { ok: false, reason: "no Signed-off-by line" };
  const author = commit.authorEmail.trim().toLowerCase();
  if (!signoffs.some((s) => s.email.toLowerCase() === author)) {
    const found = signoffs.map((s) => s.email).join(", ");
    return {
      ok: false,
      reason: `Signed-off-by email (${found}) does not match the author email (${commit.authorEmail})`,
    };
  }
  return { ok: true };
}

/** @returns {{ sha: string, authorName: string, authorEmail: string, subject: string, message: string }[]} */
function listCommits(revArgs) {
  const format = ["%H", "%an", "%ae", "%s", "%B"].join("%x1f") + "%x1e";
  const out = execFileSync("git", ["log", "--no-merges", `--format=${format}`, ...revArgs], {
    encoding: "utf8",
    maxBuffer: 256 * 1024 * 1024,
  });
  return out
    .split(RECORD)
    .map((record) => record.replace(/^\n/, ""))
    .filter((record) => record.length > 0)
    .map((record) => {
      const [sha = "", authorName = "", authorEmail = "", subject = "", message = ""] =
        record.split(FIELD);
      return { sha, authorName, authorEmail, subject, message };
    });
}

function main(argv) {
  let revArgs;
  if (argv[0] === "--all") {
    revArgs = [argv[1] ?? "HEAD"];
  } else if (argv.length === 1 && argv[0]?.includes("..")) {
    revArgs = [argv[0]];
  } else {
    console.error("usage: dco-check.mjs <base>..<head> | --all [<rev>]");
    return 2;
  }

  let commits;
  try {
    commits = listCommits(revArgs);
  } catch (error) {
    console.error(`git log failed: ${error instanceof Error ? error.message : String(error)}`);
    return 2;
  }

  const failures = [];
  for (const commit of commits) {
    const result = checkCommit(commit);
    if (!result.ok) failures.push({ commit, reason: result.reason });
  }

  console.log(`DCO: ${commits.length} commit(s) checked, ${failures.length} not signed off.`);
  if (failures.length === 0) return 0;

  for (const { commit, reason } of failures) {
    console.log(`  ✗ ${commit.sha.slice(0, 12)} ${commit.subject}\n      ${reason}`);
  }
  console.log(
    [
      "",
      "Every commit needs a sign-off certifying the Developer Certificate of Origin (CONTRIBUTING.md).",
      "Fix the last commit:    git commit --amend --signoff --no-edit",
      "Fix a whole branch:     git rebase --signoff <base-branch>",
      "Then force-push the branch.",
    ].join("\n"),
  );
  return 1;
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) {
  process.exit(main(process.argv.slice(2)));
}
