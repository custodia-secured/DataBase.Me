// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * The Kanban view (M8, Task 4): the objects of a source in columns, one per choice of a select field
 * and one for no value. Each column is a virtualised list read in pages, so a long column costs no
 * more than a short one.
 *
 * A card moves by drag, by Alt and the arrow keys, or with the "Move to" control, which is also the
 * way on a phone and for anyone who cannot drag (WCAG 2.2, 2.5.7). Moving across columns sets the
 * field and nothing else, so one move is one undo; its place in the new column is its place among
 * the pages. Moving along a column reorders the page, when the cards share a parent.
 */
import {
  MANUAL,
  useFields,
  type FieldRow,
  type Id,
  type ObjectSummary_Serialize,
  type Sort,
} from "@databaseme/client";
import { Button, Dialog, IconButton, Plus, StatusMessage, useMediaQuery } from "@databaseme/ui";
import {
  DndContext,
  DragOverlay,
  getClientRect,
  MouseSensor,
  pointerWithin,
  TouchSensor,
  useDraggable,
  useDroppable,
  useSensor,
  useSensors,
  type CollisionDetection,
  type DragEndEvent,
  type DragStartEvent,
} from "@dnd-kit/core";
import { useVirtualizer } from "@tanstack/react-virtual";
import {
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent,
  type SyntheticEvent,
} from "react";
import { actions } from "../actions";
import { ListAlternative, type ListColumn } from "../framework/ListAlternative";
import { useListingCount, usePagedListing, type Listing } from "../framework/listing";
import { useViewState } from "../framework/useViewState";
import { ViewControls } from "../framework/ViewControls";
import { PAGE } from "../paging";
import type { ViewProps } from "../registry";
import { displayValue } from "../values";
import { boardKey, columnListing, columnsOf, type Column } from "./board";

interface KanbanOptions {
  /** The field the columns come from; the first select field until someone chooses. */
  field: string | null;
}

const DEFAULTS: KanbanOptions = { field: null };

export function KanbanView({ objectId, onOpen }: ViewProps) {
  const state = useViewState(objectId, DEFAULTS);
  const fields = useFields(state.listing.source);
  const groupId = useId();
  const choices = useMemo(
    () => (fields.data ?? []).filter((field) => field.kind === "select"),
    [fields.data],
  );
  const field = choices.find((candidate) => candidate.key === state.options.field) ?? choices[0];
  return (
    <section className="dbme-kanban" aria-label="Kanban">
      <ViewControls state={state} objectId={objectId} renderer="kanban" alternative>
        {choices.length > 1 && field ? (
          <span className="dbme-source">
            <label htmlFor={groupId}>Group by</label>
            <select
              id={groupId}
              className="dbme-input"
              value={field.key}
              onChange={(event) => {
                state.setOptions({ field: event.target.value });
              }}
            >
              {choices.map((choice) => (
                <option key={choice.key} value={choice.key}>
                  {choice.label}
                </option>
              ))}
            </select>
          </span>
        ) : null}
      </ViewControls>
      {!field ? (
        fields.isSuccess ? (
          <StatusMessage>
            A board needs a field with choices to make its columns. These objects have none. Add one
            with New column in the Table.
          </StatusMessage>
        ) : null
      ) : state.asList ? (
        <ListAlternative
          label="Cards"
          listing={state.listing}
          sort={state.sort}
          columns={listColumns(field, fields.data ?? [])}
          onOpen={onOpen}
        />
      ) : (
        <Board
          key={field.key}
          objectId={objectId}
          listing={state.listing}
          sort={state.sort}
          field={field}
          onOpen={onOpen}
        />
      )}
    </section>
  );
}

/** What the board shows by position, written out: the column first, then the other fields. */
function listColumns(field: FieldRow, fields: readonly FieldRow[]): ListColumn[] {
  const others = fields.filter((other) => other.key !== field.key).slice(0, 3);
  return [field, ...others].map((column) => ({
    label: column.label,
    value: (summary) => displayValue(column, summary.object.props_json[column.key]),
  }));
}

