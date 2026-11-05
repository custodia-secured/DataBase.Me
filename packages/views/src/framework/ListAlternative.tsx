// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * The accessible alternative a visual view switches to (M8, brief principle 10): the same objects as
 * a keyboard grid, with what the picture encodes written out in columns. Virtualised and read in
 * pages like the Table, and one tab stop like it: the arrow keys move, Enter opens.
 */
import type { ObjectSummary_Serialize, Sort } from "@databaseme/client";
import { StatusMessage, useMediaQuery } from "@databaseme/ui";
import { useVirtualizer } from "@tanstack/react-virtual";
import { useEffect, useId, useRef, useState, type KeyboardEvent } from "react";
import { moveCell, type Cell } from "../grid";
import { useListingCount, usePagedListing, type Listing } from "./listing";

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
  const gridId = useId();
  const count = useListingCount(listing);
  const total = count.data ?? 0;
  const coarse = useMediaQuery("(pointer: coarse)");
  const rowHeight = coarse ? 44 : 36;
  const scroller = useRef<HTMLDivElement>(null);
  const [active, setActive] = useState<Cell>({ row: 0, col: 0 });
  const cols = columns.length + 1;
  const template = `minmax(12rem, 2fr)${cols > 1 ? ` repeat(${String(cols - 1)}, minmax(9rem, 1fr))` : ""}`;

  // eslint-disable-next-line react-hooks/incompatible-library -- TanStack Virtual returns new functions each render; nothing here memoises them
  const virtualizer = useVirtualizer({
    count: total,
    getScrollElement: () => scroller.current,
    estimateSize: () => rowHeight,
    overscan: 12,
  });
  const items = virtualizer.getVirtualItems();
  const rowAt = usePagedListing(listing, sort, items[0]?.index ?? 0, items.at(-1)?.index ?? 0);

  useEffect(() => {
    if (active.row >= total && total > 0) setActive({ row: total - 1, col: active.col });
  }, [total, active]);

  const cellId = (cell: Cell) => `${gridId}-r${String(cell.row)}-c${String(cell.col)}`;
  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (total === 0) return;
    const moved = moveCell(event.key, event.ctrlKey || event.metaKey, active, {
      rows: total,
      cols,
      page: Math.max(1, Math.floor((scroller.current?.clientHeight ?? 400) / rowHeight) - 1),
      header: false,
    });
    if (moved) {
      event.preventDefault();
      setActive(moved);
      virtualizer.scrollToIndex(moved.row, { align: "auto" });
    } else if (event.key === "Enter") {
      event.preventDefault();
      const summary = rowAt(active.row);
      if (summary) onOpen(summary.object.id);
    }
  };

  return (
    <div
      ref={scroller}
      className="dbme-grid"
      role="grid"
      aria-label={label}
      aria-rowcount={total + 1}
      aria-colcount={cols}
      aria-activedescendant={total > 0 ? cellId(active) : undefined}
      tabIndex={0}
      onKeyDown={onKeyDown}
    >
      <div role="rowgroup" className="dbme-grid__head">
        <div
          role="row"
          aria-rowindex={1}
          className="dbme-grid__row"
          style={{ gridTemplateColumns: template }}
        >
          {["Title", ...columns.map((column) => column.label)].map((header, col) => (
            <div
              key={header}
              role="columnheader"
              aria-colindex={col + 1}
              className="dbme-grid__cell dbme-grid__header"
            >
              {header}
            </div>
          ))}
        </div>
      </div>
      <div
        role="rowgroup"
        className="dbme-grid__body"
        style={{ height: `${String(virtualizer.getTotalSize())}px` }}
      >
        {items.map((item) => {
          const summary = rowAt(item.index);
          const values = summary
            ? [
                summary.object.title || "Untitled",
                ...columns.map((column) => column.value(summary)),
              ]
            : Array.from({ length: cols }, () => null);
          return (
            <div
              key={item.key}
              role="row"
              aria-rowindex={item.index + 2}
              className="dbme-grid__row"
              style={{
                gridTemplateColumns: template,
                height: `${String(item.size)}px`,
                transform: `translateY(${String(item.start)}px)`,
              }}
            >
              {values.map((value, col) => {
                const here = { row: item.index, col };
                const isActive = active.row === item.index && active.col === col;
                return (
                  <div
                    key={col}
                    id={cellId(here)}
                    role="gridcell"
                    aria-colindex={col + 1}
                    className={`dbme-grid__cell${isActive ? " is-active" : ""}`}
                    onClick={() => {
                      if (isActive && summary) onOpen(summary.object.id);
                      else setActive(here);
                      scroller.current?.focus();
                    }}
                  >
                    {value === null ? (
                      <span className="dbme-grid__placeholder" aria-hidden="true" />
                    ) : (
                      <span className="dbme-grid__text">{value}</span>
                    )}
                  </div>
                );
              })}
            </div>
          );
        })}
      </div>
      {count.isSuccess && total === 0 ? <StatusMessage>Nothing to show yet.</StatusMessage> : null}
    </div>
  );
}
