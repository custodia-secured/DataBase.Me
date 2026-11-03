// SPDX-License-Identifier: AGPL-3.0-or-later
/** What the Conflicts inbox says about each kind of entry (ADR-0021). */
import type { Conflict_Serialize, ConflictKind } from "@databaseme/client";

/** The entry kinds, in words. */
export const KIND_LABELS: Record<ConflictKind, string> = {
  edit_edit: "Edited in both places",
  edit_delete: "Edited here, deleted there",
  parse_error: "File could not be read",
  invalid_value: "A value does not fit its field",
  duplicate_id: "Two files carry the same id",
  dropped_file: "A file was dropped into the folder",
  bulk_change_hold: "Many files changed at once",
  missing_id: "A file had no id",
  cloud_conflict_file: "A cloud drive's conflicting copy",
};

/** What the entry asks the person to do, or tells them, in one sentence. */
export const KIND_HELP: Record<ConflictKind, string> = {
  edit_edit:
    "The app's version stands. The file's version is kept as its own page, so nothing is lost.",
  edit_delete: "The app's edit stands; the file was written again.",
  parse_error: "Fix the file, or delete it. Its page is not written until the file reads again.",
  invalid_value: "The page kept its value; the file was written again with it.",
  duplicate_id: "The second file was given a new id and became its own page.",
  dropped_file: "The file became a page with the file attached.",
  bulk_change_hold: "Nothing was applied. Proceed to apply the changes, or restore the folder.",
  missing_id: "The file was matched by its path and given its id.",
  cloud_conflict_file: "The copy was kept as its own page, not merged into the original.",
};

/** A short line from the entry's details, when they carry one. */
export function detailLine(conflict: Conflict_Serialize): string | null {
  const details: unknown = conflict.details;
  if (typeof details !== "object" || details === null || Array.isArray(details)) return null;
  const record = details as Record<string, unknown>;
  const message = record.message;
  if (typeof message === "string" && message !== "") {
    const line = record.line;
    return typeof line === "number" ? `Line ${String(line)}: ${message}` : message;
  }
  if (Array.isArray(record.removed) || Array.isArray(record.rewritten)) {
    const removed = Array.isArray(record.removed) ? record.removed.length : 0;
    const rewritten = Array.isArray(record.rewritten) ? record.rewritten.length : 0;
    const parts = [];
    if (removed > 0) parts.push(`${String(removed)} removed`);
    if (rewritten > 0) parts.push(`${String(rewritten)} rewritten`);
    return parts.join(", ") || null;
  }
  if (Array.isArray(details) || typeof record.also_at === "string") {
    return `Also at ${String(record.also_at)}`;
  }
  return null;
}
