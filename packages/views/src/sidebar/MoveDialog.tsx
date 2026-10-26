// SPDX-License-Identifier: AGPL-3.0-or-later
/** "Move to…": puts an object under another one found by title. Works by keyboard and touch. */
import { useSearch, type Id } from "@databaseme/client";
import { Button, Dialog, StatusMessage } from "@databaseme/ui";
import { useId, useState } from "react";
import { actions } from "../actions";

export function MoveDialog({
  object,
  onClose,
}: {
  /** The object to move, or null when the dialog is closed. */
  object: { id: Id; title: string } | null;
  onClose: () => void;
}) {
  const [text, setText] = useState("");
  const [error, setError] = useState<string | null>(null);
  const inputId = useId();
  const results = useSearch(text, 20, object !== null);
  const choices = (results.data ?? []).filter((summary) => summary.object.id !== object?.id);

  const moveTo = async (parent: Id | null) => {
    if (!object) return;
    const moved = await actions.move(object.id, parent, "last");
    if (moved === undefined) {
      setError("The page was not moved.");
      return;
    }
    setText("");
    setError(null);
    onClose();
  };

  const close = () => {
    setText("");
    setError(null);
    onClose();
  };

  return (
    <Dialog
      open={object !== null}
      onClose={close}
      title={`Move “${object && object.title !== "" ? object.title : "Untitled"}” to…`}
    >
      <label htmlFor={inputId}>Find a page by title</label>
      <input
        id={inputId}
        className="dbme-input"
        type="search"
        value={text}
        autoComplete="off"
        onChange={(event) => {
          setText(event.target.value);
        }}
      />
      {error ? <StatusMessage tone="error">{error}</StatusMessage> : null}
      <ul className="dbme-move__list" aria-label="Destinations">
        <li>
          <Button variant="ghost" className="dbme-move__choice" onClick={() => void moveTo(null)}>
            Top level
          </Button>
        </li>
        {choices.map((summary) => (
          <li key={summary.object.id}>
            <Button
              variant="ghost"
              className="dbme-move__choice"
              onClick={() => void moveTo(summary.object.id)}
            >
              {summary.object.title || "Untitled"}
            </Button>
          </li>
        ))}
      </ul>
      <div className="dbme-dialog__actions">
        <Button onClick={close}>Cancel</Button>
      </div>
    </Dialog>
  );
}
