// SPDX-License-Identifier: AGPL-3.0-or-later
import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { expressionAllowed } from "./spdx.mjs";

const allow = (...ids) => {
  const set = new Set(ids);
  return (id) => set.has(id);
};

describe("expressionAllowed", () => {
  it("accepts a single allowed licence and rejects others", () => {
    assert.equal(expressionAllowed("MIT", allow("MIT")), true);
    assert.equal(expressionAllowed("GPL-3.0-only", allow("MIT")), false);
  });

  it("needs only one side of OR", () => {
    assert.equal(expressionAllowed("Apache-2.0 OR MIT", allow("MIT")), true);
    assert.equal(expressionAllowed("(GPL-2.0-only OR MIT)", allow("MIT")), true);
  });

  it("needs both sides of AND", () => {
    assert.equal(expressionAllowed("MIT AND ISC", allow("MIT", "ISC")), true);
    assert.equal(expressionAllowed("MIT AND CC-BY-4.0", allow("MIT")), false);
  });

  it("binds AND tighter than OR and honours parentheses", () => {
    assert.equal(expressionAllowed("GPL-3.0-only OR MIT AND ISC", allow("MIT", "ISC")), true);
    assert.equal(expressionAllowed("(GPL-3.0-only OR MIT) AND ISC", allow("MIT")), false);
  });

  it("treats WITH exceptions as the base licence", () => {
    assert.equal(expressionAllowed("Apache-2.0 WITH LLVM-exception", allow("Apache-2.0")), true);
  });

  it("rejects anything that is not a licence expression", () => {
    assert.equal(expressionAllowed("SEE LICENSE IN LICENSE.md", allow("MIT")), false);
    assert.equal(expressionAllowed("", allow("MIT")), false);
    assert.equal(expressionAllowed("MIT OR", allow("MIT")), false);
    assert.equal(expressionAllowed("(MIT", allow("MIT")), false);
    assert.equal(expressionAllowed(undefined, allow("MIT")), false);
  });
});
