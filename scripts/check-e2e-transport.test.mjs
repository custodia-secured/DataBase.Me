// SPDX-License-Identifier: AGPL-3.0-or-later
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { describe, it } from "node:test";
import { MARKER, filesWithTransport } from "./check-e2e-transport.mjs";

describe("filesWithTransport", () => {
  it("names the scripts that carry the marker", () => {
    const files = [
      { name: "index.js", text: "createRoot(a).render(b)" },
      { name: "e2e.js", text: `throw new Error(\`${MARKER}<name> in its URL\`)` },
    ];
    assert.deepEqual(filesWithTransport(files), ["e2e.js"]);
  });

  it("finds nothing in a build without the transport", () => {
    assert.deepEqual(filesWithTransport([{ name: "index.js", text: "fetch(`/x`)" }]), []);
  });

  it("uses a marker the transport's source still contains", () => {
    const source = readFileSync(new URL("../packages/client/src/e2e.ts", import.meta.url), "utf8");
    assert.ok(source.includes(MARKER));
  });
});
