// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * The Canvas's alternative (M8, Task 12, ADR-0035): everything on it with where it is, in the keyboard grid.
 * Alt and an arrow move the chosen element, Enter opens a card, and nothing here needs the drawing library,
 * which is not even loaded.
 */
import type { CanvasItemRow_Serialize, Id } from "@databaseme/client";
import { useEffect, useRef, type KeyboardEvent } from "react";
import { useVirtualGrid } from "../framework/useVirtualGrid";
import { VirtualGrid } from "../framework/VirtualGrid";
import { kindOf, nameOf, placeLabel, sizeLabel } from "./scene";

/** How far Alt and an arrow moves something. */
export const STEP = 20;

const ARROWS: Readonly<Record<string, readonly [number, number]>> = {
  ArrowLeft: [-STEP, 0],
  ArrowRight: [STEP, 0],
  ArrowUp: [0, -STEP],
  ArrowDown: [0, STEP],
};

export function CardList({
  rows,
  titles,
  onChoose,
  onMove,
  onOpen,
}: {
  rows: readonly CanvasItemRow_Serialize[];
  titles: ReadonlyMap<Id, string>;
  onChoose: (itemId: string | null) => void;
  onMove: (row: CanvasItemRow_Serialize, dx: number, dy: number) => void;
  onOpen: (id: Id) => void;
}) {
  const grid = useVirtualGrid(rows.length, 4);
  const active = rows[grid.active.row];
  const activeId = active?.item_id ?? null;
  const report = useRef(onChoose);
  useEffect(() => {
    report.current = onChoose;
  });
  useEffect(() => {
    report.current(activeId);
  }, [activeId]);

  const onKeyDownCapture = (event: KeyboardEvent<HTMLDivElement>) => {
    const step = ARROWS[event.key];
    if (!event.altKey || !step || !active) return;
    event.preventDefault();
    event.stopPropagation();
    onMove(active, step[0], step[1]);
  };

  return (
    <div className="dbme-canvas__list" onKeyDownCapture={onKeyDownCapture}>
      <VirtualGrid
        grid={grid}
        label="On this canvas"
        headers={["Title", "Kind", "Position", "Size"]}
        row={(index) => {
          const row = rows[index];
          return row && [nameOf(row, titles), kindOf(row), placeLabel(row), sizeLabel(row)];
        }}
        onOpen={(index) => {
          const object = rows[index]?.object_id ?? null;
          if (object !== null) onOpen(object);
        }}
        emptyWhen={rows.length === 0}
      />
    </div>
  );
}
