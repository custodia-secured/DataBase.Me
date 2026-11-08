// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * The Map view (M8, Task 10, ADR-0033): where what is under a page is, clustered, on a basemap that ships
 * with the app. A location is set by a click or typed, and there is no geocoder, since that would be a
 * request. "Show as list" has every item with its coordinates.
 */
import {
  commands,
  keys,
  MANUAL,
  unwrap,
  useQueryCount,
  useQueryResults,
  type Id,
  type ObjectSummary_Serialize,
} from "@databaseme/client";
import { Button, Dialog, StatusMessage } from "@databaseme/ui";
import { useQueries } from "@tanstack/react-query";
import { useEffect, useId, useMemo, useState, type SyntheticEvent } from "react";
import { actions } from "../actions";
import { ListAlternative, type ListColumn } from "../framework/ListAlternative";
import { listingQuery, narrowed, type Listing } from "../framework/listing";
import { useViewState } from "../framework/useViewState";
import { ViewControls } from "../framework/ViewControls";
import type { ViewProps } from "../registry";
import {
  MOST_PLACES,
  parseCoordinate,
  placeLabel,
  placesOf,
  placesSummary,
  PLACES_PER_READ,
  type Place,
} from "./geo";
import { MapCanvas } from "./MapCanvas";
import { useMapFile } from "./mapFile";

const OPTIONS = {};
/** How many items the dialog offers to place. */
const MOST_TO_PLACE = 200;

const LAT = "$geo_lat";

const COLUMNS: readonly ListColumn[] = [
  {
    label: "Location",
    value: ({ object }) =>
      object.geo_lat === null || object.geo_lon === null
        ? "No location"
        : placeLabel(object.geo_lat, object.geo_lon),
  },
];

/** Every place a listing has, up to {@link MOST_PLACES}, read in as many goes as the store needs. */
function usePlaces(listing: Listing) {
  const located = useMemo(
    () => narrowed(listing, { field: LAT, op: "is_not_empty", value: null }),
    [listing],
  );
  const count = useQueryCount(listingQuery(located));
  const total = count.data ?? 0;
  const reads = useQueries({
    queries: Array.from(
      { length: Math.ceil(Math.min(total, MOST_PLACES) / PLACES_PER_READ) },
      (_, read) => {
        const query = listingQuery(located, MANUAL, read * PLACES_PER_READ, PLACES_PER_READ);
        return {
          queryKey: keys.query(query),
          queryFn: () => unwrap(commands.objectsQuery(query)),
        };
      },
    ),
  });
  const done = reads.every((read) => read.isSuccess);
  const stamp = reads.map((read) => read.dataUpdatedAt).join();
  const places = useMemo(
    () =>
      placesOf(reads.flatMap((read) => (read.data ?? []) as readonly ObjectSummary_Serialize[])),
    // eslint-disable-next-line react-hooks/exhaustive-deps -- `reads` is a new array each time, and `stamp` says when what it holds has changed
    [stamp],
  );
  return {
    places,
    total,
    ready: count.isSuccess && done,
    failed: count.isError || reads.some((read) => read.isError),
  };
}

function PlaceDialog({
  listing,
  startWith,
  onClose,
  onPick,
  onPlaced,
}: {
  listing: Listing;
  startWith: Id | null;
  onClose: () => void;
  onPick: (item: { id: Id; title: string }) => void;
  onPlaced: (id: Id) => void;
}) {
  const base = useId();
  const items = useQueryResults(listingQuery(listing, MANUAL, 0, MOST_TO_PLACE));
  const rows = useMemo(() => items.data ?? [], [items.data]);
  const [id, setId] = useState<Id | null>(startWith);
  const chosen = rows.find((row) => row.object.id === id) ?? rows[0];
  const [typed, setTyped] = useState<{ lat: string; lon: string } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const object = chosen?.object;
  const shown = typed ?? {
    lat: object?.geo_lat?.toString() ?? "",
    lon: object?.geo_lon?.toString() ?? "",
  };

  const submit = (event: SyntheticEvent) => {
    event.preventDefault();
    if (!object) return;
    const lat = parseCoordinate(shown.lat, 90);
    const lon = parseCoordinate(shown.lon, 180);
    if (lat === null || lon === null) {
      setError(
        lat === null
          ? "A latitude is a number from -90 to 90."
          : "A longitude is a number from -180 to 180.",
      );
      return;
    }
    void actions.setGeo(object.id, lat, lon).then(() => {
      onPlaced(object.id);
    });
  };

  return (
    <Dialog open onClose={onClose} title="Place on the map">
      <form className="dbme-columns__form" onSubmit={submit}>
        <label htmlFor={`${base}-item`}>Item</label>
        <select
          id={`${base}-item`}
          className="dbme-input"
          value={object?.id ?? ""}
          onChange={(event) => {
            setId(event.target.value);
            setTyped(null);
            setError(null);
          }}
        >
          {rows.map(({ object: row }) => (
            <option key={row.id} value={row.id}>
              {row.title || "Untitled"}
              {row.geo_lat === null ? " (no location)" : ""}
            </option>
          ))}
        </select>
        <label htmlFor={`${base}-lat`}>Latitude</label>
        <input
          id={`${base}-lat`}
          className="dbme-input"
          inputMode="decimal"
          autoComplete="off"
          value={shown.lat}
          onChange={(event) => {
            setTyped({ ...shown, lat: event.target.value });
          }}
        />
        <label htmlFor={`${base}-lon`}>Longitude</label>
        <input
          id={`${base}-lon`}
          className="dbme-input"
          inputMode="decimal"
          autoComplete="off"
          value={shown.lon}
          onChange={(event) => {
            setTyped({ ...shown, lon: event.target.value });
          }}
        />
        {error === null ? null : <StatusMessage tone="error">{error}</StatusMessage>}
        <div className="dbme-dialog__actions">
          <Button onClick={onClose}>Cancel</Button>
          <Button
            disabled={!object}
            onClick={() => {
              if (object) onPick({ id: object.id, title: object.title || "Untitled" });
            }}
          >
            Pick on the map
          </Button>
          <Button type="submit" variant="primary" disabled={!object}>
            Save
          </Button>
        </div>
      </form>
    </Dialog>
  );
}

