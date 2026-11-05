// SPDX-License-Identifier: AGPL-3.0-or-later
/** The Kanban board's rules (M8, Task 4), apart from drawing it: its columns and how keys move. */
import type { FieldRow } from "@databaseme/client";
import type { Listing } from "../framework/listing";
import { optionsOf } from "../values";

export interface Column {
  /** Stable across renders, and never the same as an option's ID. */
  id: string;
  /** What the field holds in this column; null where it holds nothing. */
  value: string | null;
  label: string;
}

/** A field's choices, in the order the field gives them, then a column for no value. */
export function columnsOf(field: Pick<FieldRow, "options_json">): Column[] {
  return [
    ...optionsOf(field).map((option) => ({
      id: `option:${option.id}`,
      value: option.id,
      label: option.label,
    })),
    { id: "none", value: null, label: "No value" },
  ];
}

/** The cards of one column: what the board lists, narrowed to the column's value. */
export function columnListing(listing: Listing, key: string, column: Column): Listing {
  return {
    source: listing.source,
    conditions: [
      ...listing.conditions,
      column.value === null
        ? { field: key, op: "is_empty", value: null }
        : { field: key, op: "eq", value: column.value },
    ],
  };
}

export type BoardKey =
  | { kind: "row"; row: number }
  | { kind: "column"; step: 1 | -1 }
  | { kind: "card-to-column"; step: 1 | -1 }
  | { kind: "card-in-column"; step: 1 | -1 }
  | { kind: "open" }
  | null;

/**
 * What a key does in a column of `total` cards, at `row`. The arrows move the selection; with Alt
 * they carry the card, across columns or along one. A key with nowhere to go does nothing and is
 * still the board's, so the page does not scroll under it.
 */
export function boardKey(
  key: string,
  alt: boolean,
  row: number,
  total: number,
  page: number,
): BoardKey {
  const last = Math.max(0, total - 1);
  const clamp = (at: number) => Math.min(last, Math.max(0, at));
  switch (key) {
    case "ArrowLeft":
      return { kind: alt ? "card-to-column" : "column", step: -1 };
    case "ArrowRight":
      return { kind: alt ? "card-to-column" : "column", step: 1 };
    case "ArrowUp":
      return alt ? { kind: "card-in-column", step: -1 } : { kind: "row", row: clamp(row - 1) };
    case "ArrowDown":
      return alt ? { kind: "card-in-column", step: 1 } : { kind: "row", row: clamp(row + 1) };
    case "PageUp":
      return { kind: "row", row: clamp(row - page) };
    case "PageDown":
      return { kind: "row", row: clamp(row + page) };
    case "Home":
      return { kind: "row", row: 0 };
    case "End":
      return { kind: "row", row: last };
    case "Enter":
      return { kind: "open" };
    default:
      return null;
  }
}
