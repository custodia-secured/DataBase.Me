// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * Paged reads for the virtualised views (ADR-0017): they are read in pages of {@link PAGE}, so a
 * list of 100,000 objects costs a few small reads, and a change refreshes only the pages on screen.
 * `framework/listing.ts` reads them.
 */
import type { Id, Source } from "@databaseme/client";

export const PAGE = 200;

/** The children of `objectId`, the list a Table or Gallery shows first. */
export function childrenOf(objectId: Id): Source {
  return { kind: "children", parent: objectId };
}

/** Two sources are the same list. */
export function sameSource(a: Source, b: Source): boolean {
  if (a.kind === "children" && b.kind === "children")
    return (a.parent ?? null) === (b.parent ?? null);
  if (a.kind === "type" && b.kind === "type") return a.type_id === b.type_id;
  return false;
}
