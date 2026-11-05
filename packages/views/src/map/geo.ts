// SPDX-License-Identifier: AGPL-3.0-or-later
/** Places and what a map is drawn from (M8, Task 10, ADR-0033), with nothing here that needs a screen. */
import type { Id, ObjectSummary_Serialize } from "@databaseme/client";
import type {
  FilterSpecification,
  LayerSpecification,
  SourceSpecification,
  StyleSpecification,
} from "maplibre-gl";

/** How many places a map shows. Past this it says so, and the list has them all. */
export const MOST_PLACES = 5000;
/** The store's limit on the results of one query (`MAX_LIMIT`). */
export const PLACES_PER_READ = 1000;
/** How far fitting the places goes in on the land the app ships with, which has no more to show. */
const FIT_ZOOM = 4;
/** How far in a map with a file of its own goes, unless the file stops earlier. */
const FIT_ZOOM_WITH_FILE = 14;

export interface Place {
  id: Id;
  title: string;
  lat: number;
  lon: number;
}

export function placesOf(summaries: readonly ObjectSummary_Serialize[]): Place[] {
  return summaries.flatMap(({ object }) =>
    object.geo_lat === null || object.geo_lon === null
      ? []
      : [
          {
            id: object.id,
            title: object.title || "Untitled",
            lat: object.geo_lat,
            lon: object.geo_lon,
          },
        ],
  );
}

/** A coordinate as someone typed it, or `null` if it is not one within `limit`. */
export function parseCoordinate(text: string, limit: number): number | null {
  const typed = text.trim().replace(",", ".");
  if (!/^[+-]?(\d+(\.\d*)?|\.\d+)$/.test(typed)) return null;
  const value = Number(typed);
  return Math.abs(value) <= limit ? value : null;
}

/** Degrees to five places, about a metre, without zeros that say nothing. */
export function degrees(value: number): string {
  return String(Number(value.toFixed(5)));
}

export function placeLabel(lat: number, lon: number): string {
  return `${degrees(lat)}, ${degrees(lon)}`;
}

export type Bounds = [[number, number], [number, number]];

export function boundsOf(places: readonly { lat: number; lon: number }[]): Bounds | null {
  const first = places[0];
  if (!first) return null;
  const bounds: Bounds = [
    [first.lon, first.lat],
    [first.lon, first.lat],
  ];
  for (const { lat, lon } of places) {
    bounds[0][0] = Math.min(bounds[0][0], lon);
    bounds[0][1] = Math.min(bounds[0][1], lat);
    bounds[1][0] = Math.max(bounds[1][0], lon);
    bounds[1][1] = Math.max(bounds[1][1], lat);
  }
  return bounds;
}

/**
 * A ring of the land as a flat map needs it. `world-atlas` keeps its polygons for a sphere, so one that
 * crosses the antimeridian simply goes from 180 to -180, and flat that is a line across the whole world.
 * Here it carries on past 180 instead, and the map's tiling wraps what lies beyond.
 *
 * It ends up in the world most of it is in, wherever the ring happens to start. Eurasia's starts at -180,
 * in Chukotka, and carried on from there the whole continent would be a world to the west of the Caspian,
 * which is its hole: tiled together, the two are joined by a sliver of land around the Earth.
 *
 * A ring that goes all the way round a pole ends a whole turn from where it began, and is closed along that
 * pole.
 */
export function flatRing(ring: readonly GeoJSON.Position[]): GeoJSON.Position[] {
  const flat: GeoJSON.Position[] = [];
  let turned = 0;
  let previous = ring[0]?.[0] ?? 0;
  let latitudes = 0;
  let west = Infinity;
  let east = -Infinity;
  for (const [lon = 0, lat = 0] of ring) {
    if (lon - previous > 180) turned -= 360;
    else if (lon - previous < -180) turned += 360;
    previous = lon;
    latitudes += lat;
    const carried = lon + turned;
    west = Math.min(west, carried);
    east = Math.max(east, carried);
    flat.push([carried, lat]);
  }
  const worlds = Math.ceil(((west + east) / 2 - 180) / 360);
  const inWorld = flat.map(([lon = 0, lat = 0]) => [lon - worlds * 360, lat]);
  const first = inWorld[0];
  const last = inWorld.at(-1);
  if (turned !== 0 && first && last) {
    const pole = latitudes < 0 ? -90 : 90;
    inWorld.push([last[0] ?? 0, pole], [first[0] ?? 0, pole], [first[0] ?? 0, first[1] ?? 0]);
  }
  return inWorld;
}

/** The land as a flat map needs it (see {@link flatRing}). */
export function flatLand(land: GeoJSON.Feature | GeoJSON.FeatureCollection): GeoJSON.GeoJSON {
  const flatGeometry = (geometry: GeoJSON.Geometry): GeoJSON.Geometry => {
    if (geometry.type === "Polygon") {
      return { type: "Polygon", coordinates: geometry.coordinates.map(flatRing) };
    }
    if (geometry.type === "MultiPolygon") {
      return {
        type: "MultiPolygon",
        coordinates: geometry.coordinates.map((polygon) => polygon.map(flatRing)),
      };
    }
    return geometry;
  };
  const flatFeature = (feature: GeoJSON.Feature): GeoJSON.Feature => ({
    ...feature,
    geometry: flatGeometry(feature.geometry),
  });
  return land.type === "Feature"
    ? flatFeature(land)
    : { ...land, features: land.features.map(flatFeature) };
}

export function fitZoom(file: MapFileShape | null): number {
  return file ? Math.min(FIT_ZOOM_WITH_FILE, file.maxZoom) : FIT_ZOOM;
}