type CardData =
  | { kind: "card"; summary: ObjectSummary_Serialize; column: Column }
  | { kind: "column"; column: Column };

/** The card under the pointer wins over the column around it, so a drop can reorder. */
const cardFirst: CollisionDetection = (args) => {
  const hits = pointerWithin(args);
  const card = hits.find(
    (hit) =>
      (hit.data?.droppableContainer as { data: { current?: CardData } } | undefined)?.data.current
        ?.kind === "card",
  );
  return card ? [card] : hits;
};

interface Selection {
  column: number;
  row: number;
}

function Board({
  objectId,
  listing,
  sort,
  field,
  onOpen,
}: {
  objectId: Id;
  listing: Listing;
  sort: Sort;
  field: FieldRow;
  onOpen: (id: Id) => void;
}) {
  const columns = useMemo(() => columnsOf(field), [field]);
  // Nothing is selected until someone chooses: a first press on a card selects it, and a second opens.
  const [selection, setSelection] = useState<Selection | null>(null);
  const [selected, setSelected] = useState<ObjectSummary_Serialize | null>(null);
  const [follow, setFollow] = useState<Id | null>(null);
  const [dragging, setDragging] = useState<ObjectSummary_Serialize | null>(null);
  const [adding, setAdding] = useState<Column | null>(null);
  const [said, setSaid] = useState("");
  const elements = useRef<(HTMLDivElement | null)[]>([]);
  const moveId = useId();
  const sensors = useSensors(
    useSensor(MouseSensor, { activationConstraint: { distance: 6 } }),
    useSensor(TouchSensor, { activationConstraint: { delay: 300, tolerance: 8 } }),
  );
  const canReorder = sort.by === MANUAL.by;

  const titleOf = (summary: ObjectSummary_Serialize) => summary.object.title || "Untitled";

  const moveTo = (summary: ObjectSummary_Serialize, target: Column) => {
    void actions.setProp(summary.object.id, field.key, target.value);
    setSaid(`Moved ${titleOf(summary)} to ${target.label}.`);
  };

  const reorderBefore = (summary: ObjectSummary_Serialize, other: ObjectSummary_Serialize) => {
    if (summary.object.parent_id !== other.object.parent_id) {
      setSaid("These cards are on different pages, so their order cannot change here.");
      return;
    }
    void actions.move(summary.object.id, summary.object.parent_id, { before: other.object.id });
  };

  const onDragStart = (event: DragStartEvent) => {
    const data = event.active.data.current as CardData | undefined;
    if (data?.kind === "card") setDragging(data.summary);
  };

  const onDragEnd = (event: DragEndEvent) => {
    setDragging(null);
    const from = event.active.data.current as CardData | undefined;
    const to = event.over?.data.current as CardData | undefined;
    if (from?.kind !== "card" || !to) return;
    if (to.column.id !== from.column.id) {
      moveTo(from.summary, to.column);
    } else if (
      canReorder &&
      to.kind === "card" &&
      to.summary.object.id !== from.summary.object.id
    ) {
      reorderBefore(from.summary, to.summary);
    }
  };

  const stepColumn = (from: number, step: 1 | -1, row: number) => {
    const column = Math.min(columns.length - 1, Math.max(0, from + step));
    setSelection({ column, row });
    elements.current[column]?.focus();
    elements.current[column]?.scrollIntoView({ block: "nearest", inline: "nearest" });
  };

  const selectedColumn = selection ? columns[selection.column] : undefined;

  return (
    <>
      <div className="dbme-view-bar dbme-kanban__selected">
        {selected ? (
          <>
            <span className="dbme-source">
              <label htmlFor={moveId}>Move “{titleOf(selected)}” to</label>
              <select
                id={moveId}
                className="dbme-input"
                value={selectedColumn?.id ?? ""}
                onChange={(event) => {
                  const target = columns.find((column) => column.id === event.target.value);
                  if (target) {
                    moveTo(selected, target);
                    setFollow(selected.object.id);
                    setSelection({ column: columns.indexOf(target), row: 0 });
                  }
                }}
              >
                {columns.map((column) => (
                  <option key={column.id} value={column.id}>
                    {column.label}
                  </option>
                ))}
              </select>
            </span>
            <Button
              onClick={() => {
                onOpen(selected.object.id);
              }}
            >
              Open
            </Button>
          </>
        ) : null}
        <span className="dbme-visually-hidden" role="status">
          {said}
        </span>
      </div>
      <DndContext
        sensors={sensors}
        collisionDetection={cardFirst}
        // Cards are placed with transforms (virtualised), so measure them with the transforms.
        measuring={{ droppable: { measure: getClientRect } }}
        onDragStart={onDragStart}
        onDragEnd={onDragEnd}
        onDragCancel={() => {
          setDragging(null);
        }}
      >
        <div className="dbme-kanban__columns">
          {columns.map((column, index) => (
            <BoardColumn
              key={column.id}
              column={column}
              fieldKey={field.key}
              listing={listing}
              sort={sort}
              selectedRow={selection?.column === index ? selection.row : null}
              follow={follow}
              canReorder={canReorder}
              register={(element) => {
                elements.current[index] = element;
              }}
              onSelect={(row) => {
                setSelection({ column: index, row });
              }}
              onSelected={setSelected}
              onFollowed={() => {
                setFollow(null);
              }}
              onStep={(step, row) => {
                stepColumn(index, step, row);
              }}
              onCarry={(summary, step) => {
                const target = columns[index + step];
                if (!target) return;
                moveTo(summary, target);
                setFollow(summary.object.id);
                stepColumn(index, step, 0);
              }}
              onReorder={reorderBefore}
              onOpen={onOpen}
              onNew={() => {
                setAdding(column);
              }}
            />
          ))}
        </div>
        <DragOverlay>
          {dragging ? (
            <div className="dbme-kanban__card dbme-kanban__card--drag">{titleOf(dragging)}</div>
          ) : null}
        </DragOverlay>
      </DndContext>
      {adding ? (
        <NewCardDialog
          column={adding}
          objectId={objectId}
          field={field}
          onClose={() => {
            setAdding(null);
          }}
        />
      ) : null}
    </>
  );
}

