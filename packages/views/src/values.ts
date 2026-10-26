// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * Field values: how each kind is shown and read back from what a person typed (FORMAT.md §3.3).
 * The kinds M2 edits are text, url, email, phone, number, boolean, date and select; the others are
 * shown read-only until their editors arrive.
 */
import type { FieldKind, FieldRow, Value } from "@databaseme/client";

export interface SelectOption {
  id: string;
  label: string;
}

/** The options of a select field, labelled by their label or, without one, their ID. */
export function optionsOf(field: Pick<FieldRow, "options_json">): SelectOption[] {
  const options = (field.options_json as { options?: unknown } | null)?.options;
  if (!Array.isArray(options)) return [];
  return options.flatMap((option: unknown) => {
    if (typeof option !== "object" || option === null) return [];
    const { id, label } = option as { id?: unknown; label?: unknown };
    if (typeof id !== "string") return [];
    return [{ id, label: typeof label === "string" && label !== "" ? label : id }];
  });
}

const EDITABLE = new Set<FieldKind>([
  "text",
  "url",
  "email",
  "phone",
  "number",
  "boolean",
  "date",
  "select",
]);

/** Whether M2 can edit values of this kind. */
export function isEditable(kind: FieldKind): boolean {
  return EDITABLE.has(kind);
}

/** A value as text for a cell or a property. */
export function displayValue(
  field: Pick<FieldRow, "kind" | "options_json">,
  value: Value | undefined,
): string {
  if (value === undefined || value === null) return "";
  switch (field.kind) {
    case "boolean":
      return value === true ? "Yes" : "No";
    case "select": {
      const option = optionsOf(field).find((candidate) => candidate.id === value);
      return option?.label ?? (typeof value === "string" ? value : JSON.stringify(value));
    }
    case "number":
      return typeof value === "number" ? value.toLocaleString() : JSON.stringify(value);
    default:
      return typeof value === "string" ? value : JSON.stringify(value);
  }
}

/** The text an input starts with when editing `value`. */
export function inputText(value: Value | undefined): string {
  if (value === undefined || value === null) return "";
  return typeof value === "string" ? value : typeof value === "number" ? String(value) : "";
}

/** Reads what was typed into a field of `kind`. Empty input clears the value (`null`). */
export function parseInput(
  kind: FieldKind,
  text: string,
): { value: Value | null } | { error: string } {
  const trimmed = text.trim();
  if (trimmed === "") return { value: null };
  switch (kind) {
    case "number": {
      const number = Number(trimmed);
      return Number.isFinite(number) ? { value: number } : { error: "Enter a number" };
    }
    case "date":
      return /^\d{4}-\d{2}-\d{2}$/.test(trimmed)
        ? { value: trimmed }
        : { error: "Enter a date as YYYY-MM-DD" };
    default:
      return { value: kind === "text" ? text : trimmed };
  }
}
