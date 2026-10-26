// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * Paged reads for the virtualised views (ADR-0017). A view asks for the rows it shows; they are
 * read in pages of {@link PAGE}, so a list of 100,000 objects costs a few small reads, and a
 * change refreshes only the pages on screen.
 */
import {
  commands,
  keys,
  unwrap,
  type Id,
  type ObjectSummary_Serialize,
  type Sort,
  type Source,
} from "@databaseme/client";
import { useQueries } from "@tanstack/react-query";

export const PAGE = 200;

/** The rows from `first` to `last` (inclusive) of `source` in `sort` order, by index. */
export function usePagedObjects(
  source: Source,
  sort: Sort,
  first: number,
  last: number,
): (index: number) => ObjectSummary_Serialize | undefined {
  const firstPage = Math.max(0, Math.floor(first / PAGE));
  const lastPage = Math.max(firstPage, Math.floor(last / PAGE));
  const pages = useQueries({
    queries: Array.from({ length: lastPage - firstPage + 1 }, (_, i) => {
      const offset = (firstPage + i) * PAGE;
      return {
        queryKey: keys.page(source, sort, offset, PAGE),
        queryFn: () => unwrap(commands.objectsPage(source, sort, offset, PAGE)),
      };
    }),
  });
  return (index) => pages[Math.floor(index / PAGE) - firstPage]?.data?.[index % PAGE];
}

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
