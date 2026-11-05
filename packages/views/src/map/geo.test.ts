// SPDX-License-Identifier: AGPL-3.0-or-later
import type { ObjectSummary_Serialize } from "@databaseme/client";
import { feature } from "topojson-client";
import type { GeometryCollection, Topology } from "topojson-specification";
import { describe, expect, test } from "vitest";
import landAtlas from "world-atlas/land-110m.json";
import {
  boundsOf,
  degrees,
  fitZoom,
  flatLand,
  flatRing,
  mapStyle,
  parseCoordinate,
  placeLabel,
  placesData,
  placesOf,
  placesSummary,
  selectedFilter,
  type MapFileShape,
  type MapLook,
} from "./geo";

const summary = (id: string, title: string, lat: number | null, lon: number | null) =>
  ({ object: { id, title, geo_lat: lat, geo_lon: lon } }) as unknown as ObjectSummary_Serialize;

const look: MapLook = {
  sea: "#111111",
  land: "#222222",
  line: "#333333",
  point: "#444444",
  onPoint: "#555555",
  surface: "#666666",
  selected: "#777777",
};
const land: GeoJSON.FeatureCollection = { type: "FeatureCollection", features: [] };
const places = [{ id: "a", title: "A", lat: 1, lon: 2 }];
const file = (change: Partial<MapFileShape>): MapFileShape => ({
  key: "map-file-1",
  kind: "vector",
  minZoom: 0,
  maxZoom: 9,
  layers: [],
  ...change,
});

describe("placesOf", () => {
  test("keeps what has a location, and names what has no title", () => {
    expect(
      placesOf([
        summary("a", "Here", 1, 2),
        summary("b", "", 0, 0),
        summary("c", "None", null, null),
      ]),
    ).toEqual([
      { id: "a", title: "Here", lat: 1, lon: 2 },
      { id: "b", title: "Untitled", lat: 0, lon: 0 },
    ]);
  });
});

describe("parseCoordinate", () => {
  test.each<[string, number]>([
    ["47.5", 47.5],
    [" -8.25 ", -8.25],
    ["+8", 8],
    ["47,5", 47.5],
    [".5", 0.5],
    ["90", 90],
    ["-90", -90],
    ["0", 0],
  ])("reads %j", (text, value) => {
    expect(parseCoordinate(text, 90)).toBe(value);
  });

  test.each(["", " ", "abc", "1e2", "12 34", "1.2.3", "90.01", "-90.01", "NaN", "Infinity", "--1"])(
    "refuses %j",
    (text) => {
      expect(parseCoordinate(text, 90)).toBeNull();
    },
  );

  test("the limit is the one asked for", () => {
    expect(parseCoordinate("120", 180)).toBe(120);
    expect(parseCoordinate("120", 90)).toBeNull();
  });
});

describe("what is written", () => {
  test("five places and no zeros that say nothing", () => {
    expect(degrees(47.5)).toBe("47.5");
    expect(degrees(8.123456789)).toBe("8.12346");
    expect(degrees(0)).toBe("0");
    expect(placeLabel(47.5, -8.25)).toBe("47.5, -8.25");
  });

  test("how many are on the map and how many are not", () => {
    expect(placesSummary(1, 0)).toBe("1 place on the map.");
    expect(placesSummary(2000, 1)).toBe("2,000 places on the map. 1 item has no location.");
    expect(placesSummary(0, 3)).toBe("0 places on the map. 3 items have no location.");
  });
});

describe("boundsOf", () => {
  test("nothing has none", () => {
    expect(boundsOf([])).toBeNull();
  });

  test("runs from the lowest of each to the highest", () => {
    expect(
      boundsOf([
        { lat: 10, lon: -5 },
        { lat: -20, lon: 30 },
        { lat: 5, lon: 1 },
      ]),
    ).toEqual([
      [-5, -20],
      [30, 10],
    ]);
  });
});

describe("fitZoom", () => {
  test("goes no further in than there is anything to see", () => {
    expect(fitZoom(null)).toBe(4);
    expect(fitZoom(file({ maxZoom: 9 }))).toBe(9);
    expect(fitZoom(file({ maxZoom: 20 }))).toBe(14);
  });
});

