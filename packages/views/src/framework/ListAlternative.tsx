// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * The accessible alternative a visual view switches to (M8, brief principle 10): the same objects as
 * a keyboard grid, with what the picture encodes written out in columns, read in pages like the Table.
 */
import type { ObjectSummary_Serialize, Sort } from "@databaseme/client";
import { useListingCount, usePagedListing, type Listing } from "./listing";
import { useVirtualGrid } from "./useVirtualGrid";
import { VirtualGrid } from "./VirtualGrid";

export interface ListColumn {
  label: string;
  value: (summary: ObjectSummary_Serialize) => string;
}

export function ListAlternative({
  label,
  listing,
  sort,
  columns,
  onOpen,
}: {
  /** The grid's accessible name. */
  label: string;
  listing: Listing;
  sort: Sort;
  /** What the picture shows besides each object's title. */
  columns: readonly ListColumn[];
  onOpen: (id: string) => void;
}) {
  const count = useListingCount(listing);
  const total = count.data ?? 0;
  const grid = useVirtualGrid(total, columns.length + 1);
  const rowAt = usePagedListing(listing, sort, grid.first, grid.last);
  return (
    <VirtualGrid
      grid={grid}
      label={label}
      headers={["Title", ...columns.map((column) => column.label)]}
      row={(index) => {
        const summary = rowAt(index);
        return summary
          ? [summary.object.title || "Untitled", ...columns.map((column) => column.value(summary))]
          : undefined;
      }}
      onOpen={(index) => {
        const summary = rowAt(index);
        if (summary) onOpen(summary.object.id);
      }}
      emptyWhen={count.isSuccess && total === 0}
    />
  );
}
