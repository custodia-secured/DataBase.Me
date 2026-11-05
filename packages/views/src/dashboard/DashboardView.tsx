// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * The Dashboard view (M8, Task 8): saved charts and views as tiles in a grid, one column on a phone. A
 * saved chart shows as its chart; any other saved view as the list of what it holds, which is what fits
 * a tile. Each tile has a boundary of its own, so a failure in one stays in that one.
 *
 * Tiles move and resize from a menu on each, which is the way by keyboard and by touch and needs no
 * other. A dashboard is a saved view like any other: changes last until Save view, or until it is left.
 */
import {
  useFields,
  useViews,
  type Id,
  type ObjectSummary_Serialize,
  type Sort,
  type ViewRow_Serialize,
} from "@databaseme/client";
import { Button, Dialog, Ellipsis, Menu, StatusMessage } from "@databaseme/ui";
import { useId, useMemo, useState, type SyntheticEvent } from "react";
import { DEFAULT_CHART, type ChartSettings } from "../charts/chart";
import { ChartPanel } from "../charts/ChartPanel";
import { ListAlternative } from "../framework/ListAlternative";
import { listingFromQuery, type Listing } from "../framework/listing";
import { useViewState, type SavedConfig } from "../framework/useViewState";
import { ViewBoundary } from "../framework/ViewBoundary";
import { ViewControls } from "../framework/ViewControls";
import type { ViewProps } from "../registry";
import { displayValue } from "../values";
import {
  movedTile,
  tilesOf,
  widenedTile,
  withoutTile,
  withTile,
  type DashboardOptions,
  type Tile,
} from "./tiles";

const DEFAULTS: DashboardOptions = { tiles: [] };

export function DashboardView({ objectId, onOpen }: ViewProps) {
  const state = useViewState<DashboardOptions>(objectId, DEFAULTS);
  const views = useViews();
  const tiles = tilesOf(state.options);
  const [adding, setAdding] = useState(false);
  const change = (next: readonly Tile[]) => {
    state.setOptions({ tiles: next });
  };
  return (
    <section className="dbme-dashboard" aria-label="Dashboard">
      <ViewControls state={state} objectId={objectId} renderer="dashboard" alternative bare>
        <Button
          onClick={() => {
            setAdding(true);
          }}
        >
          Add tile
        </Button>
      </ViewControls>
      {tiles.length === 0 ? (
        <StatusMessage>
          A dashboard shows saved charts and views side by side. Use Add tile to put one here.
        </StatusMessage>
      ) : (
        <div className="dbme-dashboard__grid">
          {tiles.map((tile, index) => (
            <TileFrame
              key={tile.view}
              tile={tile}
              first={index === 0}
              last={index === tiles.length - 1}
              view={views.data?.find((view) => view.id === tile.view)}
              loaded={views.isSuccess}
              objectId={objectId}
              asList={state.asList}
              onOpen={onOpen}
              onMove={(by) => {
                change(movedTile(tiles, tile.view, by));
              }}
              onWiden={() => {
                change(widenedTile(tiles, tile.view));
              }}
              onRemove={() => {
                change(withoutTile(tiles, tile.view));
              }}
            />
          ))}
        </div>
      )}
      {adding ? (
        <AddTileDialog
          views={views.data ?? []}
          tiles={tiles}
          onAdd={(view) => {
            change(withTile(tiles, view));
            setAdding(false);
          }}
          onClose={() => {
            setAdding(false);
          }}
        />
      ) : null}
    </section>
  );
}

function TileFrame({
  tile,
  first,
  last,
  view,
  loaded,
  objectId,
  asList,
  onOpen,
  onMove,
  onWiden,
  onRemove,
}: {
  tile: Tile;
  first: boolean;
  last: boolean;
  view: ViewRow_Serialize | undefined;
  /** Whether the saved views have been read, so that a missing one is really gone. */
  loaded: boolean;
  objectId: Id;
  asList: boolean;
  onOpen: (id: Id) => void;
  onMove: (by: number) => void;
  onWiden: () => void;
  onRemove: () => void;
}) {
  const name = view?.name ?? (loaded ? "Saved view gone" : "");
  return (
    <section className={`dbme-tile${tile.wide ? " is-wide" : ""}`} aria-label={name}>
      <div className="dbme-tile__head">
        <h3 className="dbme-tile__title">{name}</h3>
        <Menu
          label={`Change tile ${name}`}
          icon={Ellipsis}
          items={[
            {
              label: "Move earlier",
              disabled: first,
              onSelect: () => {
                onMove(-1);
              },
            },
            {
              label: "Move later",
              disabled: last,
              onSelect: () => {
                onMove(1);
              },
            },
            { label: tile.wide ? "Make narrower" : "Make wider", onSelect: onWiden },
            { label: "Remove tile", danger: true, onSelect: onRemove },
          ]}
        />
      </div>
      <div className="dbme-tile__body">
        <ViewBoundary quiet>
          {view ? (
            <TileContent view={view} objectId={objectId} asList={asList} onOpen={onOpen} />
          ) : loaded ? (
            <StatusMessage>This saved view no longer exists, so this tile can go.</StatusMessage>
          ) : null}
        </ViewBoundary>
      </div>
    </section>
  );
}

