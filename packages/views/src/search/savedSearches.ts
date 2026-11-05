// SPDX-License-Identifier: AGPL-3.0-or-later
import type { Id } from "@databaseme/client";

/** The saved views that belong to no object: searches, which open as results (ADR-0020). */
export function savedSearches<V extends { owner_object_id: Id | null }>(
  views: readonly V[] | undefined,
): readonly V[] {
  return (views ?? []).filter((view) => view.owner_object_id === null);
}
