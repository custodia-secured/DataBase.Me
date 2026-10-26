// SPDX-License-Identifier: AGPL-3.0-or-later
/** Editors for one field value, used by the Table's cells and the Document's properties. */
import type { FieldRow, Value } from "@databaseme/client";
import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import { inputText, optionsOf, parseInput } from "./values";

export interface FieldEditorProps {
  field: Pick<FieldRow, "kind" | "options_json" | "label">;
  value: Value | undefined;
  /** Saves a new value; `null` clears it. */
  onCommit: (value: Value | null) => void;
  onCancel: () => void;
  /** Text typed to start editing, which replaces the value. */
  initialText?: string;
  /** Called after a commit or cancel with the key that ended the edit, to move on. */
  onDone?: (key: "Enter" | "Tab" | "ShiftTab" | "Escape" | "blur") => void;
}

/** An input for a value of `field`, focused on mount. Enter or Tab saves; Escape cancels. */
export function FieldEditor({
  field,
  value,
  onCommit,
  onCancel,
  initialText,
  onDone,
}: FieldEditorProps) {
  const [text, setText] = useState(initialText ?? inputText(value));
  const [error, setError] = useState<string | null>(null);
  const ref = useRef<HTMLInputElement & HTMLSelectElement>(null);
  const finished = useRef(false);
  // Arrow keys on a closed select change its value at once in some browsers; while the keyboard
  // browses the options only Enter, Tab or leaving saves. A pick with a pointer saves at once.
  const browsing = useRef(false);

  useEffect(() => {
    ref.current?.focus();
    if (initialText === undefined && ref.current instanceof HTMLInputElement) ref.current.select();
  }, [initialText]);

  const commit = (key: "Enter" | "Tab" | "ShiftTab" | "blur", raw = text) => {
    if (finished.current) return;
    const parsed = parseInput(field.kind, raw);
    if ("error" in parsed) {
      if (key === "blur") {
        finished.current = true;
        onCancel();
        onDone?.("blur");
      } else {
        setError(parsed.error);
      }
      return;
    }
    finished.current = true;
    const unchanged = (parsed.value ?? undefined) === (value ?? undefined);
    if (!unchanged) onCommit(parsed.value);
    else onCancel();
    onDone?.(key);
  };

  const onKeyDown = (event: KeyboardEvent) => {
    event.stopPropagation();
    if (event.key === "Enter") {
      event.preventDefault();
      commit("Enter");
    } else if (event.key === "Tab") {
      event.preventDefault();
      commit(event.shiftKey ? "ShiftTab" : "Tab");
    } else if (event.key === "Escape") {
      event.preventDefault();
      finished.current = true;
      onCancel();
      onDone?.("Escape");
    }
  };

  if (field.kind === "select") {
    return (
      <select
        ref={ref}
        className="dbme-input dbme-field-editor"
        aria-label={field.label}
        value={text}
        onChange={(event) => {
          setText(event.target.value);
          if (!browsing.current) commit("Enter", event.target.value);
        }}
        onPointerDown={() => {
          browsing.current = false;
        }}
        onKeyDown={(event) => {
          if (!["Enter", "Tab", "Escape"].includes(event.key)) browsing.current = true;
          onKeyDown(event);
        }}
        onBlur={() => {
          commit("blur");
        }}
      >
        <option value="">None</option>
        {optionsOf(field).map((option) => (
          <option key={option.id} value={option.id}>
            {option.label}
          </option>
        ))}
      </select>
    );
  }

  const type =
    field.kind === "number"
      ? "number"
      : field.kind === "date"
        ? "date"
        : field.kind === "email"
          ? "email"
          : field.kind === "url"
            ? "url"
            : field.kind === "phone"
              ? "tel"
              : "text";
  return (
    <span className="dbme-field-editor">
      <input
        ref={ref}
        className="dbme-input"
        type={type}
        aria-label={field.label}
        aria-invalid={error !== null}
        value={text}
        onChange={(event) => {
          setText(event.target.value);
          setError(null);
        }}
        onKeyDown={onKeyDown}
        onBlur={() => {
          commit("blur");
        }}
      />
      {error ? (
        <span className="dbme-field-editor__error" role="alert">
          {error}
        </span>
      ) : null}
    </span>
  );
}
