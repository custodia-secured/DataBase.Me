// SPDX-License-Identifier: AGPL-3.0-or-later
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { Dialog, IconButton, Plus, StatusMessage, typeIcon, TYPE_ICON_NAMES } from "./index";
import { FallbackTypeIcon } from "./icons";

afterEach(cleanup);

describe("icons", () => {
  it("know the icon of every system type", () => {
    // The icons of the 25 system types, as crates/core/src/seed.rs names them.
    const seeded = [
      "sticky-note",
      "file",
      "square-check",
      "user",
      "building-2",
      "map-pin",
      "calendar",
      "folder-kanban",
      "layers",
      "book-open",
      "clapperboard",
      "globe",
      "file-text",
      "image",
      "audio-lines",
      "video",
      "mail",
      "message-square",
      "contact",
      "table",
      "tag",
      "library",
      "layout-template",
      "highlighter",
      "notebook-pen",
    ];
    expect([...TYPE_ICON_NAMES].sort()).toEqual([...seeded].sort());
    expect(new Set(seeded.map(typeIcon)).size).toBe(25);
  });

  it("fall back for unknown or missing names", () => {
    expect(typeIcon("no-such-icon")).toBe(FallbackTypeIcon);
    expect(typeIcon(null)).toBe(FallbackTypeIcon);
  });
});

describe("IconButton", () => {
  it("is named by its label and never submits", () => {
    const onClick = vi.fn();
    render(<IconButton label="New page" icon={Plus} onClick={onClick} />);
    const button = screen.getByRole("button", { name: "New page" });
    expect(button.getAttribute("type")).toBe("button");
    fireEvent.click(button);
    expect(onClick).toHaveBeenCalledOnce();
  });
});

describe("Dialog", () => {
  it("is labelled by its title and asks to close on Escape", () => {
    // jsdom lacks showModal; the browser's own behaviour is covered by the e2e tests.
    HTMLDialogElement.prototype.showModal = function showModal(this: HTMLDialogElement) {
      this.open = true;
    };
    HTMLDialogElement.prototype.close = function close(this: HTMLDialogElement) {
      this.open = false;
    };
    const onClose = vi.fn();
    render(
      <Dialog open onClose={onClose} title="Move to">
        <p>Body</p>
      </Dialog>,
    );
    const dialog = screen.getByRole("dialog", { name: "Move to" });
    fireEvent(dialog, new Event("cancel", { cancelable: true }));
    expect(onClose).toHaveBeenCalledOnce();
  });
});

describe("StatusMessage", () => {
  it("uses an alert for errors and a status otherwise", () => {
    render(<StatusMessage tone="error">Could not save</StatusMessage>);
    expect(screen.getByRole("alert").textContent).toBe("Could not save");
    cleanup();
    render(<StatusMessage>Saved</StatusMessage>);
    expect(screen.getByRole("status").textContent).toBe("Saved");
  });
});
