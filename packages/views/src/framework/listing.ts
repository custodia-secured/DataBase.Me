// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * What a view lists (M8, ADR-0031): a source and the conditions on it. With no conditions it reads
 * the source's own pages, as the Table and Gallery always did, so their budgets do not move. With
 * conditions it reads the same rows through the query DSL (ADR-0020), which is also the form a saved
 * view is stored in.
 */
import {
  commands,
  keys,
  MANUAL,
  unwrap,
  useCount,
  useQueryCount,
  type Condition_Deserialize,
  type Id,
  type ObjectSummary_Serialize,
  type Query_Deserialize,
  type Query_Serialize,
  type Sort,
  type Source,
} from "@databaseme/client";
import { useQueries } from "@tanstack/react-query";
import { childrenOf, PAGE } from "../paging";

export interface Listing {
  source: Source;
  conditions: readonly Condition_Deserialize[];
}

/** The children of `objectId`, with nothing filtered out. */
export function listingOf(objectId: Id): Listing {
  return { source: childrenOf(objectId), conditions: [] };
}

const PARENT = "$parent_id";

/** A listing as a query in the DSL. */
export function listingQuery(
  { source, conditions }: Listing,
  sort: Sort = MANUAL,
  offset = 0,
  limit = PAGE,
): Query_Deserialize {
  const parent: Condition_Deserialize[] =
    source.kind !== "children"
      ? []
      : source.parent
        ? [{ field: PARENT, op: "eq", value: source.parent }]
        : [{ field: PARENT, op: "is_empty", value: null }];
  return {
    from: source.kind === "type" ? { types: [source.type_id] } : {},
    where: [...parent, ...conditions],
    sort,
    limit,
    offset,
  };
}

/** The listing and order a saved query stands for. A query with no source lists `objectId`. */
export function listingFromQuery(
  query: Query_Serialize,
  objectId: Id,
): { listing: Listing; sort: Sort } {
  const parent = query.where.find((condition) => condition.field === PARENT);
  const conditions = query.where.filter((condition) => condition.field !== PARENT);
  const type = query.from.types[0];
  let source: Source;
  if (type !== undefined) source = { kind: "type", type_id: type };
  else if (parent?.op === "eq" && typeof parent.value === "string")
    source = childrenOf(parent.value);
  else if (parent?.op === "is_empty") source = { kind: "children", parent: null };
  else source = childrenOf(objectId);
  return { listing: { source, conditions }, sort: query.sort };
}

/** How many objects a listing has. */
export function useListingCount(listing: Listing) {
  const plain = listing.conditions.length === 0;
  const all = useCount(listing.source);
  const filtered = useQueryCount(plain ? null : listingQuery(listing));
  return plain ? all : filtered;
}

/** The objects from `first` to `last` (inclusive) of a listing in `sort` order, by index. */
export function usePagedListing(
  listing: Listing,
  sort: Sort,
  first: number,
  last: number,
): (index: number) => ObjectSummary_Serialize | undefined {
  const firstPage = Math.max(0, Math.floor(first / PAGE));
  const lastPage = Math.max(firstPage, Math.floor(last / PAGE));
  const plain = listing.conditions.length === 0;
  const pages = useQueries({
    queries: Array.from({ length: lastPage - firstPage + 1 }, (_, i) => {
      const offset = (firstPage + i) * PAGE;
      if (plain) {
        return {
          queryKey: keys.page(listing.source, sort, offset, PAGE),
          queryFn: () => unwrap(commands.objectsPage(listing.source, sort, offset, PAGE)),
        };
      }
      const query = listingQuery(listing, sort, offset, PAGE);
      return {
        queryKey: keys.query(query),
        queryFn: () => unwrap(commands.objectsQuery(query)),
      };
    }),
  });
  return (index) => pages[Math.floor(index / PAGE) - firstPage]?.data?.[index % PAGE];
}
