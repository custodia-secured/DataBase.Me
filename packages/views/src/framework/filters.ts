// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * The filter editor's rules (M8, M4 Decision 8): which comparisons a field offers, and how a row a
 * person fills in becomes a condition in the DSL and back. Pure, so every kind is tested without a
 * dialog.
 */
import type {
  Compare,
  Condition_Deserialize,
  FieldKind,
  FieldRow,
  Value,
} from "@databaseme/client";
import { optionsOf, type SelectOption } from "../values";

export type FilterKind = FieldKind | "event";

export interface FilterField {
  /** What a condition names: a field key, or a `$column`. */
  key: string;
  label: string;
  kind: FilterKind;
  options: readonly SelectOption[];
}

export interface Comparison {
  op: Compare;
  label: string;
}

/** A condition as the person sees it: every value is text until it is applied. */
export interface FilterRow {
  /** Keeps a row's inputs in place as other rows come and go. */
  id: number;
  field: string;
  op: Compare;
  text: string;
}

const EMPTY: readonly Comparison[] = [
  { op: "is_empty", label: "is empty" },
  { op: "is_not_empty", label: "is not empty" },
];

const TEXT: readonly Comparison[] = [
  { op: "contains", label: "contains" },
  { op: "eq", label: "is" },
  { op: "ne", label: "is not" },
  { op: "starts_with", label: "starts with" },
  ...EMPTY,
];

const NUMBER: readonly Comparison[] = [
  { op: "eq", label: "=" },
  { op: "ne", label: "≠" },
  { op: "lt", label: "<" },
  { op: "le", label: "≤" },
  { op: "gt", label: ">" },
  { op: "ge", label: "≥" },
  ...EMPTY,
];

const DATE: readonly Comparison[] = [
  { op: "eq", label: "is" },
  { op: "lt", label: "is before" },
  { op: "ge", label: "is on or after" },
  ...EMPTY,
];

/** An event date is a moment in milliseconds: "is" one day would match almost nothing. */
const EVENT: readonly Comparison[] = [
  { op: "lt", label: "is before" },
  { op: "ge", label: "is on or after" },
  ...EMPTY,
];

const CHOICE: readonly Comparison[] = [
  { op: "eq", label: "is" },
  { op: "ne", label: "is not" },
  ...EMPTY,
];

const LIST: readonly Comparison[] = [{ op: "has", label: "includes" }, ...EMPTY];

const YES_NO: readonly Comparison[] = [{ op: "eq", label: "is" }];

export function comparisonsFor(kind: FilterKind): readonly Comparison[] {
  switch (kind) {
    case "text":
    case "rich_text":
    case "url":
    case "email":
    case "phone":
      return TEXT;
    case "number":
    case "currency":
    case "duration":
    case "rating":
      return NUMBER;
    case "date":
    case "datetime":
      return DATE;
    case "event":
      return EVENT;
    case "select":
      return CHOICE;
    case "multi_select":
      return LIST;
    case "boolean":
      return YES_NO;
    default:
      // A relation, a file, a location, a formula: whether it has a value is all a row can say.
      return EMPTY;
  }
}

/** Whether a comparison takes a value at all. */
export function takesValue(op: Compare): boolean {
  return op !== "is_empty" && op !== "is_not_empty";
}

const TITLE: FilterField = { key: "$title", label: "Title", kind: "text", options: [] };
const EVENT_DATE: FilterField = {
  key: "$event_start",
  label: "Event date",
  kind: "event",
  options: [],
};

/** What a listing can be filtered by: the title, the event date and the fields of its types. */
export function filterFields(fields: readonly FieldRow[]): FilterField[] {
  const seen = new Set<string>();
  const own: FilterField[] = [];
  for (const field of fields) {
    if (seen.has(field.key)) continue;
    seen.add(field.key);
    own.push({
      key: field.key,
      label: field.label,
      kind: field.kind,
      options: optionsOf(field),
    });
  }
  return [TITLE, EVENT_DATE, ...own];
}

/** The field a row or a condition names, even when the listing no longer shows it. */
export function fieldNamed(fields: readonly FilterField[], key: string): FilterField {
  return (
    fields.find((field) => field.key === key) ?? { key, label: key, kind: "text", options: [] }
  );
}

/** A new row on `field`, with its first comparison and the value it starts from. */
export function newRow(id: number, field: FilterField): FilterRow {
  const op = comparisonsFor(field.kind)[0]?.op ?? "is_not_empty";
  return { id, field: field.key, op, text: startingText(field) };
}

/** What a row's value starts as: the first choice where the value is a choice, else nothing. */
export function startingText(field: FilterField): string {
  if (field.kind === "boolean") return "true";
  if (field.kind === "select" || field.kind === "multi_select") return field.options[0]?.id ?? "";
  return "";
}

const DATE_TEXT = /^\d{4}-\d{2}-\d{2}$/;

function isRealDate(text: string): boolean {
  if (!DATE_TEXT.test(text)) return false;
  const when = new Date(`${text}T00:00:00Z`);
  return !Number.isNaN(when.getTime()) && when.toISOString().startsWith(text);
}

export type RowResult = { condition: Condition_Deserialize } | { error: string };

/** A row as a condition, or what is wrong with it. */
export function rowToCondition(row: FilterRow, field: FilterField): RowResult {
  const { op } = row;
  const base = { field: row.field, op };
  if (!takesValue(op)) return { condition: { ...base, value: null } };
  const text = row.text.trim();
  switch (field.kind) {
    case "boolean":
      return { condition: { ...base, value: text === "true" } };
    case "number":
    case "currency":
    case "duration":
    case "rating": {
      const value = Number(text);
      if (text === "" || !Number.isFinite(value)) return { error: "Enter a number" };
      return { condition: { ...base, value } };
    }
    case "date":
    case "datetime":
      if (!isRealDate(text)) return { error: "Enter a date as YYYY-MM-DD" };
      return { condition: { ...base, value: text } };
    case "event":
      if (!isRealDate(text)) return { error: "Enter a date as YYYY-MM-DD" };
      return { condition: { ...base, value: Date.parse(`${text}T00:00:00Z`) } };
    case "select":
    case "multi_select":
      if (text === "") return { error: "This field has no choices yet" };
      return { condition: { ...base, value: text } };
    default:
      if (text === "") return { error: "Enter some text" };
      return { condition: { ...base, value: text } };
  }
}

function valueText(value: Value | undefined, kind: FilterKind): string {
  if (value === null || value === undefined) return "";
  if (kind === "event" && typeof value === "number") {
    return new Date(value).toISOString().slice(0, 10);
  }
  if (typeof value === "string") return value;
  if (typeof value === "number" || typeof value === "boolean") return String(value);
  return "";
}

/** A condition as a row, to edit it. */
export function conditionToRow(
  id: number,
  condition: Condition_Deserialize,
  fields: readonly FilterField[],
): FilterRow {
  const field = fieldNamed(fields, condition.field);
  return {
    id,
    field: condition.field,
    op: condition.op,
    text: valueText(condition.value, field.kind),
  };
}