function TileContent({
  view,
  objectId,
  asList,
  onOpen,
}: {
  view: ViewRow_Serialize;
  objectId: Id;
  asList: boolean;
  onOpen: (id: Id) => void;
}) {
  const { listing, sort } = useMemo(
    () => listingFromQuery(view.query_json, view.owner_object_id ?? objectId),
    [view.query_json, view.owner_object_id, objectId],
  );
  const config = (view.config_json ?? {}) as SavedConfig<ChartSettings>;
  // The read keeps a saved view's settings the same object while nothing about them changes.
  const settings = useMemo(
    () => ({
      ...DEFAULT_CHART,
      ...(view.config_json as SavedConfig<ChartSettings> | null)?.options,
    }),
    [view.config_json],
  );
  if (view.renderer === "charts") {
    return (
      <ChartPanel
        listing={listing}
        settings={settings}
        asList={asList || config.as_list === true}
      />
    );
  }
  return <ListTile label={view.name} listing={listing} sort={sort} onOpen={onOpen} />;
}

/** Any other saved view: what it holds, with the first fields written out, as a tile has no room for more. */
function ListTile({
  label,
  listing,
  sort,
  onOpen,
}: {
  label: string;
  listing: Listing;
  sort: Sort;
  onOpen: (id: Id) => void;
}) {
  const fields = useFields(listing.source);
  const columns = useMemo(
    () =>
      (fields.data ?? []).slice(0, 2).map((field) => ({
        label: field.label,
        value: (summary: ObjectSummary_Serialize) =>
          displayValue(field, summary.object.props_json[field.key]),
      })),
    [fields.data],
  );
  return (
    <ListAlternative
      label={label}
      listing={listing}
      sort={sort}
      columns={columns}
      onOpen={onOpen}
    />
  );
}

const RENDERERS: Record<string, string> = {
  charts: "Chart",
  table: "Table",
  gallery: "Gallery",
  kanban: "Kanban",
  calendar: "Calendar",
  timeline: "Timeline",
  results: "Search",
};

function AddTileDialog({
  views,
  tiles,
  onAdd,
  onClose,
}: {
  views: readonly ViewRow_Serialize[];
  tiles: readonly Tile[];
  onAdd: (view: Id) => void;
  onClose: () => void;
}) {
  const id = useId();
  const choices = views.filter(
    (view) => view.renderer !== "dashboard" && !tiles.some((tile) => tile.view === view.id),
  );
  const [chosen, setChosen] = useState("");
  const value = choices.some((view) => view.id === chosen) ? chosen : (choices[0]?.id ?? "");
  const submit = (event: SyntheticEvent) => {
    event.preventDefault();
    if (value !== "") onAdd(value);
  };
  return (
    <Dialog open onClose={onClose} title="Add tile">
      <form className="dbme-columns__form" onSubmit={submit}>
        {choices.length === 0 ? (
          <StatusMessage>
            There is nothing saved to add yet. Set up a chart or a view, then use Save view on it.
          </StatusMessage>
        ) : (
          <>
            <label htmlFor={id}>Saved chart or view</label>
            <select
              id={id}
              className="dbme-input"
              value={value}
              onChange={(event) => {
                setChosen(event.target.value);
              }}
            >
              {choices.map((view) => (
                <option key={view.id} value={view.id}>
                  {view.name} ({RENDERERS[view.renderer] ?? view.renderer})
                </option>
              ))}
            </select>
          </>
        )}
        <div className="dbme-dialog__actions">
          <Button onClick={onClose}>Cancel</Button>
          <Button variant="primary" type="submit" disabled={value === ""}>
            Add
          </Button>
        </div>
      </form>
    </Dialog>
  );
}
