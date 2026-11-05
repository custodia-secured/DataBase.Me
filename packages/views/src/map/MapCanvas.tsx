// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * The map itself (M8, Task 10, ADR-0033): MapLibre over the land the app ships with. Its own arrow and
 * plus-and-minus keys pan and zoom. Whatever it draws is also in the list, which is also where someone
 * goes if this machine cannot draw it at all.
 */
import { type Id } from "@databaseme/client";
import { IconButton, Maximize, Minus, Plus } from "@databaseme/ui";
import landAtlas from "world-atlas/land-110m.json";
import { Map as MapLibre, setWorkerUrl, type GeoJSONSource } from "maplibre-gl";
import workerUrl from "maplibre-gl/dist/maplibre-gl-worker.mjs?url";
import "maplibre-gl/dist/maplibre-gl.css";
import { feature } from "topojson-client";
import type { GeometryCollection, Topology } from "topojson-specification";
import { useEffect, useMemo, useRef } from "react";
import { useThemeKey } from "../framework/useTheme";
import {
  boundsOf,
  COUNT_IMAGE,
  fitZoom,
  flatLand,
  mapStyle,
  selectedFilter,
  type MapFileShape,
  type MapLook,
  type Place,
} from "./geo";

/** How far from a click something counts as clicked: a finger needs more than a pointer's few pixels. */
const REACH = 12;
const FIT_PADDING = 48;
/** The land once, for every map: it is parsed from text the app ships with, and it never changes. */
let land: GeoJSON.GeoJSON | null = null;

function theLand(): GeoJSON.GeoJSON {
  if (!land) {
    const atlas = landAtlas as unknown as Topology<{ land: GeometryCollection }>;
    land = flatLand(feature(atlas, atlas.objects.land));
  }
  return land;
}