describe("the land on a flat map", () => {
  const atlas = landAtlas as unknown as Topology<{ land: GeometryCollection }>;
  const shipped = feature(atlas, atlas.objects.land);
  const ringsOf = (land: GeoJSON.GeoJSON): GeoJSON.Position[][] => {
    const geometries =
      land.type === "FeatureCollection"
        ? land.features.map((one) => one.geometry)
        : land.type === "Feature"
          ? [land.geometry]
          : [land];
    return geometries.flatMap((geometry) => {
      if (geometry.type === "Polygon") return geometry.coordinates;
      if (geometry.type === "MultiPolygon") return geometry.coordinates.flat();
      return [];
    });
  };
  const longestStep = (ring: readonly GeoJSON.Position[]) =>
    ring.reduce(
      (most, [lon = 0], at) => Math.max(most, Math.abs(lon - (ring[at - 1]?.[0] ?? lon))),
      0,
    );

  test("what ships does cross the map in one step, which is what this is for", () => {
    expect(Math.max(...ringsOf(shipped).map(longestStep))).toBeGreaterThan(180);
  });

  test("nothing crosses the map in one step once it is flat", () => {
    const rings = ringsOf(flatLand(shipped));
    expect(rings.length).toBeGreaterThan(50);
    // A ring that goes round a pole closes along it, a whole turn in one step, and only there.
    const across = rings.filter((ring) => longestStep(ring) > 180);
    expect(across).toHaveLength(1);
    const pole = across[0] ?? [];
    expect(pole.some(([, lat]) => lat === -90)).toBe(true);
    for (const ring of rings) {
      if (ring === pole) continue;
      expect(ring.some(([, lat]) => lat === 90 || lat === -90)).toBe(false);
    }
  });

  test("every ring is in the world most of it is in", () => {
    for (const ring of ringsOf(flatLand(shipped))) {
      const lons = ring.map(([lon = 0]) => lon);
      const middle = (Math.min(...lons) + Math.max(...lons)) / 2;
      expect(Math.abs(middle)).toBeLessThanOrEqual(180);
    }
  });

  test("a hole stays inside the ring around it, or the two are joined across the Earth", () => {
    const polygonsOf = (land: GeoJSON.GeoJSON): GeoJSON.Position[][][] => {
      if (land.type !== "FeatureCollection") throw new Error("the land ships as a collection");
      return land.features.flatMap(({ geometry }) =>
        geometry.type === "MultiPolygon" ? geometry.coordinates : [],
      );
    };
    const holed = polygonsOf(flatLand(shipped)).filter((polygon) => polygon.length > 1);
    expect(holed.length).toBeGreaterThan(0);
    for (const [outer = [], ...holes] of holed) {
      const lons = outer.map(([lon = 0]) => lon);
      for (const hole of holes) {
        for (const [lon = 0] of hole) {
          expect(lon).toBeGreaterThanOrEqual(Math.min(...lons));
          expect(lon).toBeLessThanOrEqual(Math.max(...lons));
        }
      }
    }
  });

  test("Eurasia runs from Europe past the antimeridian, though its ring starts beyond it", () => {
    const rings = ringsOf(flatLand(shipped));
    const eurasia = rings.reduce((most, ring) => (ring.length > most.length ? ring : most), []);
    const lons = eurasia.map(([lon = 0]) => lon);
    expect(Math.min(...lons)).toBeGreaterThan(-20);
    expect(Math.min(...lons)).toBeLessThan(0);
    expect(Math.max(...lons)).toBeGreaterThan(180);
    expect(Math.max(...lons)).toBeLessThan(195);
  });

  test("every ring still ends where it began", () => {
    for (const ring of ringsOf(flatLand(shipped))) {
      expect(ring.at(-1)).toEqual(ring[0]);
    }
  });

  test("a ring that crosses the antimeridian carries on past it", () => {
    expect(
      flatRing([
        [170, 60],
        [180, 65],
        [-180, 65],
        [-170, 60],
        [-180, 55],
        [180, 55],
        [170, 60],
      ]),
    ).toEqual([
      [170, 60],
      [180, 65],
      [180, 65],
      [190, 60],
      [180, 55],
      [180, 55],
      [170, 60],
    ]);
  });

  test("a ring that starts beyond the antimeridian comes back to the world it is mostly in", () => {
    expect(
      flatRing([
        [-175, 65],
        [-180, 65],
        [170, 65],
        [170, 60],
        [-180, 60],
        [-175, 65],
      ]),
    ).toEqual([
      [185, 65],
      [180, 65],
      [170, 65],
      [170, 60],
      [180, 60],
      [185, 65],
    ]);
  });

  test("a ring that goes round a pole closes along that pole", () => {
    const ring = flatRing([
      [-180, -70],
      [-60, -70],
      [60, -70],
      [180, -70],
      [-180, -70],
    ]);
    expect(ring.slice(-3)).toEqual([
      [180, -90],
      [-180, -90],
      [-180, -70],
    ]);
  });

  test("a ring anywhere else is left as it is", () => {
    const ring = [
      [1, 1],
      [2, 1],
      [2, 2],
      [1, 1],
    ];
    expect(flatRing(ring)).toEqual(ring);
  });
});

