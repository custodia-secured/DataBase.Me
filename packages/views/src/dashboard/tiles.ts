// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * A dashboard's tiles (M8, Task 8), apart from drawing them. A dashboard is a saved view whose settings
 * are these, so changing one is a change to its settings, kept when it is saved like any other view's.
 */
import type { Id } from "@databaseme/client";

export interface Tile {
  /** The saved view it shows. */
  view: Id;
  /** Across the whole grid, rather than one column of it. */
  wide: boolean;
}

export interface DashboardOptions {
  tiles: readonly Tile[];
}

/** The tiles in saved settings. Settings come from files too, so what is not a tile is dropped. */
export function tilesOf(options: Partial<DashboardOptions> | undefined): Tile[] {
  const tiles: unknown = options?.tiles;
  if (!Array.isArray(tiles)) return [];
  const seen = new Set<Id>();
  const result: Tile[] = [];
  for (const tile of tiles as unknown[]) {
    if (typeof tile !== "object" || tile === null) continue;
    const { view, wide } = tile as { view?: unknown; wide?: unknown };
    if (typeof view !== "string" || seen.has(view)) continue;
    seen.add(view);
    result.push({ view, wide: wide === true });
  }
  return result;
}

/** With a saved view added at the end. It shows once. */
export function withTile(tiles: readonly Tile[], view: Id): Tile[] {
  return tiles.some((tile) => tile.view === view) ? [...tiles] : [...tiles, { view, wide: false }];
}

export function withoutTile(tiles: readonly Tile[], view: Id): Tile[] {
  return tiles.filter((tile) => tile.view !== view);
}

/** With a tile `by` places along. It stops at either end. */
export function movedTile(tiles: readonly Tile[], view: Id, by: number): Tile[] {
  const at = tiles.findIndex((tile) => tile.view === view);
  const tile = tiles[at];
  if (!tile) return [...tiles];
  const to = Math.min(tiles.length - 1, Math.max(0, at + by));
  const rest = tiles.filter((_, index) => index !== at);
  return [...rest.slice(0, to), tile, ...rest.slice(to)];
}

export function widenedTile(tiles: readonly Tile[], view: Id): Tile[] {
  return tiles.map((tile) => (tile.view === view ? { ...tile, wide: !tile.wide } : tile));
}
