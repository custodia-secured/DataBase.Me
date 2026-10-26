// SPDX-License-Identifier: AGPL-3.0-or-later
import type { Value } from "@databaseme/client";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { FieldEditor } from "./fields";

afterEach(cleanup);

const status = {
  kind: "select" as const,
  label: "Status",
  options_json: {
    options: [
      { id: "todo", label: "To do" },
      { id: "done", label: "Done" },
    ],
  },
};
const estimate = { kind: "number" as const, label: "Estimate", options_json: null };

function setup(field: typeof status | typeof estimate, value: Value | undefined) {
  const onCommit = vi.fn();
  const onDone = vi.fn();
  render(
    <FieldEditor
      field={field}
      value={value}
      onCommit={onCommit}
      onCancel={() => undefined}
      onDone={onDone}
    />,
  );
  return { onCommit, onDone };
}

describe("FieldEditor", () => {
  it("saves a select at once when an option is picked with a pointer", () => {
    const { onCommit, onDone } = setup(status, "todo");
    const select = screen.getByRole("combobox", { name: "Status" });
    fireEvent.pointerDown(select);
    fireEvent.change(select, { target: { value: "done" } });
    expect(onCommit).toHaveBeenCalledWith("done");
    expect(onDone).toHaveBeenCalledWith("Enter");
  });

  it("lets the keyboard browse a select's options and saves on Enter", () => {
    const { onCommit, onDone } = setup(status, "todo");
    const select = screen.getByRole("combobox", { name: "Status" });
    fireEvent.keyDown(select, { key: "ArrowDown" });
    fireEvent.change(select, { target: { value: "done" } });
    expect(onCommit).not.toHaveBeenCalled();
    fireEvent.keyDown(select, { key: "Enter" });
    expect(onCommit).toHaveBeenCalledWith("done");
    expect(onDone).toHaveBeenCalledWith("Enter");
  });

  it("parses a number and saves it on Tab", () => {
    const { onCommit } = setup(estimate, 3);
    const input = screen.getByRole("spinbutton", { name: "Estimate" });
    expect(document.activeElement).toBe(input);
    fireEvent.change(input, { target: { value: "5" } });
    fireEvent.keyDown(input, { key: "Tab" });
    expect(onCommit).toHaveBeenCalledWith(5);
  });

  it("cancels on Escape without saving", () => {
    const { onCommit, onDone } = setup(estimate, 3);
    const input = screen.getByRole("spinbutton", { name: "Estimate" });
    fireEvent.change(input, { target: { value: "8" } });
    fireEvent.keyDown(input, { key: "Escape" });
    expect(onCommit).not.toHaveBeenCalled();
    expect(onDone).toHaveBeenCalledWith("Escape");
  });
});
