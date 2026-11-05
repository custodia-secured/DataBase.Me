// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * The Canvas view (M8, Task 12, ADR-0035): a page's own drawing, with cards that show other pages and stay up
 * to date with them. Every element is a row in the store, so a stroke is one undo, a vault replays to the same
 * drawing, and a page takes its canvas with it. "Show as list" has everything with its position, and moves it
 * from the keyboard. The drawing library loads only for the picture.
 */
import {
  MANUAL,
  useCanvas,
  useQueryResults,
  useSummaries,
  type CanvasItemRow_Serialize,
  type Id,
} from "@databaseme/client";
import { Button, Dialog, StatusMessage } from "@databaseme/ui";
import { lazy, Suspense, useId, useMemo, useState, type SyntheticEvent } from "react";
import { actions, flushEditors } from "../actions";
import { listingOf, listingQuery } from "../framework/listing";
import { useIsDark } from "../framework/useTheme";
import type { ViewProps } from "../registry";
import { CardList, STEP } from "./CardList";
import { canvasSummary, moved, nameOf, newCard, placeLabel, type CanvasChanges } from "./scene";

declare global {
  interface Window {
    /** Where Excalidraw looks for its fonts: the app's own copy (vite-plugin-excalidraw.ts). */
    EXCALIDRAW_ASSET_PATH?: string;
  }
}

const CanvasScene = lazy(() => {
  window.EXCALIDRAW_ASSET_PATH = "/excalidraw-assets/";
  return import("./CanvasScene");
});

/** How many pages the dialog offers. */
const MOST_TO_OFFER = 200;
const NO_ROWS: readonly CanvasItemRow_Serialize[] = [];

function AddCardDialog({
  objectId,
  shown,
  onClose,
  onAdd,
}: {
  objectId: Id;
  /** What already has a card. */
  shown: ReadonlySet<Id>;
  onClose: () => void;
  onAdd: (id: Id) => void;
}) {
  const base = useId();
  const items = useQueryResults(listingQuery(listingOf(objectId), MANUAL, 0, MOST_TO_OFFER));
  const offered = useMemo(
    () => (items.data ?? []).filter(({ object }) => !shown.has(object.id)),
    [items.data, shown],
  );
  const [id, setId] = useState<Id | null>(null);
  const chosen = offered.find(({ object }) => object.id === id) ?? offered[0];
  const submit = (event: SyntheticEvent) => {
    event.preventDefault();
    if (chosen) onAdd(chosen.object.id);
  };
  return (
    <Dialog open onClose={onClose} title="Add card">
      <form className="dbme-columns__form" onSubmit={submit}>
        {items.isSuccess && offered.length === 0 ? (
          <StatusMessage>Every page under this one already has a card.</StatusMessage>
        ) : (
          <>
            <label htmlFor={`${base}-page`}>Page</label>
            <select
              id={`${base}-page`}
              className="dbme-input"
              value={chosen?.object.id ?? ""}
              onChange={(event) => {
                setId(event.target.value);
              }}
            >
              {offered.map(({ object }) => (
                <option key={object.id} value={object.id}>
                  {object.title || "Untitled"}
                </option>
              ))}
            </select>
          </>
        )}
        <div className="dbme-dialog__actions">
          <Button onClick={onClose}>Cancel</Button>
          <Button type="submit" variant="primary" disabled={!chosen}>
            Add
          </Button>
        </div>
      </form>
    </Dialog>
  );
}

function Chosen({
  row,
  name,
  list,
  onOpen,
  onRemove,
  onMove,
}: {
  row: CanvasItemRow_Serialize;
  name: string;
  /** The list has move buttons as well, for touch and for whoever does not use Alt. */
  list: boolean;
  onOpen: (id: Id) => void;
  onRemove: () => void;
  onMove: (dx: number, dy: number) => void;
}) {
  const object = row.object_id;
  return (
    <div className="dbme-canvas__selected">
      <span className="dbme-canvas__selected-name">
        <strong>{name}</strong> <span>at {placeLabel(row)}</span>
      </span>
      {object === null ? null : (
        <Button
          onClick={() => {
            onOpen(object);
          }}
        >
          Open
        </Button>
      )}
      {list ? (
        <>
          <Button
            onClick={() => {
              onMove(-STEP, 0);
            }}
          >
            Move left
          </Button>
          <Button
            onClick={() => {
              onMove(STEP, 0);
            }}
          >
            Move right
          </Button>
          <Button
            onClick={() => {
              onMove(0, -STEP);
            }}
          >
            Move up
          </Button>
          <Button
            onClick={() => {
              onMove(0, STEP);
            }}
          >
            Move down
          </Button>
        </>
      ) : null}
      <Button onClick={onRemove}>Remove from canvas</Button>
    </div>
  );
}

