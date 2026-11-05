// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * The Timeline view (M8, Task 6): one lane over a date field or an event's start and end, from days to
 * decades. Close up it draws the objects as bars, a running event as long as it runs. Further out it
 * draws how many fall in each bucket, counted in the store (`aggregate`), so zooming out over a large
 * vault reads less, not more. Only what is near the screen exists at all. On a phone the lane runs down
 * the screen. It shows and opens; the Calendar is where a date moves.
 */
import {
  useAggregate,
  useFields,
  useQueryCount,
  useQueryResults,
  type FieldRow,
  type Id,
  type ObjectSummary_Serialize,
} from "@databaseme/client";
import {
  Button,
  ChevronLeft,
  ChevronRight,
  IconButton,
  StatusMessage,
  useMediaQuery,
} from "@databaseme/ui";
import { useId, useLayoutEffect, useMemo, useRef, useState, type KeyboardEvent } from "react";
import {
  chosenDate,
  dateChoices,
  dayOf,
  EVENT,
  longDay,
  msOfDay,
  todayOf,
  type DateChoice,
  type Day,
} from "../calendar/dates";
import { ListAlternative, type ListColumn } from "../framework/ListAlternative";
import { byField, listingQuery, narrowed, type Listing } from "../framework/listing";
import { useBuckets, inBucket } from "../framework/useBuckets";
import { useViewState } from "../framework/useViewState";
import { ViewControls } from "../framework/ViewControls";
import type { ViewProps } from "../registry";
import { displayValue } from "../values";
import {
  axisLabel,
  bucketAt,
  bucketCount,
  bucketName,
  domainOf,
  indexOfDay,
  laneKey,
  layOut,
  LEVELS,
  levelOf,
  offsetOf,
  readWindow,
  zoomed,
  type Domain,
  type Level,
  type LevelId,
} from "./scale";

interface TimelineOptions {
  /** What the lane goes by; the first date field until someone chooses. */
  field: string | null;
  zoom: LevelId;
}

const DEFAULTS: TimelineOptions = { field: null, zoom: "month" };

/** How many objects are read at once. Beyond it, the lane says so and zooming in shows the rest. */
const ITEM_LIMIT = 200;
const RUNNING_LIMIT = 50;

/** The space the axis takes across the lane, and below it the strip that says Today, clear of the bars. */
const AXIS = 52;
const AXIS_VERTICAL = 80;
/** How thick a bar is, and the least it is long, so its title and a finger both fit. */
const ROW = 36;
const MIN_LENGTH = 120;
const MIN_LENGTH_VERTICAL = 44;
const MAX_LANES = 12;
const MAX_LANES_VERTICAL = 3;

const titleOfItem = (summary: ObjectSummary_Serialize) => summary.object.title || "Untitled";

export function TimelineView({ objectId, onOpen }: ViewProps) {
  const state = useViewState(objectId, DEFAULTS);
  const fields = useFields(state.listing.source);
  const choices = useMemo(() => dateChoices(fields.data ?? []), [fields.data]);
  const date = chosenDate(choices, state.options.field);
  const level = levelOf(state.options.zoom);
  const vertical = useMediaQuery("(max-width: 767px)");
  const byId = useId();
  const zoomId = useId();
  const setZoom = (zoom: LevelId) => {
    state.setOptions({ ...state.options, zoom });
  };
  return (
    <section className="dbme-timeline" aria-label="Timeline">
      <ViewControls state={state} objectId={objectId} renderer="timeline" alternative>
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
        <span className="dbme-source">
          <label htmlFor={zoomId}>Zoom</label>
          <select
            id={zoomId}
            className="dbme-input"
            value={level.id}
            onChange={(event) => {
              setZoom(levelOf(event.target.value).id);
            }}
          >
            {LEVELS.map((one) => (
              <option key={one.id} value={one.id}>
                {one.label}
              </option>
            ))}
          </select>
        </span>
      </ViewControls>
      {/* What it goes by is not known until the fields are, as in the Calendar. */}
      {fields.isSuccess ? (
        <Timeline
          key={date.key}
          listing={state.listing}
          date={date}
          level={level}
          vertical={vertical}
          asList={state.asList}
          fields={fields.data}
          onZoom={setZoom}
          onOpen={onOpen}
        />
      ) : null}
    </section>
  );
}

