// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * The Table view: the objects of a source as rows and their fields as columns. Rows are
 * virtualised (TanStack Virtual) and read in pages, so 100,000 rows scroll at 60 fps; TanStack
 * Table models the columns. The grid is one tab stop: the arrow keys move between cells
 * (`aria-activedescendant`), Enter or typing edits, Escape cancels, Enter on the first column
 * opens the row's object.
 */
import {
  MANUAL,
  useCount,
  useFields,
  type FieldRow,
  type ObjectSummary_Serialize,
  type Sort,
  type Source,
  type Value,
} from "@databaseme/client";
import {
  ArrowDown,
  ArrowUp,
  Button,
  DocumentIcon,
  Plus,
  StatusMessage,
  useMediaQuery,
} from "@databaseme/ui";
import { createColumnHelper, flexRender, tableFeatures, useTable } from "@tanstack/react-table";
import { useVirtualizer } from "@tanstack/react-virtual";
import { useEffect, useId, useMemo, useRef, useState, type KeyboardEvent } from "react";
import { actions } from "../actions";
import { FieldEditor } from "../fields";
import { isTyping, moveCell, type Cell } from "../grid";
import { childrenOf, usePagedObjects } from "../paging";
import type { ViewProps } from "../registry";
import { SourcePicker } from "../SourcePicker";
import { displayValue, isEditable } from "../values";

interface RowData {
  index: number;
  summary: ObjectSummary_Serialize | undefined;
}

type ColumnKind = { kind: "open" } | { kind: "title" } | { kind: "field"; field: FieldRow };

const features = tableFeatures({ columnMeta: {} as { column: ColumnKind } });
const helper = createColumnHelper<typeof features, RowData>();

const TITLE_FIELD = { kind: "text", label: "Title", options_json: null } as const;

function sortOf(column: ColumnKind): Sort | null {
  if (column.kind === "title") return { by: "title", field: null, descending: false };
  if (column.kind === "field") return { by: "field", field: column.field.key, descending: false };
  return null;
}

function sameColumn(sort: Sort, column: ColumnKind): boolean {
  if (column.kind === "title") return sort.by === "title";
  if (column.kind === "field") return sort.by === "field" && sort.field === column.field.key;
  return false;
}

/** Ascending, then descending, then the manual order again. */
function nextSort(sort: Sort, column: ColumnKind): Sort {
  const base = sortOf(column);
  if (!base) return sort;
  if (!sameColumn(sort, column)) return base;
  return sort.descending ? MANUAL : { ...base, descending: true };
}

export function TableView({ objectId, onOpen }: ViewProps) {
  const [source, setSource] = useState<Source>(() => childrenOf(objectId));
  const [sort, setSort] = useState<Sort>(MANUAL);
  // A new object resets the table to its children.
  const [shownFor, setShownFor] = useState(objectId);
  if (shownFor !== objectId) {
    setShownFor(objectId);
    setSource(childrenOf(objectId));
    setSort(MANUAL);
  }
  return (
    <section className="dbme-table" aria-label="Table">
      <div className="dbme-view-bar">
        <SourcePicker objectId={objectId} source={source} onChange={setSource} />
      </div>
      <Grid objectId={objectId} source={source} sort={sort} onSort={setSort} onOpen={onOpen} />
    </section>
  );
}

interface GridProps {
  objectId: string;
  source: Source;
  sort: Sort;
  onSort: (sort: Sort) => void;
  onOpen: (id: string) => void;
}