function BoardColumn({
  column,
  fieldKey,
  listing,
  sort,
  selectedRow,
  follow,
  canReorder,
  register,
  onSelect,
  onSelected,
  onFollowed,
  onStep,
  onCarry,
  onReorder,
  onOpen,
  onNew,
}: {
  column: Column;
  fieldKey: string;
  listing: Listing;
  sort: Sort;
  /** The selected row, when the selection is in this column. */
  selectedRow: number | null;
  /** A card that moved here and should take the selection when it arrives. */
  follow: Id | null;
  canReorder: boolean;
  register: (element: HTMLDivElement | null) => void;
  onSelect: (row: number) => void;
  onSelected: (summary: ObjectSummary_Serialize | null) => void;
  onFollowed: () => void;
  onStep: (step: 1 | -1, row: number) => void;
  onCarry: (summary: ObjectSummary_Serialize, step: 1 | -1) => void;
  onReorder: (summary: ObjectSummary_Serialize, other: ObjectSummary_Serialize) => void;
  onOpen: (id: Id) => void;
  onNew: () => void;
}) {
  const base = useId();
  const headingId = `${base}-heading`;
  const cards = useMemo(
    () => columnListing(listing, fieldKey, column),
    [listing, fieldKey, column],
  );
  const count = useListingCount(cards);
  const total = count.data ?? 0;
  const coarse = useMediaQuery("(pointer: coarse)");
  const cardHeight = coarse ? 72 : 60;
  const scroller = useRef<HTMLDivElement | null>(null);
  const drop = useDroppable({ id: column.id, data: { kind: "column", column } });
  // A press focuses the column before its click lands, and the click decides what it selects. These
  // are mouse events because a touch sends its mousedown, and so its focus, after its pointerup.
  const pressing = useRef(false);

  // eslint-disable-next-line react-hooks/incompatible-library -- TanStack Virtual returns new functions each render; nothing here memoises them
  const virtualizer = useVirtualizer({
    count: total,
    getScrollElement: () => scroller.current,
    estimateSize: () => cardHeight,
    overscan: 6,
  });
  const items = virtualizer.getVirtualItems();
  const first = items[0]?.index ?? 0;
  const last = items.at(-1)?.index ?? 0;
  const rowAt = usePagedListing(cards, sort, first, last);

  const row = selectedRow === null ? null : Math.min(selectedRow, Math.max(0, total - 1));
  const current = row === null || total === 0 ? undefined : rowAt(row);
  const here = selectedRow !== null;

  // The board's "Move to" control follows the selected card.
  useEffect(() => {
    if (here) onSelected(current ?? null);
    // Only the card's identity and title matter: the summary is a new object on every read.
    // eslint-disable-next-line react-hooks/exhaustive-deps -- as the line above says
  }, [here, current?.object.id, current?.object.title, current?.object.parent_id]);

  // A card that moved here takes the selection once it is on a page that has been read.
  useEffect(() => {
    if (follow === null || !here) return;
    for (let index = 0; index < Math.min(total, PAGE); index++) {
      if (rowAt(index)?.object.id === follow) {
        onSelect(index);
        onFollowed();
        virtualizer.scrollToIndex(index, { align: "auto" });
        return;
      }
    }
  });

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const at = row ?? 0;
    const page = Math.max(1, Math.floor((scroller.current?.clientHeight ?? 300) / cardHeight) - 1);
    const key = boardKey(event.key, event.altKey, at, total, page);
    if (!key) return;
    event.preventDefault();
    const summary = rowAt(at);
    switch (key.kind) {
      case "row":
        onSelect(key.row);
        virtualizer.scrollToIndex(key.row, { align: "auto" });
        break;
      case "column":
        onStep(key.step, at);
        break;
      case "card-to-column":
        if (summary) onCarry(summary, key.step);
        break;
      case "card-in-column": {
        const other = rowAt(key.step === -1 ? at - 1 : at + 2);
        if (summary && canReorder) {
          if (other) onReorder(summary, other);
          else if (key.step === 1 && at < total - 1) {
            // To the end of the column: after the last card, which is the place before nothing.
            void actions.move(summary.object.id, summary.object.parent_id, "last");
          }
          const moved = Math.min(total - 1, Math.max(0, at + key.step));
          onSelect(moved);
          virtualizer.scrollToIndex(moved, { align: "auto" });
        }
        break;
      }
      case "open":
        if (summary) onOpen(summary.object.id);
        break;
    }
  };

  const optionId = (index: number) => `${base}-card-${String(index)}`;

  return (
    <div
      ref={drop.setNodeRef}
      className={`dbme-kanban__column${drop.isOver ? " is-over" : ""}`}
      data-column={column.label}
    >
      <div className="dbme-kanban__head">
        <h3 id={headingId} className="dbme-kanban__title">
          {column.label}
        </h3>
        <span className="dbme-muted">{count.isSuccess ? total.toLocaleString() : ""}</span>
        <IconButton label={`New card in ${column.label}`} icon={Plus} onClick={onNew} />
      </div>
      <div
        ref={(element) => {
          scroller.current = element;
          register(element);
        }}
        className="dbme-kanban__cards"
        role="listbox"
        aria-labelledby={headingId}
        aria-activedescendant={row !== null && total > 0 ? optionId(row) : undefined}
        tabIndex={0}
        onKeyDown={onKeyDown}
        onMouseDownCapture={() => {
          pressing.current = true;
        }}
        onMouseUpCapture={() => {
          pressing.current = false;
        }}
        onBlur={() => {
          pressing.current = false;
        }}
        onFocus={() => {
          if (selectedRow === null && !pressing.current) onSelect(0);
        }}
      >
        <div
          role="presentation"
          className="dbme-kanban__body"
          style={{ height: `${String(virtualizer.getTotalSize())}px` }}
        >
          {items.map((item) => {
            const summary = rowAt(item.index);
            const placement = {
              height: `${String(item.size)}px`,
              transform: `translateY(${String(item.start)}px)`,
            };
            if (!summary) {
              return (
                <div
                  key={item.key}
                  role="presentation"
                  className="dbme-kanban__slot"
                  style={placement}
                />
              );
            }
            return (
              <div
                key={item.key}
                role="presentation"
                className="dbme-kanban__slot"
                style={placement}
              >
                <Card
                  id={optionId(item.index)}
                  summary={summary}
                  column={column}
                  position={item.index + 1}
                  total={total}
                  selected={row === item.index}
                  onClick={() => {
                    if (row === item.index) onOpen(summary.object.id);
                    else onSelect(item.index);
                    scroller.current?.focus();
                  }}
                />
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}

function Card({
  id,
  summary,
  column,
  position,
  total,
  selected,
  onClick,
}: {
  id: string;
  summary: ObjectSummary_Serialize;
  column: Column;
  position: number;
  total: number;
  selected: boolean;
  onClick: () => void;
}) {
  const objectId = summary.object.id;
  const data: CardData = { kind: "card", summary, column };
  const drag = useDraggable({ id: objectId, data });
  const drop = useDroppable({ id: `card:${objectId}`, data });
  return (
    <div
      ref={(element) => {
        drag.setNodeRef(element);
        drop.setNodeRef(element);
      }}
      id={id}
      role="option"
      aria-selected={selected}
      aria-setsize={total}
      aria-posinset={position}
      className={`dbme-kanban__card${selected ? " is-active" : ""}${drag.isDragging ? " is-dragging" : ""}${drop.isOver ? " is-over" : ""}`}
      onClick={onClick}
      {...drag.listeners}
    >
      {summary.object.title || <span className="dbme-muted">Untitled</span>}
    </div>
  );
}

function NewCardDialog({
  column,
  objectId,
  field,
  onClose,
}: {
  column: Column;
  objectId: Id;
  field: FieldRow;
  onClose: () => void;
}) {
  const id = useId();
  const [title, setTitle] = useState("");
  const [failed, setFailed] = useState(false);
  const submit = async (event: SyntheticEvent) => {
    event.preventDefault();
    if (title.trim() === "") return;
    // One change, so one undo takes the whole card away: its title, its type and its column.
    const created = await actions.create({
      title: title.trim(),
      parent_id: objectId,
      position: "last",
      type_ids: [field.type_id],
      props: column.value === null ? {} : { [field.key]: column.value },
    });
    if (created === undefined) setFailed(true);
    else onClose();
  };
  return (
    <Dialog open onClose={onClose} title={`New card in ${column.label}`}>
      <form className="dbme-columns__form" onSubmit={(event) => void submit(event)}>
        <label htmlFor={id}>Title</label>
        <input
          id={id}
          className="dbme-input"
          type="text"
          autoComplete="off"
          value={title}
          onChange={(event) => {
            setTitle(event.target.value);
            setFailed(false);
          }}
        />
        {failed ? <StatusMessage tone="error">The card was not added.</StatusMessage> : null}
        <div className="dbme-dialog__actions">
          <Button onClick={onClose}>Cancel</Button>
          <Button variant="primary" type="submit" disabled={title.trim() === ""}>
            Add card
          </Button>
        </div>
      </form>
    </Dialog>
  );
}
