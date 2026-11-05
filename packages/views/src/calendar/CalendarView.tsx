// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * The Calendar view (M8, Task 5): objects on the days of a month or a week, by the event date or any
 * date field. The store counts the days on screen, and only those with something on them are read,
 * each to a bound with "+N more" for the rest, so a busy vault costs what the screen shows.
 *
 * An item moves by drag, by Alt and the arrow keys, or with the Move to control, which is also the way
 * on a phone, where the grid gives way to an agenda. Every move is one change, so one undo.
 */
import {
  useFields,
  useQueryCount,
  useSummary,
  type FieldRow,
  type Id,
  type ObjectSummary_Serialize,
} from "@databaseme/client";
import {
  Button,
  ChevronLeft,
  ChevronRight,
  Dialog,
  IconButton,
  StatusMessage,
  useMediaQuery,
} from "@databaseme/ui";
import {
  DndContext,
  DragOverlay,
  MouseSensor,
  pointerWithin,
  TouchSensor,
  useDraggable,
  useDroppable,
  useSensor,
  useSensors,
  type DragEndEvent,
  type DragStartEvent,
} from "@dnd-kit/core";
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
import { byField, listingQuery, narrowed, type Listing } from "../framework/listing";
import { dayBuckets, useBuckets, type BucketReads } from "../framework/useBuckets";
import { useViewState } from "../framework/useViewState";
import { ViewControls } from "../framework/ViewControls";
import type { ViewProps } from "../registry";
import { displayValue } from "../values";
import {
  addDays,
  carryKey,
  chosenDate,
  dateChoices,
  dateWrite,
  dayKey,
  dayOf,
  EVENT,
  isDay,
  longDay,
  monthDays,
  rangeConditions,
  stepped,
  titleOf,
  todayOf,
  weekdayShort,
  weeksOf,
  type CalendarMode,
  type DateChoice,
  type Day,
} from "./dates";

interface CalendarOptions {
  /** What the days come from; the first date field until someone chooses. */
  field: string | null;
  mode: CalendarMode;
}

const DEFAULTS: CalendarOptions = { field: null, mode: "month" };

/** How many items a day shows before "+N more". */
const BOUND: Record<CalendarMode, number> = { month: 3, week: 12 };
const AGENDA_BOUND = 5;

const titleOfItem = (summary: ObjectSummary_Serialize) => summary.object.title || "Untitled";

const onDay = (listing: Listing, key: string, day: Day) =>
  narrowed(listing, ...rangeConditions(key, day, addDays(day, 1)));