function useMapLook(): MapLook {
  const theme = useThemeKey();
  return useMemo(() => {
    const style = getComputedStyle(document.documentElement);
    const token = (name: string) => style.getPropertyValue(`--dbme-${name}`).trim();
    return {
      sea: token("map-sea"),
      land: token("map-land"),
      line: token("map-line"),
      point: token("color-accent"),
      onPoint: token("color-bg"),
      surface: token("color-bg"),
      selected: token("color-fg"),
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- the theme is what these all read from
  }, [theme]);
}

/** A cluster's count, drawn here since text on a map would take glyphs, which are a request. */
function countImage(ink: string, label: string) {
  const scale = Math.max(2, Math.ceil(window.devicePixelRatio));
  const canvas = document.createElement("canvas");
  const context = canvas.getContext("2d");
  if (!context) return null;
  const font = `600 ${String(12 * scale)}px ${getComputedStyle(document.body).fontFamily}`;
  context.font = font;
  canvas.width = Math.ceil(context.measureText(label).width) + 2 * scale;
  canvas.height = 16 * scale;
  context.font = font;
  context.fillStyle = ink;
  context.textAlign = "center";
  context.textBaseline = "middle";
  context.fillText(label, canvas.width / 2, canvas.height / 2);
  return {
    image: {
      width: canvas.width,
      height: canvas.height,
      data: context.getImageData(0, 0, canvas.width, canvas.height).data,
    },
    scale,
  };
}

export interface MapCanvasProps {
  places: readonly Place[];
  file: MapFileShape | null;
  selected: Id | null;
  /** While set, a click places this and does nothing else. */
  picking: boolean;
  /** The map in words, for a screen reader. */
  summary: string;
  onSelect: (id: Id | null) => void;
  onPick: (lat: number, lon: number) => void;
  /** This machine cannot draw a map. */
  onFailed: () => void;
}

export function MapCanvas({
  places,
  file,
  selected,
  picking,
  summary,
  onSelect,
  onPick,
  onFailed,
}: MapCanvasProps) {
  const host = useRef<HTMLDivElement>(null);
  const mapRef = useRef<MapLibre | null>(null);
  const look = useMapLook();
  const style = useMemo(() => ({ look, land: theLand(), places, file }), [look, places, file]);
  // What the map's own callbacks see, so that they need not be bound again as these change.
  const now = useRef({ style, selected, picking, summary, onSelect, onPick, onFailed });
  useEffect(() => {
    now.current = { style, selected, picking, summary, onSelect, onPick, onFailed };
  });
  const fitted = useRef(false);

  const showAll = (animate: boolean) => {
    const map = mapRef.current;
    const bounds = boundsOf(now.current.style.places);
    if (!map || !bounds) return;
    map.fitBounds(bounds, {
      padding: Math.min(FIT_PADDING, map.getContainer().clientWidth / 4),
      maxZoom: fitZoom(now.current.style.file),
      animate,
    });
  };

  useEffect(() => {
    const container = host.current;
    if (!container) return;
    setWorkerUrl(workerUrl);
    let map: MapLibre;
    try {
      map = new MapLibre({
        container,
        attributionControl: false,
        center: [10, 20],
        zoom: 1,
        // At zoom 0 the world is wider than a phone, and fitting everything has to go further out.
        minZoom: -1,
        style: mapStyle({ ...now.current.style, selected: now.current.selected }),
      });
    } catch (error) {
      // Not a fault: the view says so and shows the list.
      console.warn(error);
      now.current.onFailed();
      return;
    }
    mapRef.current = map;
    map.getCanvas().setAttribute("aria-label", now.current.summary);

    map.setMissingStyleImageResolver((id) => {
      const [kind, ink, label] = id.split(":");
      if (kind !== COUNT_IMAGE || ink === undefined || label === undefined) return;
      const drawn = countImage(ink, label);
      if (drawn) map.addImage(id, drawn.image, { pixelRatio: drawn.scale });
    });

    const clickable = ["clusters", "points"];
    map.on("click", ({ lngLat, point }) => {
      if (now.current.picking) {
        const { lat, lng } = lngLat.wrap();
        now.current.onPick(lat, lng);
        return;
      }
      const hits = map.queryRenderedFeatures(
        [
          [point.x - REACH, point.y - REACH],
          [point.x + REACH, point.y + REACH],
        ],
        { layers: clickable },
      );
      const place = hits.find((hit) => hit.layer.id === "points");
      if (place) {
        now.current.onSelect(place.properties.id as Id);
        return;
      }
      const cluster = hits[0];
      if (!cluster) {
        now.current.onSelect(null);
        return;
      }
      const source = map.getSource<GeoJSONSource>("places");
      const geometry = cluster.geometry;
      if (!source || geometry.type !== "Point") return;
      void source
        .getClusterExpansionZoom(cluster.properties.cluster_id as number)
        .then((zoom) => {
          map.easeTo({ center: geometry.coordinates as [number, number], zoom });
        })
        .catch((error: unknown) => {
          console.error(error);
        });
    });
    for (const layer of clickable) {
      map.on("mouseenter", layer, () => {
        map.getCanvas().classList.add("is-over");
      });
      map.on("mouseleave", layer, () => {
        map.getCanvas().classList.remove("is-over");
      });
    }

    // What is on the map, for whoever is checking that it is: a canvas shows nothing else.
    map.on("idle", () => {
      const count = (layer: string) =>
        String(map.getLayer(layer) ? map.queryRenderedFeatures({ layers: [layer] }).length : 0);
      container.dataset.points = count("points");
      container.dataset.clusters = count("clusters");
      container.dataset.land = count("land");
      container.dataset.tiles = String(
        map.getStyle().layers.filter((layer) => layer.id.startsWith("file-")).length,
      );
      container.dataset.zoom = map.getZoom().toFixed(2);
      container.dataset.idle = "true";
    });
    map.on("error", ({ error }) => {
      // A tile that cannot be read leaves a hole and no more than that, so it is not the end of the map.
      console.warn(error.message);
    });
    return () => {
      mapRef.current = null;
      map.remove();
    };
  }, []);

  // The style follows the look, the places and the file; only what changed gets redrawn.
  useEffect(() => {
    const map = mapRef.current;
    if (!map) return;
    const apply = () => {
      map.setStyle(mapStyle({ ...style, selected: now.current.selected }));
      if (!fitted.current && style.places.length > 0) {
        fitted.current = true;
        showAll(false);
      }
    };
    if (map.isStyleLoaded() === true || map.loaded()) apply();
    else map.once("load", apply);
    return () => {
      map.off("load", apply);
    };
  }, [style]);

  useEffect(() => {
    const map = mapRef.current;
    if (map?.getLayer("selected")) map.setFilter("selected", selectedFilter(selected));
  }, [selected]);

  useEffect(() => {
    mapRef.current?.getCanvas().setAttribute("aria-label", summary);
  }, [summary]);

  const zoomBy = (by: number) => {
    mapRef.current?.zoomTo((mapRef.current.getZoom() || 0) + by);
  };
  const tools = ["Zoom in", "Zoom out", "Show all places"] as const;
  return (
    <div className={`dbme-map__stage${picking ? " is-picking" : ""}`}>
      <div ref={host} className="dbme-map__host" data-basemap={file ? "file" : "land"} />
      <div className="dbme-map__tools" role="group" aria-label="Map controls">
        <IconButton
          label={tools[0]}
          icon={Plus}
          onClick={() => {
            zoomBy(1);
          }}
        />
        <IconButton
          label={tools[1]}
          icon={Minus}
          onClick={() => {
            zoomBy(-1);
          }}
        />
        <IconButton
          label={tools[2]}
          icon={Maximize}
          disabled={places.length === 0}
          onClick={() => {
            showAll(true);
          }}
        />
      </div>
    </div>
  );
}