describe("mapStyle", () => {
  const style = (change: { file?: MapFileShape; selected?: string } = {}) =>
    mapStyle({
      look,
      land,
      places,
      selected: change.selected ?? null,
      file: change.file ?? null,
    });

  test("asks for nothing from anywhere", () => {
    const text = JSON.stringify(style());
    expect(text).not.toMatch(/https?:/);
    expect(style().glyphs).toBeUndefined();
    expect(style().sprite).toBeUndefined();
  });

  test("no layer draws text, which would take glyphs", () => {
    const fileStyle = style({ file: file({ layers: ["roads", "water"] }) });
    for (const layer of [...style().layers, ...fileStyle.layers]) {
      expect(JSON.stringify(layer)).not.toContain("text-field");
    }
  });

  test("the places cluster, and only they do", () => {
    expect(style().sources.places).toMatchObject({ type: "geojson", cluster: true });
    expect(style().sources.land).not.toHaveProperty("cluster");
  });

  test("a place's feature carries who it is, in lon, lat order", () => {
    expect(placesData(places).features[0]).toEqual({
      type: "Feature",
      properties: { id: "a", title: "A" },
      geometry: { type: "Point", coordinates: [2, 1] },
    });
  });

  test("the selected layer follows the selection, and nothing matches when there is none", () => {
    expect(selectedFilter("a")).toEqual(["==", ["get", "id"], "a"]);
    expect(selectedFilter(null)).toEqual(["==", ["get", "id"], ""]);
    expect(style({ selected: "a" }).layers.find((layer) => layer.id === "selected")).toMatchObject({
      filter: selectedFilter("a"),
    });
  });

  test("the places are drawn over the land, and a file between them", () => {
    const ids = style({ file: file({ kind: "raster" }) }).layers.map((layer) => layer.id);
    expect(ids.indexOf("land")).toBeLessThan(ids.indexOf("file-tiles"));
    expect(ids.indexOf("file-tiles")).toBeLessThan(ids.indexOf("clusters"));
    expect(ids.indexOf("clusters")).toBeLessThan(ids.indexOf("points"));
    expect(ids.indexOf("points")).toBeLessThan(ids.indexOf("selected"));
  });

  test("no file, no file source", () => {
    expect(style().sources).not.toHaveProperty("file");
  });

  test("a raster file is a raster source at its own zooms", () => {
    expect(style({ file: file({ kind: "raster", minZoom: 2, maxZoom: 5 }) }).sources.file).toEqual({
      type: "raster",
      tiles: ["pmtiles://map-file-1/{z}/{x}/{y}"],
      tileSize: 256,
      minzoom: 2,
      maxzoom: 5,
    });
  });

  test("a vector file has each layer drawn, and what is water is drawn as water", () => {
    const layers = style({ file: file({ layers: ["earth", "Water"] }) }).layers;
    const fill = (name: string) =>
      layers.find((layer) => layer.type === "fill" && layer["source-layer"] === name);
    expect(fill("earth")).toMatchObject({ paint: { "fill-color": look.land } });
    expect(fill("Water")).toMatchObject({ paint: { "fill-color": look.sea } });
    expect(layers.filter((layer) => layer.id.startsWith("file-line-"))).toHaveLength(2);
  });

  test("however many layers a file has, a few are drawn", () => {
    const many = Array.from({ length: 200 }, (_, at) => `layer${String(at)}`);
    const drawn = style({ file: file({ layers: many }) }).layers.filter((layer) =>
      layer.id.startsWith("file-"),
    );
    expect(drawn).toHaveLength(48);
  });
});