function PageCanvas({ objectId, onOpen }: ViewProps) {
  const canvas = useCanvas(objectId);
  const rows = canvas.data?.items ?? NO_ROWS;
  const dark = useIsDark();
  const [asList, setAsList] = useState(false);
  const [adding, setAdding] = useState(false);
  const [selected, setSelected] = useState<string | null>(null);
  const [focus, setFocus] = useState<string | null>(null);
  const [hasPicture, setHasPicture] = useState(false);
  const [announced, setAnnounced] = useState("");

  const cardObjects = useMemo(
    () => rows.flatMap((row) => (row.object_id === null ? [] : [row.object_id])),
    [rows],
  );
  const summaries = useSummaries(cardObjects);
  const titles = useMemo(
    () =>
      new Map(
        (summaries.data ?? []).map(
          ({ object }) => [object.id, object.title || "Untitled"] as const,
        ),
      ),
    [summaries.data],
  );
  const shown = useMemo(() => new Set(cardObjects), [cardObjects]);
  const chosen = rows.find((row) => row.item_id === selected) ?? null;

  const move = (row: CanvasItemRow_Serialize, dx: number, dy: number) => {
    const next = moved(row, dx, dy);
    void actions.applyToCanvas(objectId, [next], []);
    setAnnounced(
      `${nameOf(row, titles)} moved to ${String(Math.round(next.x ?? 0))}, ${String(Math.round(next.y ?? 0))}.`,
    );
  };

  const commit = async ({ upsert, remove }: CanvasChanges) => {
    // `attempt` gives nothing back for a failure, and a canvas it had nothing to write to gives null.
    const written = await actions.applyToCanvas(objectId, upsert, remove);
    return written !== undefined;
  };

  return (
    <section className="dbme-canvas" aria-label="Canvas">
      <div className="dbme-view-bar">
        <Button
          onClick={() => {
            setAdding(true);
          }}
        >
          Add card
        </Button>
        <label className="dbme-canvas__as-list">
          <input
            type="checkbox"
            checked={asList}
            onChange={(event) => {
              setAsList(event.target.checked);
              setSelected(null);
            }}
          />
          Show as list
        </label>
        <p className="dbme-canvas__count" role="status">
          {canvas.isSuccess ? canvasSummary(rows) : ""}
        </p>
        <span className="dbme-visually-hidden" role="status">
          {announced}
        </span>
      </div>
      {canvas.isError ? (
        <StatusMessage tone="error">This canvas could not be read.</StatusMessage>
      ) : null}
      {hasPicture ? (
        <StatusMessage>
          A canvas does not keep pictures yet. This one will be gone when you leave this page.
        </StatusMessage>
      ) : null}
      {chosen ? (
        <Chosen
          row={chosen}
          name={nameOf(chosen, titles)}
          list={asList}
          onOpen={onOpen}
          onRemove={() => {
            void actions.applyToCanvas(objectId, [], [chosen.item_id]);
          }}
          onMove={(dx, dy) => {
            move(chosen, dx, dy);
          }}
        />
      ) : null}
      {!canvas.isSuccess ? null : asList ? (
        <CardList
          rows={rows}
          titles={titles}
          onChoose={setSelected}
          onMove={move}
          onOpen={onOpen}
        />
      ) : (
        <div className="dbme-canvas__stage">
          <Suspense fallback={<StatusMessage>Opening the canvas…</StatusMessage>}>
            <CanvasScene
              rows={rows}
              readAt={canvas.dataUpdatedAt}
              dark={dark}
              focus={focus}
              onSelect={setSelected}
              onOpen={onOpen}
              onPicture={() => {
                setHasPicture(true);
              }}
              onCommit={commit}
            />
          </Suspense>
        </div>
      )}
      {adding ? (
        <AddCardDialog
          objectId={objectId}
          shown={shown}
          onClose={() => {
            setAdding(false);
          }}
          onAdd={(id) => {
            setAdding(false);
            const card = newCard(id, rows);
            setFocus(card.item_id);
            // What is in hand goes first, so that Undo takes back the card and then the stroke.
            void flushEditors().then(() => actions.applyToCanvas(objectId, [card], []));
          }}
        />
      ) : null}
    </section>
  );
}

/** A page's canvas starts afresh for each page, with nothing chosen and the last one's drawing saved. */
export function CanvasView(props: ViewProps) {
  return <PageCanvas key={props.objectId} {...props} />;
}