export function CalendarView({ objectId, onOpen }: ViewProps) {
  const state = useViewState(objectId, DEFAULTS);
  const fields = useFields(state.listing.source);
  const choices = useMemo(() => dateChoices(fields.data ?? []), [fields.data]);
  const date = chosenDate(choices, state.options.field);
  const phone = useMediaQuery("(max-width: 767px)");
  const mode: CalendarMode = phone ? "month" : state.options.mode;
  const byId = useId();
  const periodId = useId();
  return (
    <section className="dbme-calendar" aria-label="Calendar">
      <ViewControls state={state} objectId={objectId} renderer="calendar" alternative>
        {choices.length > 1 ? (
          <span className="dbme-source">
            <label htmlFor={byId}>By</label>
            <select
              id={byId}
              className="dbme-input"
              value={date.key}
              onChange={(event) => {
                state.setOptions({ ...state.options, field: event.target.value });
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
        {phone ? null : (
          <span className="dbme-source">
            <label htmlFor={periodId}>Period</label>
            <select
              id={periodId}
              className="dbme-input"
              value={mode}
              onChange={(event) => {
                state.setOptions({
                  ...state.options,
                  mode: event.target.value === "week" ? "week" : "month",
                });
              }}
            >
              <option value="month">Month</option>
              <option value="week">Week</option>
            </select>
          </span>
        )}
      </ViewControls>
      {/* What it goes by is not known until the fields are: starting on the event date and then
          changing would throw away the month and the focus. */}
      {fields.isSuccess ? (
        <Calendar
          key={date.key}
          objectId={objectId}
          listing={state.listing}
          date={date}
          mode={mode}
          phone={phone}
          asList={state.asList}
          fields={fields.data}
          onOpen={onOpen}
        />
      ) : null}
    </section>
  );
}

type Opened = { kind: "day"; day: Day } | { kind: "undated" } | { kind: "new" };

function Calendar({
  objectId,
  listing,
  date,
  mode,
  phone,
  asList,
  fields,
  onOpen,
}: {
  objectId: Id;
  listing: Listing;
  date: DateChoice;
  mode: CalendarMode;
  phone: boolean;
  asList: boolean;
  fields: readonly FieldRow[];
  onOpen: (id: Id) => void;
}) {
  const today = todayOf();
  const [anchor, setAnchor] = useState(today);
  const weeks = useMemo(() => weeksOf(mode, anchor), [mode, anchor]);
  const days = useMemo(() => (phone ? monthDays(anchor) : weeks.flat()), [phone, weeks, anchor]);
  const buckets = useMemo(() => dayBuckets(days), [days]);
  const read = useBuckets(
    listing,
    date.key,
    "day",
    buckets,
    phone ? AGENDA_BOUND : BOUND[mode],
    byField(date.key),
  );
  const undated = useQueryCount(
    listingQuery(narrowed(listing, { field: date.key, op: "is_empty", value: null })),
  );

  const [active, setActive] = useState(today);
  const current = days.includes(active) ? active : (days[0] ?? anchor);
  const [selectedId, setSelectedId] = useState<Id | null>(null);
  const selected = useSummary(selectedId).data ?? null;
  const [follow, setFollow] = useState<{ id: Id; day: Day } | null>(null);
  const [dragging, setDragging] = useState<ObjectSummary_Serialize | null>(null);
  const [opened, setOpened] = useState<Opened | null>(null);
  const [said, setSaid] = useState("");
  const sensors = useSensors(
    useSensor(MouseSensor, { activationConstraint: { distance: 6 } }),
    useSensor(TouchSensor, { activationConstraint: { delay: 300, tolerance: 8 } }),
  );

  const cells = useRef(new Map<Day, HTMLElement>());
  const focusing = useRef<Day | null>(null);
  useEffect(() => {
    const cell = focusing.current === null ? undefined : cells.current.get(focusing.current);
    if (cell) {
      cell.focus();
      focusing.current = null;
    }
  });

  /** Moves the grid's focus to a day, turning the view to it if asked or if it is not on screen. */
  const goTo = (day: Day, turn = false) => {
    focusing.current = day;
    setActive(day);
    if (turn || !days.includes(day)) setAnchor(day);
  };

  const moveItem = (summary: ObjectSummary_Serialize, target: Day) => {
    if (dayOf(summary, date.key) === target) return;
    const write = dateWrite(summary, date.key, target);
    const { id } = summary.object;
    void (write.kind === "event"
      ? actions.setEvent(id, write.start, write.end)
      : actions.setProp(id, write.key, write.value));
    setSaid(`Moved ${titleOfItem(summary)} to ${longDay(target)}.`);
  };

  const press = (summary: ObjectSummary_Serialize) => {
    if (selectedId === summary.object.id) onOpen(summary.object.id);
    else setSelectedId(summary.object.id);
  };

  const onDragStart = (event: DragStartEvent) => {
    const summary = event.active.data.current?.summary as ObjectSummary_Serialize | undefined;
    if (summary) setDragging(summary);
  };

  const onDragEnd = (event: DragEndEvent) => {
    setDragging(null);
    const summary = event.active.data.current?.summary as ObjectSummary_Serialize | undefined;
    const target = event.over?.id;
    if (summary && typeof target === "string") moveItem(summary, target);
  };

  const title = titleOf(mode, anchor);
  const missing = undated.data ?? 0;
  const unit = mode === "month" ? "month" : "week";
  const selectedDay = selected ? dayOf(selected, date.key) : null;

  return (
    <>
      <div className="dbme-view-bar dbme-calendar__nav">
        <IconButton
          label={`Previous ${unit}`}
          icon={ChevronLeft}
          onClick={() => {
            setAnchor(stepped(mode, anchor, -1));
          }}
        />
        <Button
          onClick={() => {
            setAnchor(today);
            setActive(today);
          }}
        >
          Today
        </Button>
        <IconButton
          label={`Next ${unit}`}
          icon={ChevronRight}
          onClick={() => {
            setAnchor(stepped(mode, anchor, 1));
          }}
        />
        <h3 className="dbme-calendar__title" aria-live="polite">
          {title}
        </h3>
        <Button
          onClick={() => {
            setOpened({ kind: "new" });
          }}
        >
          New item
        </Button>
        {missing > 0 ? (
          <Button
            onClick={() => {
              setOpened({ kind: "undated" });
            }}
          >
            {missing.toLocaleString()} with no date
          </Button>
        ) : null}
      </div>
      <div className="dbme-view-bar dbme-calendar__selected">
        {selected ? (
          <>
            <MoveControl
              key={`${selected.object.id}:${selectedDay ?? ""}`}
              summary={selected}
              day={selectedDay}
              onMove={(target) => {
                moveItem(selected, target);
                setActive(target);
                if (!days.includes(target)) setAnchor(target);
              }}
            />
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
      {asList ? (
        <ListAlternative
          label="Dated items"
          listing={narrowed(
            listing,
            ...rangeConditions(date.key, days[0] ?? anchor, addDays(days.at(-1) ?? anchor, 1)),
          )}
          sort={byField(date.key)}
          columns={listColumns(date, fields)}
          onOpen={onOpen}
        />
      ) : phone ? (
        <Agenda
          title={title}
          read={read}
          selectedId={selectedId}
          onPress={press}
          onMore={(day) => {
            setOpened({ kind: "day", day });
          }}
        />
      ) : (
        <DndContext
          sensors={sensors}
          collisionDetection={pointerWithin}
          onDragStart={onDragStart}
          onDragEnd={onDragEnd}
          onDragCancel={() => {
            setDragging(null);
          }}
        >
          <div role="grid" aria-label={title} className={`dbme-calendar__grid is-${mode}`}>
            <div role="row" className="dbme-calendar__week dbme-calendar__head">
              {(weeks[0] ?? []).map((day) => (
                <div key={day} role="columnheader" className="dbme-calendar__weekday">
                  {weekdayShort(day)}
                </div>
              ))}
            </div>
            {weeks.map((week) => (
              <div key={week[0]} role="row" className="dbme-calendar__week">
                {week.map((day) => (
                  <DayCell
                    key={day}
                    day={day}
                    mode={mode}
                    inMonth={day.slice(0, 7) === anchor.slice(0, 7)}
                    isToday={day === today}
                    active={day === current}
                    count={read.countOf(day)}
                    items={read.itemsOf(day)}
                    selectedId={selectedId}
                    follow={follow?.day === day ? follow.id : null}
                    register={(element) => {
                      if (element) cells.current.set(day, element);
                      else cells.current.delete(day);
                    }}
                    onFocused={() => {
                      setActive(day);
                    }}
                    onGo={goTo}
                    onPress={press}
                    onOpen={onOpen}
                    onFollowed={() => {
                      setFollow(null);
                    }}
                    onCarry={(summary, by) => {
                      const target = addDays(day, by);
                      moveItem(summary, target);
                      setFollow({ id: summary.object.id, day: target });
                      goTo(target);
                    }}
                    onMore={() => {
                      setOpened({ kind: "day", day });
                    }}
                  />
                ))}
              </div>
            ))}
          </div>
          <DragOverlay>
            {dragging ? (
              <div className="dbme-calendar__item dbme-calendar__item--drag">
                {titleOfItem(dragging)}
              </div>
            ) : null}
          </DragOverlay>
        </DndContext>
      )}
      {opened?.kind === "new" ? (
        <NewItemDialog
          objectId={objectId}
          listing={listing}
          date={date}
          day={current}
          onClose={() => {
            setOpened(null);
          }}
        />
      ) : opened ? (
        <PickDialog
          title={opened.kind === "day" ? longDay(opened.day) : `No ${date.label.toLowerCase()}`}
          listing={
            opened.kind === "day"
              ? onDay(listing, date.key, opened.day)
              : narrowed(listing, { field: date.key, op: "is_empty", value: null })
          }
          onPick={(id) => {
            setSelectedId(id);
            setOpened(null);
          }}
          onClose={() => {
            setOpened(null);
          }}
        />
      ) : null}
    </>
  );
}

/** What the grid shows by position, written out: the date first, then the other fields. */
function listColumns(date: DateChoice, fields: readonly FieldRow[]): ListColumn[] {
  const seen = new Set([date.key]);
  const others = fields.filter((field) => {
    if (seen.has(field.key)) return false;
    seen.add(field.key);
    return true;
  });
  return [
    {
      label: date.label,
      value: (summary) => {
        const day = dayOf(summary, date.key);
        return day ? longDay(day) : "";
      },
    },
    ...others.slice(0, 3).map((field) => ({
      label: field.label,
      value: (summary: ObjectSummary_Serialize) =>
        displayValue(field, summary.object.props_json[field.key]),
    })),
  ];
}

/**
 * The date of the selected item, for anyone who cannot drag (WCAG 2.2, 2.5.7). A button commits it, as
 * typing a date passes through other valid dates on the way, and one move is one undo.
 */
function MoveControl({
  summary,
  day,
  onMove,
}: {
  summary: ObjectSummary_Serialize;
  day: Day | null;
  onMove: (target: Day) => void;
}) {
  const id = useId();
  const [text, setText] = useState(day ?? "");
  const name = titleOfItem(summary);
  return (
    <form
      className="dbme-source"
      onSubmit={(event) => {
        event.preventDefault();
        if (isDay(text)) onMove(text);
      }}
    >
      <label htmlFor={id}>{day ? `Move “${name}” to` : `Date “${name}”`}</label>
      <input
        id={id}
        className="dbme-input"
        type="date"
        value={text}
        onChange={(event) => {
          setText(event.target.value);
        }}
      />
      <Button type="submit" disabled={!isDay(text) || text === day}>
        {day ? "Move" : "Set date"}
      </Button>
    </form>
  );
}

function DayCell({
  day,
  mode,
  inMonth,
  isToday,
  active,
  count,
  items,
  selectedId,
  follow,
  register,
  onFocused,
  onGo,
  onPress,
  onOpen,
  onFollowed,
  onCarry,
  onMore,
}: {
  day: Day;
  mode: CalendarMode;
  inMonth: boolean;
  isToday: boolean;
  active: boolean;
  count: number;
  /** Undefined until they have been read. */
  items: readonly ObjectSummary_Serialize[] | undefined;
  selectedId: Id | null;
  follow: Id | null;
  register: (element: HTMLElement | null) => void;
  onFocused: () => void;
  onGo: (day: Day, turn: boolean) => void;
  onPress: (summary: ObjectSummary_Serialize) => void;
  onOpen: (id: Id) => void;
  onFollowed: () => void;
  onCarry: (summary: ObjectSummary_Serialize, by: number) => void;
  onMore: () => void;
}) {
  const drop = useDroppable({ id: day });
  const element = useRef<HTMLDivElement | null>(null);
  const more = items ? count - items.length : 0;
  const classes = [
    "dbme-calendar__day",
    inMonth || mode === "week" ? "" : "is-outside",
    isToday ? "is-today" : "",
    drop.isOver ? "is-over" : "",
  ]
    .filter(Boolean)
    .join(" ");

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    // The items in a cell have keys of their own.
    if (event.target !== event.currentTarget) return;
    if (event.key === "Enter") {
      event.preventDefault();
      event.currentTarget.querySelector<HTMLElement>("[data-cell-item]")?.focus();
      return;
    }
    const to = dayKey(event.key, day, mode);
    if (to !== null) {
      event.preventDefault();
      onGo(to, event.key === "PageUp" || event.key === "PageDown");
    }
  };

  return (
    <div
      ref={(node) => {
        drop.setNodeRef(node);
        element.current = node;
        register(node);
      }}
      role="gridcell"
      tabIndex={active ? 0 : -1}
      aria-current={isToday ? "date" : undefined}
      data-day={day}
      className={classes}
      onFocus={onFocused}
      onKeyDown={onKeyDown}
    >
      <span className="dbme-calendar__date">
        <span aria-hidden="true">{Number(day.slice(8))}</span>
        <span className="dbme-visually-hidden">
          {longDay(day)}, {count === 1 ? "1 item" : `${count.toLocaleString()} items`}
        </span>
      </span>
      {(items ?? []).map((summary) => (
        <DayItem
          key={summary.object.id}
          summary={summary}
          selected={selectedId === summary.object.id}
          follow={follow}
          cell={element}
          onPress={onPress}
          onOpen={onOpen}
          onFollowed={onFollowed}
          onCarry={onCarry}
        />
      ))}
      {more > 0 ? (
        <button
          type="button"
          tabIndex={-1}
          data-cell-item
          className="dbme-calendar__more"
          onClick={onMore}
          onKeyDown={(event) => {
            if (event.key === "Escape") element.current?.focus();
            else stepAmongItems(event);
          }}
        >
          +{more.toLocaleString()} more
        </button>
      ) : null}
    </div>
  );
}

/** Up and down along the items of a cell. */
function stepAmongItems(event: KeyboardEvent<HTMLElement>): void {
  if (event.key !== "ArrowUp" && event.key !== "ArrowDown") return;
  event.preventDefault();
  const cell = event.currentTarget.closest("[data-day]");
  const all = [...(cell?.querySelectorAll<HTMLElement>("[data-cell-item]") ?? [])];
  const at = all.indexOf(event.currentTarget);
  all[at + (event.key === "ArrowUp" ? -1 : 1)]?.focus();
}

function DayItem({
  summary,
  selected,
  follow,
  cell,
  onPress,
  onOpen,
  onFollowed,
  onCarry,
}: {
  summary: ObjectSummary_Serialize;
  selected: boolean;
  follow: Id | null;
  cell: { readonly current: HTMLElement | null };
  onPress: (summary: ObjectSummary_Serialize) => void;
  onOpen: (id: Id) => void;
  onFollowed: () => void;
  onCarry: (summary: ObjectSummary_Serialize, by: number) => void;
}) {
  const { id } = summary.object;
  const drag = useDraggable({ id, data: { summary } });
  const element = useRef<HTMLButtonElement | null>(null);

  // An item that was carried here takes the focus back once it has arrived.
  useEffect(() => {
    if (follow === id) {
      element.current?.focus();
      onFollowed();
    }
  });

  const onKeyDown = (event: KeyboardEvent<HTMLButtonElement>) => {
    if (event.altKey) {
      const by = carryKey(event.key);
      if (by !== null) {
        event.preventDefault();
        onCarry(summary, by);
      }
    } else if (event.key === "Escape") {
      cell.current?.focus();
    } else if (event.key === "Enter") {
      event.preventDefault();
      onOpen(id);
    } else {
      stepAmongItems(event);
    }
  };

  return (
    <button
      ref={(node) => {
        drag.setNodeRef(node);
        element.current = node;
      }}
      type="button"
      tabIndex={-1}
      data-cell-item
      aria-pressed={selected}
      className={`dbme-calendar__item${selected ? " is-active" : ""}${drag.isDragging ? " is-dragging" : ""}`}
      onClick={() => {
        onPress(summary);
      }}
      onKeyDown={onKeyDown}
      {...drag.listeners}
    >
      {titleOfItem(summary)}
    </button>
  );
}

/** The month as a list of the days with something on them: what fits a phone. */
function Agenda({
  title,
  read,
  selectedId,
  onPress,
  onMore,
}: {
  title: string;
  read: BucketReads;
  selectedId: Id | null;
  onPress: (summary: ObjectSummary_Serialize) => void;
  onMore: (day: Day) => void;
}) {
  if (read.ready && read.filled.length === 0) {
    return <StatusMessage>Nothing is dated in {title}.</StatusMessage>;
  }
  return (
    <ul className="dbme-calendar__agenda" aria-label={title}>
      {read.filled.map(({ key: day }) => {
        const items = read.itemsOf(day);
        const more = items ? read.countOf(day) - items.length : 0;
        return (
          <li key={day} data-day={day} className="dbme-calendar__agenda-day">
            <h4 className="dbme-calendar__agenda-date">{longDay(day)}</h4>
            {(items ?? []).map((summary) => (
              <button
                key={summary.object.id}
                type="button"
                aria-pressed={selectedId === summary.object.id}
                className={`dbme-calendar__item${selectedId === summary.object.id ? " is-active" : ""}`}
                onClick={() => {
                  onPress(summary);
                }}
              >
                {titleOfItem(summary)}
              </button>
            ))}
            {more > 0 ? (
              <button
                type="button"
                className="dbme-calendar__more"
                onClick={() => {
                  onMore(day);
                }}
              >
                +{more.toLocaleString()} more
              </button>
            ) : null}
          </li>
        );
      })}
    </ul>
  );
}

/** A list of what does not fit the grid. Picking one selects it, so the Move to control can reach it. */
function PickDialog({
  title,
  listing,
  onPick,
  onClose,
}: {
  title: string;
  listing: Listing;
  onPick: (id: Id) => void;
  onClose: () => void;
}) {
  return (
    <Dialog open onClose={onClose} title={title}>
      <div className="dbme-calendar__list">
        <ListAlternative
          label={title}
          listing={listing}
          sort={{ by: "title", descending: false }}
          columns={[]}
          onOpen={onPick}
        />
      </div>
      <div className="dbme-dialog__actions">
        <Button onClick={onClose}>Close</Button>
      </div>
    </Dialog>
  );
}

function NewItemDialog({
  objectId,
  listing,
  date,
  day,
  onClose,
}: {
  objectId: Id;
  listing: Listing;
  date: DateChoice;
  day: Day;
  onClose: () => void;
}) {
  const titleId = useId();
  const dayId = useId();
  const [title, setTitle] = useState("");
  const [text, setText] = useState(day);
  const [failed, setFailed] = useState(false);
  const { source } = listing;
  const ready = title.trim() !== "" && isDay(text);
  const submit = async (event: SyntheticEvent) => {
    event.preventDefault();
    if (!ready) return;
    const typeId = date.typeId ?? (source.kind === "type" ? source.type_id : null);
    // One change, so one undo takes the whole item away: its title, its type and its date.
    const created = await actions.create({
      title: title.trim(),
      parent_id: source.kind === "children" ? (source.parent ?? null) : objectId,
      position: "last",
      type_ids: typeId === null ? [] : [typeId],
      props: date.key === EVENT ? {} : { [date.key]: text },
      event_start: date.key === EVENT ? Date.parse(`${text}T00:00:00Z`) : null,
    });
    if (created === undefined) setFailed(true);
    else onClose();
  };
  return (
    <Dialog open onClose={onClose} title="New item">
      <form className="dbme-columns__form" onSubmit={(event) => void submit(event)}>
        <label htmlFor={titleId}>Title</label>
        <input
          id={titleId}
          className="dbme-input"
          type="text"
          autoComplete="off"
          value={title}
          onChange={(event) => {
            setTitle(event.target.value);
            setFailed(false);
          }}
        />
        <label htmlFor={dayId}>{date.label}</label>
        <input
          id={dayId}
          className="dbme-input"
          type="date"
          value={text}
          onChange={(event) => {
            setText(event.target.value);
          }}
        />
        {failed ? <StatusMessage tone="error">The item was not added.</StatusMessage> : null}
        <div className="dbme-dialog__actions">
          <Button onClick={onClose}>Cancel</Button>
          <Button variant="primary" type="submit" disabled={!ready}>
            Add item
          </Button>
        </div>
      </form>
    </Dialog>
  );
}
