// SPDX-License-Identifier: AGPL-3.0-or-later
import type { Source } from "@databaseme/client";
import { describe, expect, it } from "vitest";
import { hasColumn, parseChoices, targetText } from "./columns";

const children: Source = { kind: "children", parent: "01J0000000000000000000000A" };
const ofType: Source = { kind: "type", type_id: "01J0000000000000000000000B" };

describe("parseChoices", () => {
  it("splits on commas and drops blanks", () => {
    expect(parseChoices(" To do, Doing ,, Done ,")).toEqual(["To do", "Doing", "Done"]);
    expect(parseChoices(" , ")).toEqual([]);
  });
});

describe("hasColumn", () => {
  const fields = [{ label: "Serial" }];

  it("ignores case and spaces, and knows every Table has a Title", () => {
    expect(hasColumn(fields, " serial ")).toBe(true);
    expect(hasColumn(fields, "TITLE")).toBe(true);
    expect(hasColumn(fields, "Owner")).toBe(false);
  });
});

describe("targetText", () => {
  it("names the type a Table of a type adds to", () => {
    expect(targetText(ofType, { type_id: "x", type_name: "Task" }, 0)).toBe(
      "Every page of the type Task gets this column.",
    );
  });

  it("says when the pages here get a new type", () => {
    expect(targetText(children, { type_id: null, type_name: "Devices" }, 2)).toBe(
      "The pages here get a new type, Devices, to hold this column.",
    );
  });

  it("says when the pages here already share the type", () => {
    expect(targetText(children, { type_id: "x", type_name: "Devices" }, 2)).toBe(
      "This column goes on the type Devices, so every page of that type gets it.",
    );
  });

  it("says when an empty Table gets a first row", () => {
    expect(targetText(children, { type_id: null, type_name: "Devices" }, 0)).toContain(
      "It also adds a first row.",
    );
  });
});