function Timeline({
  listing,
  date,
  level,
  vertical,
  asList,
  fields,
  onZoom,
  onOpen,
}: {
  listing: Listing;
  date: DateChoice;
  level: Level;
  vertical: boolean;
  asList: boolean;
  fields: readonly FieldRow[];
  onZoom: (zoom: LevelId) => void;
  onOpen: (id: Id) => void;
}) {
  const dated = narrowed(listing, { field: date.key, op: "is_not_empty", value: null });
  const years = useAggregate({
    query: listingQuery(dated),
    group_by: [{ field: date.key, bin: "year" }],
    measures: [{ op: "count" }],
  });
  const today = todayOf();
  const domain = useMemo(() => domainOf(years.data, today), [years.data, today]);
  const undated = useQueryCount(
    listingQuery(narrowed(listing, { field: date.key, op: "is_empty", value: null })),
  );
  const missing = undated.data ?? 0;

  if (asList) {
    return (
      <ListAlternative
        label="Dated items"
        listing={dated}
        sort={byField(date.key)}
        columns={listColumns(date, fields)}
        onOpen={onOpen}
      />
    );
  }
  if (years.isSuccess && years.data.rows.length === 0) {
    return (
      <StatusMessage>
        Nothing here has a {date.label.toLowerCase()} yet, so there is nothing to place on a
        timeline.
      </StatusMessage>
    );
  }
  if (!years.isSuccess) return null;
  return (
    <Lane
      // The lane's first bucket decides where everything sits, so a new span starts again.
      key={`${String(domain.firstYear)}:${String(domain.lastYear)}`}
      listing={listing}
      date={date}
      level={level}
      domain={domain}
      today={today}
      vertical={vertical}
      missing={missing}
      onZoom={onZoom}
      onOpen={onOpen}
    />
  );
}

/** What the lane shows by position, written out: its dates first, then the other fields. */
function listColumns(date: DateChoice, fields: readonly FieldRow[]): ListColumn[] {
  const seen = new Set([date.key]);
  const others = fields.filter((field) => {
    if (seen.has(field.key)) return false;
    seen.add(field.key);
    return true;
  });
  const when: ListColumn[] = [
    {
      label: date.label,
      value: (summary) => {
        const day = dayOf(summary, date.key);
        return day ? longDay(day) : "";
      },
    },
  ];
  if (date.key === EVENT) {
    when.push({
      label: "Ends",
      value: (summary) => {
        const { event_end: end } = summary.object;
        return end === null ? "" : longDay(new Date(end).toISOString().slice(0, 10));
      },
    });
  }
  return [
    ...when,
    ...others.slice(0, 3).map((field) => ({
      label: field.label,
      value: (summary: ObjectSummary_Serialize) =>
        displayValue(field, summary.object.props_json[field.key]),
    })),
  ];
}

/** What can be chosen in the lane: an object, or further out a bucket with something in it. */
interface Thing {
  id: string;
  kind: "item" | "bucket";
  name: string;
  /** Where it starts along the lane. */
  at: number;
  /** The day to zoom in on, for a bucket. */
  day: Day;
}