export function placesSummary(placed: number, missing: number): string {
  const places = placed === 1 ? "1 place" : `${placed.toLocaleString("en")} places`;
  if (missing === 0) return `${places} on the map.`;
  const without = missing === 1 ? "1 item has" : `${missing.toLocaleString("en")} items have`;
  return `${places} on the map. ${without} no location.`;
}

/** What the map draws in, from the theme's tokens. */
export interface MapLook {
  sea: string;
  land: string;
  line: string;
  point: string;
  /** What is written on a point's colour. */
  onPoint: string;
  surface: string;
  selected: string;
}

/** What a map file holds, as far as drawing it goes. */
export interface MapFileShape {
  /** The name it goes by in `pmtiles://` URLs. */
  key: string;
  kind: "raster" | "vector";
  minZoom: number;
  maxZoom: number;
  /** A vector file's layers. */
  layers: readonly string[];
}

/** No more of a vector file's layers than this are drawn. */
const MOST_FILE_LAYERS = 24;

function fileLayers(file: MapFileShape, look: MapLook): LayerSpecification[] {
  if (file.kind === "raster") return [{ id: "file-tiles", type: "raster", source: "file" }];
  // A file can name its layers anything, and without its own style all that can be said is what is
  // water. There are no labels: drawing text would take glyphs from somewhere.
  return file.layers.slice(0, MOST_FILE_LAYERS).flatMap((layer, at): LayerSpecification[] => {
    const water = /water|ocean|sea|lake/i.test(layer);
    return [
      {
        id: `file-fill-${String(at)}`,
        type: "fill",
        source: "file",
        "source-layer": layer,
        filter: ["==", ["geometry-type"], "Polygon"],
        paint: { "fill-color": water ? look.sea : look.land, "fill-outline-color": look.line },
      },
      {
        id: `file-line-${String(at)}`,
        type: "line",
        source: "file",
        "source-layer": layer,
        filter: ["==", ["geometry-type"], "LineString"],
        paint: { "line-color": look.line, "line-width": 1 },
      },
    ];
  });
}

const CLUSTER_RADIUS = 48;
/** A cluster's size follows its count in steps, so that a large one reads as large. */
const CLUSTER_SIZES = [14, 10, 17, 100, 21, 1000, 26] as const;

export const COUNT_IMAGE = "count";

export function placesData(places: readonly Place[]): GeoJSON.FeatureCollection {
  return {
    type: "FeatureCollection",
    features: places.map((place) => ({
      type: "Feature",
      properties: { id: place.id, title: place.title },
      geometry: { type: "Point", coordinates: [place.lon, place.lat] },
    })),
  };
}

const NOTHING_SELECTED = "";

export function selectedFilter(selected: Id | null): FilterSpecification {
  return ["==", ["get", "id"], selected ?? NOTHING_SELECTED];
}

/**
 * The whole map as a style. Nothing here names a URL but the file's own `pmtiles://`, which goes over
 * IPC, and no layer draws text: cluster counts are images the page draws (`COUNT_IMAGE`), since text
 * would take glyphs, which are a request.
 */
export function mapStyle({
  look,
  land,
  places,
  selected,
  file,
}: {
  look: MapLook;
  land: GeoJSON.GeoJSON;
  places: readonly Place[];
  selected: Id | null;
  file: MapFileShape | null;
}): StyleSpecification {
  const sources: Record<string, SourceSpecification> = {
    land: { type: "geojson", data: land },
    places: {
      type: "geojson",
      data: placesData(places),
      cluster: true,
      clusterRadius: CLUSTER_RADIUS,
    },
  };
  if (file) {
    const tiles = [`pmtiles://${file.key}/{z}/{x}/{y}`];
    sources.file =
      file.kind === "raster"
        ? {
            type: "raster",
            tiles,
            tileSize: 256,
            minzoom: file.minZoom,
            maxzoom: file.maxZoom,
          }
        : { type: "vector", tiles, minzoom: file.minZoom, maxzoom: file.maxZoom };
  }
  const ring = { "circle-stroke-color": look.surface, "circle-stroke-width": 2 };
  return {
    version: 8,
    sources,
    layers: [
      { id: "sea", type: "background", paint: { "background-color": look.sea } },
      { id: "land", type: "fill", source: "land", paint: { "fill-color": look.land } },
      { id: "coast", type: "line", source: "land", paint: { "line-color": look.line } },
      ...(file ? fileLayers(file, look) : []),
      {
        id: "clusters",
        type: "circle",
        source: "places",
        filter: ["has", "point_count"],
        paint: {
          "circle-color": look.point,
          "circle-radius": ["step", ["get", "point_count"], ...CLUSTER_SIZES],
          ...ring,
        },
      },
      {
        id: "cluster-counts",
        type: "symbol",
        source: "places",
        filter: ["has", "point_count"],
        layout: {
          "icon-image": [
            "concat",
            `${COUNT_IMAGE}:${look.onPoint}:`,
            ["get", "point_count_abbreviated"],
          ],
          "icon-allow-overlap": true,
          "icon-ignore-placement": true,
        },
      },
      {
        id: "points",
        type: "circle",
        source: "places",
        filter: ["!", ["has", "point_count"]],
        paint: { "circle-color": look.point, "circle-radius": 7, ...ring },
      },
      {
        id: "selected",
        type: "circle",
        source: "places",
        filter: selectedFilter(selected),
        paint: {
          "circle-color": look.point,
          "circle-radius": 9,
          "circle-stroke-color": look.selected,
          "circle-stroke-width": 3,
        },
      },
    ],
  };
}
