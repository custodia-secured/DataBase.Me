// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * The keyboard grid behind every view's accessible alternative (M8, brief principle 10): virtualised, and
 * one tab stop like the Table. The arrow keys move and Enter opens. The hook owns what is on screen, so
 * whoever holds the rows knows which to read; the component draws them.
 */
import { StatusMessage } from "@databaseme/ui";
import { useId, type KeyboardEvent } from "react";
import { moveCell, type Cell } from "../grid";
import type { VirtualGridState } from "./useVirtualGrid";

export function VirtualGrid({
  grid,
  label,
  headers,
  row,
  onOpen,
  emptyWhen,
}: {
  grid: VirtualGridState;
  /** The grid's accessible name. */
  label: string;
  headers: readonly string[];
  /** One row's cells, or nothing while it is being read. */
  row: (index: number) => readonly string[] | undefined;
  onOpen: (index: number) => void;
  /** Whether to say there is nothing here. */
  emptyWhen: boolean;
}) {
  const { total, cols, rowHeight, scroller, items, active, setActive } = grid;
  const gridId = useId();
  const template = `minmax(12rem, 2fr)${cols > 1 ? ` repeat(${String(cols - 1)}, minmax(9rem, 1fr))` : ""}`;

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
      grid.scrollToRow(moved.row);
    } else if (event.key === "Enter") {
      event.preventDefault();
      if (row(active.row)) onOpen(active.row);
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
          {headers.map((header, col) => (
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
      <div role="rowgroup" className="dbme-grid__body" style={{ height: `${String(grid.size)}px` }}>
        {items.map((item) => {
          const cells = row(item.index);
          const values = cells ?? Array.from({ length: cols }, () => null);
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
                      if (isActive && cells) onOpen(item.index);
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
      {emptyWhen ? <StatusMessage>Nothing to show yet.</StatusMessage> : null}
    </div>
  );
}
