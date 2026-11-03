// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * Modes (M7, ADR-0027; brief §2.9): presets of what is shown, never a change to the vault.
 * Simple reads as a plain notes app: pages, search, the Document, Outline, Mind map and Gallery
 * views, the files folder, import and the daily journal, with types and fields out of sight.
 * Organizer shows everything of M1 to M6. A vault made in one opens unchanged in the other.
 * Which views each mode offers is the registry's (`visibleViews`).
 */
import { useSettings, type Mode } from "@databaseme/client";

export type { Mode };

/** The mode in effect; Simple until the settings are read, which is also their default. */
export function useMode(): Mode {
  return useSettings().data?.mode ?? "simple";
}

/** Whether types, fields and the type picker are shown. */
export function showsTypes(mode: Mode): boolean {
  return mode === "organizer";
}

/** Today's journal page title, the local date as `YYYY-MM-DD`. */
export function todayTitle(now: Date = new Date()): string {
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${String(now.getFullYear())}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`;
}

/** The page the daily notes live under. */
export const JOURNAL_TITLE = "Journal";