function Chosen({
  place,
  onOpen,
  onMove,
}: {
  place: Place;
  onOpen: (id: Id) => void;
  onMove: () => void;
}) {
  return (
    <div className="dbme-geo__selected" role="status">
      <span className="dbme-geo__selected-name">
        <strong>{place.title}</strong> <span>{placeLabel(place.lat, place.lon)}</span>
      </span>
      <Button
        onClick={() => {
          onOpen(place.id);
        }}
      >
        Open
      </Button>
      <Button onClick={onMove}>Move</Button>
      <Button
        onClick={() => {
          void actions.setGeo(place.id, null, null);
        }}
      >
        Remove location
      </Button>
    </div>
  );
}

export function MapView({ objectId, onOpen }: ViewProps) {
  const state = useViewState<typeof OPTIONS>(objectId, OPTIONS);
  const { listing } = state;
  const { places, total, ready, failed } = usePlaces(listing);
  const all = useQueryCount(listingQuery(listing));
  const mapFile = useMapFile();
  const [selected, setSelected] = useState<Id | null>(null);
  const [dialog, setDialog] = useState<{ startWith: Id | null } | null>(null);
  const [picking, setPicking] = useState<{ id: Id; title: string } | null>(null);
  const [cannotDraw, setCannotDraw] = useState(false);
  const chosen = places.find((place) => place.id === selected) ?? null;
  const summary = placesSummary(total, Math.max(0, (all.data ?? total) - total));

  useEffect(() => {
    if (!picking) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") setPicking(null);
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
    };
  }, [picking]);

  return (
    <section className="dbme-geo" aria-label="Map">
      <ViewControls state={state} objectId={objectId} renderer="map" alternative />
      <div className="dbme-view-bar">
        <Button
          onClick={() => {
            setDialog({ startWith: null });
          }}
        >
          Place on the map
        </Button>
        <p className="dbme-geo__count" role="status">
          {ready ? summary : ""}
        </p>
      </div>
      {failed ? <StatusMessage tone="error">The places could not be read.</StatusMessage> : null}
      {mapFile.failed ? (
        <StatusMessage tone="error">
          The map file could not be read, so this is the map the app comes with.
        </StatusMessage>
      ) : null}
      {total > MOST_PLACES ? (
        <StatusMessage>
          This shows the first {MOST_PLACES.toLocaleString("en")} of {total.toLocaleString("en")}{" "}
          places. The list has them all.
        </StatusMessage>
      ) : null}
      {picking ? (
        <div className="dbme-geo__picking" role="status">
          <span>Click the map to place {picking.title}.</span>
          <Button
            onClick={() => {
              setPicking(null);
            }}
          >
            Cancel
          </Button>
        </div>
      ) : null}
      {cannotDraw ? (
        <StatusMessage>This device cannot draw the map, so here is the list.</StatusMessage>
      ) : null}
      {state.asList || cannotDraw ? (
        <ListAlternative
          label="Places"
          listing={listing}
          sort={MANUAL}
          columns={COLUMNS}
          onOpen={onOpen}
        />
      ) : mapFile.settled ? (
        <>
          {chosen ? (
            <Chosen
              place={chosen}
              onOpen={onOpen}
              onMove={() => {
                setDialog({ startWith: chosen.id });
              }}
            />
          ) : null}
          <MapCanvas
            key={objectId}
            places={places}
            file={mapFile.file}
            selected={selected}
            picking={picking !== null}
            summary={ready ? `Map. ${summary}` : "Map"}
            onSelect={setSelected}
            onPick={(lat, lon) => {
              if (!picking) return;
              const { id } = picking;
              setPicking(null);
              setSelected(id);
              void actions.setGeo(id, lat, lon);
            }}
            onFailed={() => {
              setCannotDraw(true);
            }}
          />
        </>
      ) : null}
      {dialog ? (
        <PlaceDialog
          listing={listing}
          startWith={dialog.startWith}
          onClose={() => {
            setDialog(null);
          }}
          onPick={(item) => {
            setDialog(null);
            setPicking(item);
            state.setAsList(false);
          }}
          onPlaced={(id) => {
            setDialog(null);
            setSelected(id);
          }}
        />
      ) : null}
    </section>
  );
}
