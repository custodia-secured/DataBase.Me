// SPDX-License-Identifier: AGPL-3.0-or-later
/** "Relate to…": draws a relation without a drag. Works by keyboard and touch. */
import { useSearch, type Id } from "@databaseme/client";
import { Button, Dialog, StatusMessage } from "@databaseme/ui";
import { useId, useState } from "react";
import { actions } from "../actions";

export function RelateDialog({
  object,
  onClose,
}: {
  /** The object the relation starts at, or null when the dialog is closed. */
  object: { id: Id; title: string } | null;
  onClose: () => void;
}) {
  const [text, setText] = useState("");
  const [error, setError] = useState<string | null>(null);
  const inputId = useId();
  const results = useSearch(text, 20, object !== null);
  const choices = (results.data ?? []).filter((summary) => summary.object.id !== object?.id);

  const close = () => {
    setText("");
    setError(null);
    onClose();
  };

  const relateTo = async (other: Id) => {
    if (!object) return;
    const made = await actions.relate(object.id, other);
    if (made === undefined) {
      setError("The relation was not made.");
      return;
    }
    close();
  };

  return (
    <Dialog
      open={object !== null}
      onClose={close}
      title={`Relate “${object && object.title !== "" ? object.title : "Untitled"}” to…`}
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
      <ul className="dbme-move__list" aria-label="Pages to relate to">
        {choices.map((summary) => (
          <li key={summary.object.id}>
            <Button
              variant="ghost"
              className="dbme-move__choice"
              onClick={() => void relateTo(summary.object.id)}
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
