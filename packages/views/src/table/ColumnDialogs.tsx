// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * The Table's columns (ADR-0030). A column is a field on a type: **New column** adds one and says
 * which type it goes on, and **Edit columns** renames or deletes them. Dialogs, not header controls,
 * so that the grid stays one tab stop and every step works by keyboard and touch.
 */
import {
  useColumnTarget,
  useTypes,
  type FieldKind,
  type FieldRow,
  type Source,
} from "@databaseme/client";
import { Button, Dialog, StatusMessage } from "@databaseme/ui";
import { useId, useState, type KeyboardEvent, type SyntheticEvent } from "react";
import { actions } from "../actions";
import { hasColumn, parseChoices, targetText } from "./columns";

/** The kinds a Table's cells can edit, as the store allows them for a column. */
const KINDS: readonly { kind: FieldKind; label: string }[] = [
  { kind: "text", label: "Text" },
  { kind: "number", label: "Number" },
  { kind: "date", label: "Date" },
  { kind: "boolean", label: "Yes or no" },
  { kind: "select", label: "Choice from a list" },
  { kind: "url", label: "Web address" },
  { kind: "email", label: "Email" },
  { kind: "phone", label: "Phone" },
];

export function NewColumnDialog({
  open,
  source,
  fields,
  rows,
  onClose,
}: {
  open: boolean;
  source: Source;
  /** The columns the Table shows now. */
  fields: readonly FieldRow[];
  /** How many rows the Table has. */
  rows: number;
  onClose: () => void;
}) {
  const nameId = useId();
  const kindId = useId();
  const choicesId = useId();
  const [name, setName] = useState("");
  const [kind, setKind] = useState<FieldKind>("text");
  const [choices, setChoices] = useState("");
  const [error, setError] = useState<string | null>(null);
  const target = useColumnTarget(source, open);
  const typed = parseChoices(choices);
  const ready = name.trim() !== "" && (kind !== "select" || typed.length > 0);

  const close = () => {
    setName("");
    setKind("text");
    setChoices("");
    setError(null);
    onClose();
  };

  const submit = async (event: SyntheticEvent) => {
    event.preventDefault();
    if (!ready) return;
    if (hasColumn(fields, name)) {
      setError(`There is already a column called “${name.trim()}”.`);
      return;
    }
    const added = await actions.addColumn(source, {
      label: name.trim(),
      kind,
      choices: kind === "select" ? typed : [],
    });
    if (added === undefined) {
      setError("The column was not added.");
      return;
    }
    close();
  };

  return (
    <Dialog open={open} onClose={close} title="New column">
      <form className="dbme-columns__form" onSubmit={(event) => void submit(event)}>
        <label htmlFor={nameId}>Name</label>
        <input
          id={nameId}
          className="dbme-input"
          type="text"
          value={name}
          autoComplete="off"
          aria-invalid={error !== null}
          onChange={(event) => {
            setName(event.target.value);
            setError(null);
          }}
        />
        <label htmlFor={kindId}>Holds</label>
        <select
          id={kindId}
          className="dbme-input"
          value={kind}
          onChange={(event) => {
            setKind(event.target.value as FieldKind);
          }}
        >
          {KINDS.map((option) => (
            <option key={option.kind} value={option.kind}>
              {option.label}
            </option>
          ))}
        </select>
        {kind === "select" ? (
          <>
            <label htmlFor={choicesId}>Choices, separated by commas</label>
            <input
              id={choicesId}
              className="dbme-input"
              type="text"
              value={choices}
              autoComplete="off"
              onChange={(event) => {
                setChoices(event.target.value);
              }}
            />
          </>
        ) : null}
        {target.data ? (
          <p className="dbme-muted dbme-columns__note">{targetText(source, target.data, rows)}</p>
        ) : null}
        {error ? <StatusMessage tone="error">{error}</StatusMessage> : null}
        <div className="dbme-dialog__actions">
          <Button onClick={close}>Cancel</Button>
          <Button type="submit" variant="primary" disabled={!ready}>
            Add column
          </Button>
        </div>
      </form>
    </Dialog>
  );
}

/** One column to rename or delete. A new name is saved on Enter or on leaving the input. */
function ColumnRow({ field, typeName }: { field: FieldRow; typeName: string | undefined }) {
  const [text, setText] = useState(field.label);
  // The label can change from outside: undo, or the other pane.
  const [shown, setShown] = useState(field.label);
  if (shown !== field.label) {
    setShown(field.label);
    setText(field.label);
  }

  const save = async () => {
    const label = text.trim();
    if (label === field.label) {
      setText(label);
      return;
    }
    const saved = await actions.renameField(field.id, label);
    if (saved === undefined) setText(field.label);
  };

  const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === "Enter") {
      event.preventDefault();
      void save();
    }
    // Escape closes the dialog, and the input loses focus as it does: forget the typing first.
    if (event.key === "Escape") setText(field.label);
  };

  return (
    <li className="dbme-columns__row">
      <input
        className="dbme-input"
        type="text"
        aria-label={`Name of the ${field.label} column`}
        value={text}
        autoComplete="off"
        onChange={(event) => {
          setText(event.target.value);
        }}
        onKeyDown={onKeyDown}
        onBlur={() => void save()}
      />
      <span className="dbme-muted dbme-columns__type">{typeName}</span>
      <Button
        variant="danger"
        aria-label={`Delete the ${field.label} column`}
        onClick={() => void actions.deleteField(field.id)}
      >
        Delete
      </Button>
    </li>
  );
}

export function EditColumnsDialog({
  open,
  fields,
  onClose,
}: {
  open: boolean;
  /** The columns the Table shows now. */
  fields: readonly FieldRow[];
  onClose: () => void;
}) {
  const types = useTypes();
  const nameOf = (field: FieldRow) => types.data?.find((type) => type.id === field.type_id)?.name;

  return (
    <Dialog open={open} onClose={onClose} title="Edit columns">
      <p className="dbme-muted dbme-columns__note">
        A column belongs to its type, so a change shows on every page of that type. Deleting a
        column keeps the values on the pages, and Undo brings it back.
      </p>
      {fields.length === 0 ? (
        <StatusMessage>No columns to edit.</StatusMessage>
      ) : (
        <ul className="dbme-columns__list" aria-label="Columns">
          {fields.map((field) => (
            <ColumnRow key={field.id} field={field} typeName={nameOf(field)} />
          ))}
        </ul>
      )}
      <div className="dbme-dialog__actions">
        <Button variant="primary" onClick={onClose}>
          Done
        </Button>
      </div>
    </Dialog>
  );
}