function Grid({ objectId, source, sort, onSort, onOpen }: GridProps) {
  const gridId = useId();
  const count = useCount(source);
  const fields = useFields(source);
  const coarse = useMediaQuery("(pointer: coarse)");
  const rowHeight = coarse ? 44 : 36;
  const scroller = useRef<HTMLDivElement>(null);
  const [active, setActive] = useState<Cell>({ row: 0, col: 1 });
  const [editing, setEditing] = useState<{ cell: Cell; text?: string } | null>(null);
  const total = count.data ?? 0;

  // eslint-disable-next-line react-hooks/incompatible-library -- TanStack Virtual returns new functions each render; nothing here memoises them
  const virtualizer = useVirtualizer({
    count: total,
    getScrollElement: () => scroller.current,
    estimateSize: () => rowHeight,
    overscan: 12,
  });
  const items = virtualizer.getVirtualItems();
  const first = items[0]?.index ?? 0;
  const last = items.at(-1)?.index ?? 0;
  const rowAt = usePagedObjects(source, sort, first, last);

  const columnKinds = useMemo<ColumnKind[]>(
    () => [
      { kind: "open" },
      { kind: "title" },
      ...(fields.data ?? []).map((field) => ({ kind: "field" as const, field })),
    ],
    [fields.data],
  );

  const columns = useMemo(
    () =>
      columnKinds.map((column) =>
        helper.display({
          id: column.kind === "field" ? `field:${column.field.key}` : column.kind,
          header:
            column.kind === "open"
              ? "Open"
              : column.kind === "title"
                ? "Title"
                : column.field.label,
          meta: { column },
        }),
      ),
    [columnKinds],
  );

  const data = useMemo<RowData[]>(
    () => items.map((item) => ({ index: item.index, summary: rowAt(item.index) })),
    [items, rowAt],
  );
  const table = useTable({
    features,
    columns,
    data,
    getRowId: (row) => String(row.index),
  });
  const cols = columnKinds.length;
  const template = `3rem minmax(12rem, 2fr)${cols > 2 ? ` repeat(${String(cols - 2)}, minmax(9rem, 1fr))` : ""}`;

  // Keep the active cell on the grid when the grid shrinks.
  useEffect(() => {
    if (active.row >= total && total > 0) setActive({ row: total - 1, col: active.col });
  }, [total, active]);

  const cellId = (cell: Cell) => `${gridId}-r${String(cell.row)}-c${String(cell.col)}`;
  const focusGrid = () => scroller.current?.focus();

  const goTo = (cell: Cell) => {
    setActive(cell);
    if (cell.row >= 0) virtualizer.scrollToIndex(cell.row, { align: "auto" });
  };

  const valueOf = (
    summary: ObjectSummary_Serialize | undefined,
    column: ColumnKind,
  ): Value | undefined => {
    if (!summary) return undefined;
    if (column.kind === "title") return summary.object.title;
    if (column.kind === "field") return summary.object.props_json[column.field.key];
    return undefined;
  };

  const activate = (cell: Cell, text?: string) => {
    const column = columnKinds[cell.col];
    if (!column) return;
    if (cell.row < 0) {
      onSort(nextSort(sort, column));
      return;
    }
    const summary = rowAt(cell.row);
    if (!summary) return;
    if (column.kind === "open") {
      onOpen(summary.object.id);
    } else if (column.kind === "field" && column.field.kind === "boolean") {
      if (text === undefined || text === " ") {
        void actions.setProp(
          summary.object.id,
          column.field.key,
          valueOf(summary, column) !== true,
        );
      }
    } else if (column.kind === "title" || isEditable(column.field.kind)) {
      setEditing(text === undefined ? { cell } : { cell, text });
    }
  };

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (editing) return;
    const ctrl = event.ctrlKey || event.metaKey;
    const moved = moveCell(event.key, ctrl, active, {
      rows: total,
      cols,
      page: Math.max(1, Math.floor((scroller.current?.clientHeight ?? 400) / rowHeight) - 1),
      header: true,
    });
    if (moved) {
      event.preventDefault();
      goTo(moved);
    } else if (event.key === "Enter" || event.key === "F2") {
      event.preventDefault();
      if (ctrl && active.row >= 0) {
        const summary = rowAt(active.row);
        if (summary) onOpen(summary.object.id);
      } else {
        activate(active);
      }
    } else if (event.key === " ") {
      event.preventDefault();
      activate(active, " ");
    } else if (isTyping(event) && active.row >= 0) {
      event.preventDefault();
      activate(active, event.key);
    }
  };

  const commit = (summary: ObjectSummary_Serialize, column: ColumnKind, value: Value | null) => {
    if (column.kind === "title")
      void actions.rename(summary.object.id, typeof value === "string" ? value : "");
    else if (column.kind === "field")
      void actions.setProp(summary.object.id, column.field.key, value);
  };

  const finishEdit = (key: "Enter" | "Tab" | "ShiftTab" | "Escape" | "blur") => {
    const at = editing?.cell;
    setEditing(null);
    if (key === "blur" || !at) return;
    focusGrid();
    if (key === "Enter") goTo({ row: Math.min(total - 1, at.row + 1), col: at.col });
    if (key === "Tab") goTo({ row: at.row, col: Math.min(cols - 1, at.col + 1) });
    if (key === "ShiftTab") goTo({ row: at.row, col: Math.max(0, at.col - 1) });
  };

  const addRow = async () => {
    const id = await actions.create({
      title: "",
      parent_id: objectId,
      position: "last",
      ...(source.kind === "type" ? { type_ids: [source.type_id] } : {}),
    });
    if (id === undefined) return;
    onSort(MANUAL);
    const row = total;
    goTo({ row, col: 1 });
    setEditing({ cell: { row, col: 1 } });
  };

  const headers = table.getHeaderGroups()[0]?.headers ?? [];
  const rows = table.getRowModel().rows;
  const activeId = active.row < total || active.row < 0 ? cellId(active) : undefined;

  return (
    <>
      <div className="dbme-view-bar">
        <Button onClick={() => void addRow()}>
          <Plus aria-hidden="true" size={16} /> New row
        </Button>
        <span className="dbme-muted" aria-live="polite">
          {count.isSuccess ? `${total.toLocaleString()} ${total === 1 ? "row" : "rows"}` : ""}
        </span>
      </div>
      <div
        ref={scroller}
        className="dbme-grid"
        role="grid"
        aria-label="Rows"
        aria-rowcount={total + 1}
        aria-colcount={cols}
        aria-activedescendant={editing ? undefined : activeId}
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
            {headers.map((header, col) => {
              const column = header.column.columnDef.meta?.column ?? { kind: "open" as const };
              const sorted = sameColumn(sort, column)
                ? sort.descending
                  ? "descending"
                  : "ascending"
                : undefined;
              const Arrow = sorted === "descending" ? ArrowDown : ArrowUp;
              return (
                <div
                  key={header.id}
                  id={cellId({ row: -1, col })}
                  role="columnheader"
                  aria-colindex={col + 1}
                  aria-sort={sorted ?? "none"}
                  className={`dbme-grid__cell dbme-grid__header${active.row === -1 && active.col === col ? " is-active" : ""}`}
                  onClick={() => {
                    goTo({ row: -1, col });
                    activate({ row: -1, col });
                  }}
                >
                  {column.kind === "open" ? (
                    <span className="dbme-visually-hidden">Open</span>
                  ) : (
                    flexRender(header.column.columnDef.header, header.getContext())
                  )}
                  {sorted ? <Arrow aria-hidden="true" size={14} /> : null}
                </div>
              );
            })}
          </div>
        </div>
        <div
          role="rowgroup"
          className="dbme-grid__body"
          style={{ height: `${String(virtualizer.getTotalSize())}px` }}
        >
          {items.map((item, i) => {
            const row = rows[i];
            const summary = row?.original.summary;
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
                {(row?.getAllCells() ?? []).map((cell, col) => {
                  const column = cell.column.columnDef.meta?.column ?? { kind: "open" as const };
                  const here = { row: item.index, col };
                  const isActive = active.row === item.index && active.col === col;
                  const isEditing = editing?.cell.row === item.index && editing.cell.col === col;
                  const value = valueOf(summary, column);
                  return (
                    <div
                      key={cell.id}
                      id={cellId(here)}
                      role="gridcell"
                      aria-colindex={col + 1}
                      className={`dbme-grid__cell${isActive ? " is-active" : ""}`}
                      onClick={() => {
                        if (isActive) activate(here);
                        else goTo(here);
                        if (!isEditing) focusGrid();
                      }}
                    >
                      {!summary ? (
                        <span className="dbme-grid__placeholder" aria-hidden="true" />
                      ) : column.kind === "open" ? (
                        <span
                          className="dbme-grid__open"
                          title={`Open ${summary.object.title || "Untitled"}`}
                        >
                          <DocumentIcon aria-hidden="true" size={16} />
                          <span className="dbme-visually-hidden">
                            Open {summary.object.title || "Untitled"}
                          </span>
                        </span>
                      ) : isEditing ? (
                        <FieldEditor
                          field={column.kind === "title" ? TITLE_FIELD : column.field}
                          value={value}
                          {...(editing.text === undefined ? {} : { initialText: editing.text })}
                          onCommit={(next) => {
                            commit(summary, column, next);
                          }}
                          onCancel={() => undefined}
                          onDone={finishEdit}
                        />
                      ) : column.kind === "field" && column.field.kind === "boolean" ? (
                        <span
                          className="dbme-grid__check"
                          aria-label={value === true ? "Yes" : "No"}
                        >
                          {value === true ? "✓" : ""}
                        </span>
                      ) : (
                        <span className="dbme-grid__text">
                          {column.kind === "title"
                            ? summary.object.title || <span className="dbme-muted">Untitled</span>
                            : displayValue(column.field, value)}
                        </span>
                      )}
                    </div>
                  );
                })}
              </div>
            );
          })}
        </div>
        {count.isSuccess && total === 0 ? (
          <StatusMessage>No rows yet. Use New row to add one.</StatusMessage>
        ) : null}
      </div>
    </>
  );
}
