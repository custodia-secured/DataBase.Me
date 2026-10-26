// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * The Gallery view: the objects of a source as cards, with the first image attached to each as
 * its cover. Rows of cards are virtualised and read in pages, like the Table. Covers are read as
 * bytes over IPC and shown through `blob:` URLs, which the CSP allows. One tab stop; the arrow
 * keys move between cards and Enter opens one.
 */
import {
  MANUAL,
  useBlobUrl,
  useCount,
  type ObjectSummary_Serialize,
  type Sort,
  type Source,
} from "@databaseme/client";
import { StatusMessage } from "@databaseme/ui";
import { useVirtualizer } from "@tanstack/react-virtual";
import { useEffect, useId, useRef, useState, type KeyboardEvent } from "react";
import { moveCell } from "../grid";
import { childrenOf, usePagedObjects } from "../paging";
import type { ViewProps } from "../registry";
import { SourcePicker } from "../SourcePicker";

// Two cards a row on a 360 px phone.
const MIN_CARD = 140;
const GAP = 12;
const CAPTION = 48;

const SORTS: { label: string; sort: Sort }[] = [
  { label: "Manual order", sort: MANUAL },
  { label: "Title", sort: { by: "title", field: null, descending: false } },
  { label: "Newest first", sort: { by: "created", field: null, descending: true } },
  { label: "Recently changed", sort: { by: "updated", field: null, descending: true } },
];

export function GalleryView({ objectId, onOpen }: ViewProps) {
  const [source, setSource] = useState<Source>(() => childrenOf(objectId));
  const [sortIndex, setSortIndex] = useState(0);
  const [shownFor, setShownFor] = useState(objectId);
  if (shownFor !== objectId) {
    setShownFor(objectId);
    setSource(childrenOf(objectId));
  }
  const sortId = useId();
  const sort = SORTS[sortIndex]?.sort ?? MANUAL;
  return (
    <section className="dbme-gallery" aria-label="Gallery">
      <div className="dbme-view-bar">
        <SourcePicker objectId={objectId} source={source} onChange={setSource} />
        <span className="dbme-source">
          <label htmlFor={sortId}>Order</label>
          <select
            id={sortId}
            className="dbme-input"
            value={sortIndex}
            onChange={(event) => {
              setSortIndex(Number(event.target.value));
            }}
          >
            {SORTS.map((option, index) => (
              <option key={option.label} value={index}>
                {option.label}
              </option>
            ))}
          </select>
        </span>
      </div>
      <Cards source={source} sort={sort} onOpen={onOpen} />
    </section>
  );
}

function Cards({
  source,
  sort,
  onOpen,
}: {
  source: Source;
  sort: Sort;
  onOpen: (id: string) => void;
}) {
  const gridId = useId();
  const count = useCount(source);
  const total = count.data ?? 0;
  const scroller = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(0);
  const [active, setActive] = useState(0);

  useEffect(() => {
    const element = scroller.current;
    if (!element) return;
    const observer = new ResizeObserver(([entry]) => {
      setWidth(entry?.contentRect.width ?? 0);
    });
    observer.observe(element);
    return () => {
      observer.disconnect();
    };
  }, []);

  const perRow = Math.max(1, Math.floor((width + GAP) / (MIN_CARD + GAP)));
  const cardWidth = width > 0 ? (width - GAP * (perRow - 1)) / perRow : MIN_CARD;
  const rowHeight = Math.round(cardWidth * 0.75 + CAPTION + GAP);
  const rowCount = Math.ceil(total / perRow);

  // eslint-disable-next-line react-hooks/incompatible-library -- TanStack Virtual returns new functions each render; nothing here memoises them
  const virtualizer = useVirtualizer({
    count: rowCount,
    getScrollElement: () => scroller.current,
    estimateSize: () => rowHeight,
    overscan: 3,
  });
  useEffect(() => {
    virtualizer.measure();
  }, [virtualizer, rowHeight]);

  const items = virtualizer.getVirtualItems();
  const first = (items[0]?.index ?? 0) * perRow;
  const last = ((items.at(-1)?.index ?? 0) + 1) * perRow - 1;
  const cardAt = usePagedObjects(source, sort, first, Math.min(last, Math.max(0, total - 1)));

  const cardId = (index: number) => `${gridId}-card-${String(index)}`;
  const go = (index: number) => {
    const next = Math.max(0, Math.min(total - 1, index));
    setActive(next);
    virtualizer.scrollToIndex(Math.floor(next / perRow), { align: "auto" });
  };

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (total === 0) return;
    const at = { row: Math.floor(active / perRow), col: active % perRow };
    const pageRows = Math.max(1, Math.floor((scroller.current?.clientHeight ?? 400) / rowHeight));
    const moved = moveCell(event.key, event.ctrlKey || event.metaKey, at, {
      rows: rowCount,
      cols: perRow,
      page: pageRows,
      header: false,
    });
    if (moved) {
      event.preventDefault();
      go(moved.row * perRow + moved.col);
    } else if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      const card = cardAt(active);
      if (card) onOpen(card.object.id);
    }
  };

  return (
    <div
      ref={scroller}
      className="dbme-cards"
      role="grid"
      aria-label="Cards"
      aria-rowcount={rowCount}
      aria-colcount={perRow}
      aria-activedescendant={total > 0 ? cardId(Math.min(active, total - 1)) : undefined}
      tabIndex={0}
      onKeyDown={onKeyDown}
    >
      <div
        role="presentation"
        className="dbme-cards__body"
        style={{ height: `${String(virtualizer.getTotalSize())}px` }}
      >
        {items.map((item) => (
          <div
            key={item.key}
            role="row"
            aria-rowindex={item.index + 1}
            className="dbme-cards__row"
            style={{
              gridTemplateColumns: `repeat(${String(perRow)}, minmax(0, 1fr))`,
              height: `${String(item.size)}px`,
              transform: `translateY(${String(item.start)}px)`,
            }}
          >
            {Array.from({ length: perRow }, (_, col) => {
              const index = item.index * perRow + col;
              if (index >= total) return null;
              return (
                <Card
                  key={index}
                  id={cardId(index)}
                  col={col}
                  summary={cardAt(index)}
                  active={index === active}
                  onSelect={() => {
                    setActive(index);
                    scroller.current?.focus();
                  }}
                  onOpen={onOpen}
                />
              );
            })}
          </div>
        ))}
      </div>
      {count.isSuccess && total === 0 ? <StatusMessage>Nothing to show yet.</StatusMessage> : null}
    </div>
  );
}

function Card({
  id,
  col,
  summary,
  active,
  onSelect,
  onOpen,
}: {
  id: string;
  col: number;
  summary: ObjectSummary_Serialize | undefined;
  active: boolean;
  onSelect: () => void;
  onOpen: (id: string) => void;
}) {
  const cover = useBlobUrl(summary?.cover ?? null);
  const title = summary ? summary.object.title || "Untitled" : "";
  return (
    <div
      id={id}
      role="gridcell"
      aria-colindex={col + 1}
      className={`dbme-card${active ? " is-active" : ""}`}
      onClick={() => {
        if (active && summary) onOpen(summary.object.id);
        else onSelect();
      }}
      onDoubleClick={() => {
        if (summary) onOpen(summary.object.id);
      }}
    >
      <div className="dbme-card__cover">
        {cover.data ? (
          <img src={cover.data} alt="" />
        ) : (
          <span className="dbme-card__initial" aria-hidden="true">
            {title.slice(0, 1)}
          </span>
        )}
      </div>
      <div className="dbme-card__title">{title}</div>
    </div>
  );
}
