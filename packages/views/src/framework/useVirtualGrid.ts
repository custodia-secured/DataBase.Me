// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * What a keyboard grid has on screen (M8), so that whoever holds its rows knows which to read, while
 * `VirtualGrid` draws them.
 */
import { useMediaQuery } from "@databaseme/ui";
import { useVirtualizer, type VirtualItem } from "@tanstack/react-virtual";
import { useEffect, useRef, useState, type RefObject } from "react";
import type { Cell } from "../grid";

export interface VirtualGridState {
  total: number;
  cols: number;
  rowHeight: number;
  scroller: RefObject<HTMLDivElement | null>;
  items: readonly VirtualItem[];
  /** The first and last rows on screen, for reading them in pages. */
  first: number;
  last: number;
  active: Cell;
  setActive: (cell: Cell) => void;
  size: number;
  scrollToRow: (row: number) => void;
}

export function useVirtualGrid(total: number, cols: number, startRow = 0): VirtualGridState {
  const coarse = useMediaQuery("(pointer: coarse)");
  const rowHeight = coarse ? 44 : 36;
  const scroller = useRef<HTMLDivElement>(null);
  const [active, setActive] = useState<Cell>({ row: startRow, col: 0 });
  const virtualizer = useVirtualizer({
    count: total,
    getScrollElement: () => scroller.current,
    estimateSize: () => rowHeight,
    overscan: 12,
  });
  const items = virtualizer.getVirtualItems();

  useEffect(() => {
    if (active.row >= total && total > 0) setActive({ row: total - 1, col: active.col });
  }, [total, active]);

  return {
    total,
    cols,
    rowHeight,
    scroller,
    items,
    first: items[0]?.index ?? 0,
    last: items.at(-1)?.index ?? 0,
    active,
    setActive,
    size: virtualizer.getTotalSize(),
    scrollToRow: (row) => {
      virtualizer.scrollToIndex(row, { align: "auto" });
    },
  };
}