function Lane({
  listing,
  date,
  level,
  domain,
  today,
  vertical,
  missing,
  onZoom,
  onOpen,
}: {
  listing: Listing;
  date: DateChoice;
  level: Level;
  domain: Domain;
  today: Day;
  vertical: boolean;
  missing: number;
  onZoom: (zoom: LevelId) => void;
  onOpen: (id: Id) => void;
}) {
  const base = useId();
  const scroller = useRef<HTMLDivElement | null>(null);
  const [scroll, setScroll] = useState(0);
  const [view, setView] = useState(0);
  const [activeId, setActiveId] = useState<string | null>(null);
  /** A bucket's length: its level's own, or longer when that would leave the lane part empty. */
  const sizeOf = (one: Level) =>
    Math.max(vertical ? one.sizeVertical : one.size, view / bucketCount(one, domain));
  const size = sizeOf(level);
  const count = bucketCount(level, domain);

  // Where to look once the lane has its size: today to begin with, then whatever a zoom kept in view.
  const looking = useRef<Day | null>(today);
  // The level last drawn. The same pixels are another date at another level, so however the zoom
  // changes, the day in the middle of the lane is what carries over.
  const drawn = useRef(level);

  const scrollTo = (position: number) => {
    const element = scroller.current;
    if (!element) return;
    if (vertical) element.scrollTop = position;
    else element.scrollLeft = position;
    setScroll(position);
  };

  useLayoutEffect(() => {
    const element = scroller.current;
    if (!element) return;
    const measure = () => {
      setView(vertical ? element.clientHeight : element.clientWidth);
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => {
      observer.disconnect();
    };
  }, [vertical]);

  useLayoutEffect(() => {
    if (drawn.current.id !== level.id) {
      if (looking.current === null) {
        const before = drawn.current;
        const beforeSize = sizeOf(before);
        const buckets = bucketCount(before, domain);
        const middle = Math.floor((scroll + view / 2) / beforeSize);
        // A lane that fits on screen has no place of its own to keep, so zooming in goes to today.
        looking.current =
          buckets * beforeSize <= view + 1
            ? today
            : bucketAt(before, domain, Math.min(buckets - 1, Math.max(0, middle))).start;
      }
      drawn.current = level;
    }
    if (looking.current === null || view === 0) return;
    const index = indexOfDay(level, domain, looking.current);
    looking.current = null;
    scrollTo(Math.max(0, (index + 0.5) * size - view / 2));
    // Only a new level, or a first measurement, asks for this.
    // eslint-disable-next-line react-hooks/exhaustive-deps -- as the line above says
  }, [level.id, view, size]);

  const first = Math.max(0, Math.floor(scroll / size) - 2);
  const last = Math.min(count - 1, Math.ceil((scroll + view) / size) + 2);
  const chunk = Math.max(8, Math.ceil(view / size));
  const { from, to } = readWindow(first, last, chunk, count);
  const inWindow = useMemo(
    () =>
      Array.from({ length: Math.max(0, to - from) }, (_, i) => bucketAt(level, domain, from + i)),
    [level, domain, from, to],
  );
  const sort = useMemo(() => byField(date.key), [date.key]);
  const read = useBuckets(listing, date.key, level.bin, inWindow, 0, sort);

  const windowStart = inWindow[0]?.start ?? today;
  const windowEnd = inWindow.at(-1)?.end ?? today;
  const items = useQueryResults(
    level.items
      ? listingQuery(
          narrowed(listing, ...inBucket(date.key, { key: "", start: windowStart, end: windowEnd })),
          sort,
          0,
          ITEM_LIMIT,
        )
      : null,
  );
  // An event that began before what is read and is still running there has its start out of range.
  const running = useQueryResults(
    level.items && date.key === EVENT
      ? listingQuery(
          narrowed(
            listing,
            { field: EVENT, op: "lt", value: msOfDay(windowStart) },
            { field: "$event_end", op: "ge", value: msOfDay(windowStart) },
          ),
          sort,
          0,
          RUNNING_LIMIT,
        )
      : null,
  );

  const minLength = vertical ? MIN_LENGTH_VERTICAL : MIN_LENGTH;
  const maxLanes = vertical ? MAX_LANES_VERTICAL : MAX_LANES;
  const { placed, hidden } = useMemo(() => {
    if (!level.items) return { placed: [], hidden: [] };
    const seen = new Set<Id>();
    const spans = [...(running.data ?? []), ...(items.data ?? [])].flatMap((summary) => {
      const { id, event_start: startMs, event_end: endMs } = summary.object;
      const day = dayOf(summary, date.key);
      if (day === null || seen.has(id)) return [];
      seen.add(id);
      const startAt = date.key === EVENT && startMs !== null ? startMs : msOfDay(day);
      const start = offsetOf(level, domain, startAt, size);
      const end =
        date.key === EVENT && endMs !== null ? offsetOf(level, domain, endMs, size) : start;
      return [{ id, start, length: Math.max(minLength, end - start), summary, day }];
    });
    return layOut(spans, maxLanes);
  }, [level, domain, size, minLength, maxLanes, date.key, items.data, running.data]);

  const things: Thing[] = level.items
    ? placed.map(({ span }) => ({
        id: span.id,
        kind: "item",
        name: `${titleOfItem(span.summary)}, ${longDay(span.day)}`,
        at: span.start,
        day: span.day,
      }))
    : read.filled.map((bucket) => ({
        id: bucket.key,
        kind: "bucket",
        name: `${bucketName(level, bucket)}, ${itemsText(read.countOf(bucket.key))}`,
        at: indexOfDay(level, domain, bucket.start) * size,
        day: bucket.start,
      }));
  const active = things.find((thing) => thing.id === activeId);

  /** Changes the zoom, on a given day or on the one in the middle of the lane. */
  const zoomTo = (next: Level, onDay?: Day) => {
    if (next.id === level.id) return;
    looking.current = onDay ?? null;
    setActiveId(null);
    onZoom(next.id);
  };

  const act = (thing: Thing) => {
    if (thing.kind === "item") onOpen(thing.id);
    else zoomTo(zoomed(level.id, -1), thing.day);
  };

  const press = (thing: Thing) => {
    if (activeId === thing.id) act(thing);
    else setActiveId(thing.id);
    scroller.current?.focus();
  };

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const key = laneKey(event.key);
    if (!key) return;
    event.preventDefault();
    switch (key.kind) {
      case "step": {
        const at = things.findIndex((thing) => thing.id === activeId);
        const next =
          at === -1
            ? (things.find((thing) => thing.at >= scroll) ?? things.at(key.by === 1 ? 0 : -1))
            : things[at + key.by];
        if (next) {
          setActiveId(next.id);
          if (next.at < scroll || next.at + size > scroll + view) {
            scrollTo(Math.max(0, next.at - view / 2));
          }
        } else {
          // Nothing more has been read this way: go and look.
          scrollTo(Math.max(0, scroll + key.by * (view / 2)));
        }
        break;
      }
      case "page":
        scrollTo(Math.max(0, scroll + key.by * view * 0.8));
        break;
      case "edge":
        scrollTo(key.to === "first" ? 0 : Math.max(0, count * size - view));
        break;
      case "zoom":
        zoomTo(zoomed(level.id, key.by));
        break;
      case "act":
        if (active) act(active);
        break;
    }
  };

  const across = vertical ? AXIS_VERTICAL : AXIS;
  const most = Math.max(1, ...read.filled.map((bucket) => read.countOf(bucket.key)));
  const onScreen = Array.from({ length: Math.max(0, last - first + 1) }, (_, i) =>
    bucketAt(level, domain, first + i),
  );
  const shownFirst = bucketAt(level, domain, Math.min(count - 1, Math.floor(scroll / size)));
  const shownLast = bucketAt(
    level,
    domain,
    Math.max(0, Math.min(count, Math.ceil((scroll + view) / size)) - 1),
  );
  const counted = inWindow.reduce((sum, bucket) => sum + read.countOf(bucket.key), 0);
  const inRange = placed.filter(({ span }) => span.day >= windowStart && span.day < windowEnd);
  const notDrawn = level.items ? Math.max(hidden.length, counted - inRange.length) : 0;
  const todayAt = indexOfDay(level, domain, today) * size;
  const optionId = (id: string) => `${base}-${id}`;

  return (
    <>
      <div className="dbme-view-bar dbme-timeline__nav">
        <IconButton
          label="Earlier"
          icon={ChevronLeft}
          onClick={() => {
            scrollTo(Math.max(0, scroll - view * 0.8));
          }}
        />
        <Button
          onClick={() => {
            scrollTo(Math.max(0, todayAt + size / 2 - view / 2));
          }}
        >
          Today
        </Button>
        <IconButton
          label="Later"
          icon={ChevronRight}
          onClick={() => {
            scrollTo(scroll + view * 0.8);
          }}
        />
        <h3 className="dbme-timeline__title">
          {`${axisTitle(level, shownFirst)} – ${axisTitle(level, shownLast)}`}
        </h3>
        {missing > 0 ? (
          <span className="dbme-muted">{missing.toLocaleString()} with no date</span>
        ) : null}
      </div>
      <div className="dbme-view-bar dbme-timeline__selected">
        {active ? (
          <>
            <span>{active.name}</span>
            <Button
              onClick={() => {
                act(active);
              }}
            >
              {active.kind === "item" ? "Open" : "Zoom in"}
            </Button>
          </>
        ) : null}
        {notDrawn > 0 ? (
          <span className="dbme-muted">
            {notDrawn.toLocaleString()} more here do not fit. Zoom in to see them.
          </span>
        ) : null}
      </div>
      <div
        ref={scroller}
        className={`dbme-timeline__lane${vertical ? " is-vertical" : ""}`}
        role="listbox"
        aria-label={`Timeline, ${level.label}`}
        aria-orientation={vertical ? "vertical" : "horizontal"}
        aria-activedescendant={active ? optionId(active.id) : undefined}
        tabIndex={0}
        onKeyDown={onKeyDown}
        onScroll={(event) => {
          const element = event.currentTarget;
          setScroll(vertical ? element.scrollTop : element.scrollLeft);
        }}
      >
        <div
          role="presentation"
          className="dbme-timeline__body"
          style={
            vertical
              ? { height: `${String(count * size)}px` }
              : { width: `${String(count * size)}px` }
          }
        >
          <div aria-hidden="true" className="dbme-timeline__axis">
            {onScreen.map((bucket, i) => {
              const at = (first + i) * size;
              return (
                <div
                  key={bucket.key}
                  className="dbme-timeline__tick"
                  style={
                    vertical
                      ? { top: `${String(at)}px`, height: `${String(size)}px` }
                      : { left: `${String(at)}px`, width: `${String(size)}px` }
                  }
                >
                  {axisLabel(level, bucket)}
                </div>
              );
            })}
          </div>
          <div
            aria-hidden="true"
            className="dbme-timeline__today"
            style={
              vertical
                ? { top: `${String(todayAt)}px`, height: `${String(size)}px` }
                : { left: `${String(todayAt)}px`, width: `${String(size)}px` }
            }
          >
            Today
          </div>
          {level.items
            ? placed.map(({ span, lane }) => {
                const thing = things.find((one) => one.id === span.id);
                if (!thing) return null;
                return (
                  <div
                    key={span.id}
                    id={optionId(span.id)}
                    role="option"
                    aria-selected={activeId === span.id}
                    aria-label={thing.name}
                    data-day={span.day}
                    className={`dbme-timeline__bar${activeId === span.id ? " is-active" : ""}`}
                    style={
                      vertical
                        ? {
                            top: `${String(span.start)}px`,
                            height: `${String(span.length - 2)}px`,
                            left: `calc(${String(across)}px + ${String(lane)} * (100% - ${String(across)}px) / ${String(maxLanes)})`,
                            width: `calc((100% - ${String(across)}px) / ${String(maxLanes)} - 4px)`,
                          }
                        : {
                            left: `${String(span.start)}px`,
                            width: `${String(span.length - 2)}px`,
                            top: `${String(across + lane * ROW)}px`,
                            height: `${String(ROW - 4)}px`,
                          }
                    }
                    onClick={() => {
                      press(thing);
                    }}
                  >
                    {titleOfItem(span.summary)}
                  </div>
                );
              })
            : read.filled.map((bucket) => {
                const thing = things.find((one) => one.id === bucket.key);
                if (!thing) return null;
                const number = read.countOf(bucket.key);
                const share = number / most;
                return (
                  <div
                    key={bucket.key}
                    id={optionId(bucket.key)}
                    role="option"
                    aria-selected={activeId === bucket.key}
                    aria-label={thing.name}
                    data-bucket={bucket.key}
                    className={`dbme-timeline__count${activeId === bucket.key ? " is-active" : ""}`}
                    style={
                      vertical
                        ? {
                            top: `${String(thing.at)}px`,
                            height: `${String(size)}px`,
                            left: `${String(across)}px`,
                            right: "0",
                          }
                        : {
                            left: `${String(thing.at)}px`,
                            width: `${String(size)}px`,
                            top: `${String(across)}px`,
                          }
                    }
                    onClick={() => {
                      press(thing);
                    }}
                  >
                    <span className="dbme-timeline__number">{number.toLocaleString()}</span>
                    <span
                      className="dbme-timeline__fill"
                      style={
                        vertical
                          ? { width: `calc((100% - 2.5rem) * ${String(share)})` }
                          : { height: `calc((100% - 1.25rem) * ${String(share)})` }
                      }
                    />
                  </div>
                );
              })}
        </div>
      </div>
    </>
  );
}

/** A bucket in the lane's title: an axis label where it has one, since a decade's mostly do not. */
function axisTitle(level: Level, bucket: { key: string; start: Day; end: Day }): string {
  return level.id === "decade" ? bucket.key : axisLabel(level, bucket);
}

function itemsText(count: number): string {
  return count === 1 ? "1 item" : `${count.toLocaleString()} items`;
}
