// SPDX-License-Identifier: AGPL-3.0-or-later
/** The diagnostic report carries versions, counts and kinds, and nothing of the person's. */
import { describe, expect, it } from "vitest";
import { renderReport, type ReportInput } from "./report";

const input: ReportInput = {
  about: { version: "0.1.0", formatVersion: 0 },
  os: "windows",
  arch: "x86_64",
  webview: "WebView2 1.0.2903.70",
  counts: { objects: 1234, blobs: 56, schema_version: 4 },
  settings: { mode: "simple", theme: "dark", update_checks: false },
  filesFolder: true,
  errors: [
    { at: Date.UTC(2026, 10, 3, 11, 0, 0), kind: "not_found" },
    { at: Date.UTC(2026, 10, 3, 10, 59, 0), kind: "rule" },
  ],
  now: new Date(Date.UTC(2026, 10, 3, 12, 0, 0)),
};

describe("renderReport", () => {
  it("names the versions, the machine, the settings, the counts and the failures", () => {
    const text = renderReport(input);
    expect(text.split("\n")).toEqual([
      "DataBase.Me diagnostic report",
      "made: 2026-11-03T12:00:00.000Z",
      "app: 0.1.0, vault format 0",
      "system: windows x86_64",
      "webview: WebView2 1.0.2903.70",
      "settings: mode simple, theme dark, update checks off",
      "vault: 1234 objects, 56 blobs, schema 4",
      "files folder: on",
      "recent errors (newest first): 2026-11-03T11:00:00.000Z not_found; 2026-11-03T10:59:00.000Z rule",
      "",
    ]);
  });

  it("says when there is no vault, no settings and no failure", () => {
    const text = renderReport({
      ...input,
      about: null,
      counts: null,
      settings: null,
      filesFolder: null,
      errors: [],
    });
    expect(text).toContain("app: version unknown");
    expect(text).toContain("vault: none open");
    expect(text).toContain("files folder: no vault");
    expect(text).toContain("settings: not read");
    expect(text).toContain("recent errors: none");
  });

  it("carries no path and no title, whatever the input holds beside the fields it reads", () => {
    const leaky = {
      ...input,
      settings: {
        ...input.settings,
        last_vault: "C:\\Users\\someone\\Secret plans.dbme",
        onboarded: true,
        version: 1,
      },
    } as unknown as ReportInput;
    const text = renderReport(leaky);
    expect(text).not.toContain("Secret");
    expect(text).not.toContain("someone");
    expect(text).not.toMatch(/[A-Z]:\\/);
  });
});
