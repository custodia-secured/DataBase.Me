// SPDX-License-Identifier: AGPL-3.0-or-later
/** What the Table's column dialogs read and say (ADR-0030). */
import type { ColumnTarget_Serialize, FieldRow, Source } from "@databaseme/client";

/** The choices typed for a select column: separated by commas, blanks dropped. */
export function parseChoices(text: string): string[] {
  return text
    .split(",")
    .map((choice) => choice.trim())
    .filter((choice) => choice !== "");
}

/** Whether a Table showing `fields` already has a column called `label`. Title is always there. */
export function hasColumn(fields: readonly Pick<FieldRow, "label">[], label: string): boolean {
  const lower = label.trim().toLowerCase();
  return lower === "title" || fields.some((field) => field.label.toLowerCase() === lower);
}

/** Says where a new column goes, before it is added. */
export function targetText(
  source: Source,
  target: Pick<ColumnTarget_Serialize, "type_id" | "type_name">,
  rows: number,
): string {
  const firstRow = source.kind === "children" && rows === 0 ? " It also adds a first row." : "";
  if (source.kind === "type") {
    return `Every page of the type ${target.type_name} gets this column.`;
  }
  return target.type_id === null
    ? `The pages here get a new type, ${target.type_name}, to hold this column.${firstRow}`
    : `This column goes on the type ${target.type_name}, so every page of that type gets it.${firstRow}`;
}
