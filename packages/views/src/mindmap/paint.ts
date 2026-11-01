// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * What the overview draws (ADR-0019): the whole map as plain boxes on one canvas, for when it is too
 * far away to read. Everything of one kind goes in one path, which is what makes it cheap.
 */
import type { Id } from "@databaseme/client";
import { NODE_HEIGHT, NODE_WIDTH, type Drop, type MapLayout } from "./layout";

/** Below this zoom a box is too small to outline. */
const OUTLINE_ZOOM = 0.1;

export interface OverviewColors {
  branch: string;
  relation: string;
  box: string;
  outline: string;
  active: string;
}

export interface OverviewScene {
  layout: MapLayout;
  relations: readonly { src_id: Id; dst_id: Id }[];
  active: Id;
  dragged: { id: Id; x: number; y: number } | null;
  drop: Drop | null;
}

/** The parts of a canvas context the overview draws with. */
export type OverviewPainter = Pick<
  CanvasRenderingContext2D,
  | "beginPath"
  | "clearRect"
  | "fill"
  | "fillRect"
  | "lineTo"
  | "moveTo"
  | "rect"
  | "setLineDash"
  | "stroke"
> & {
  fillStyle: unknown;
  strokeStyle: unknown;
  lineWidth: number;
};

/**
 * Draws `scene` for a view of `width` by `height` at `[x, y, zoom]`, and returns how many boxes it
 * drew. Boxes outside the view are skipped.
 */
export function paintOverview(
  context: OverviewPainter,
  scene: OverviewScene,
  [tx, ty, zoom]: readonly [number, number, number],
  width: number,
  height: number,
  colors: OverviewColors,
): number {
  const { layout, active, dragged, drop } = scene;
  const placed = (id: Id) => {
    if (dragged?.id === id) return dragged;
    return layout.byId.get(id) ?? null;
  };
  const boxWidth = Math.max(1, NODE_WIDTH * zoom);
  const boxHeight = Math.max(1, NODE_HEIGHT * zoom);
  context.clearRect(0, 0, width, height);

  context.beginPath();
  context.setLineDash([]);
  for (const node of layout.nodes) {
    const from = node.parent === null ? null : placed(node.parent);
    const to = placed(node.id);
    if (!from || !to) continue;
    context.moveTo((from.x + NODE_WIDTH) * zoom + tx, (from.y + NODE_HEIGHT / 2) * zoom + ty);
    context.lineTo(to.x * zoom + tx, (to.y + NODE_HEIGHT / 2) * zoom + ty);
  }
  context.strokeStyle = colors.branch;
  context.lineWidth = 1;
  context.stroke();

  context.beginPath();
  context.setLineDash([4, 3]);
  for (const relation of scene.relations) {
    const from = placed(relation.src_id);
    const to = placed(relation.dst_id);
    if (!from || !to) continue;
    context.moveTo((from.x + NODE_WIDTH) * zoom + tx, (from.y + NODE_HEIGHT / 2) * zoom + ty);
    context.lineTo(to.x * zoom + tx, (to.y + NODE_HEIGHT / 2) * zoom + ty);
  }
  context.strokeStyle = colors.relation;
  context.stroke();
  context.setLineDash([]);

  let drawn = 0;
  context.beginPath();
  for (const node of layout.nodes) {
    const at = placed(node.id);
    if (!at) continue;
    const left = at.x * zoom + tx;
    const top = at.y * zoom + ty;
    if (left + boxWidth < 0 || top + boxHeight < 0 || left > width || top > height) continue;
    drawn++;
    context.rect(left, top, boxWidth, boxHeight);
  }
  context.fillStyle = zoom < OUTLINE_ZOOM ? colors.outline : colors.box;
  context.fill();
  if (zoom >= OUTLINE_ZOOM) {
    context.strokeStyle = colors.outline;
    context.stroke();
  }

  context.fillStyle = colors.active;
  for (const id of [active, drop?.target]) {
    const at = id ? placed(id) : null;
    if (at) context.fillRect(at.x * zoom + tx, at.y * zoom + ty, boxWidth, boxHeight);
  }
  return drawn;
}
