// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * The view registry (handoff brief §8). Every view shows one object and what belongs to it; the
 * shell picks one per pane. Later milestones and plugins add views here.
 */
import type { Id, Mode } from "@databaseme/client";
import {
  DocumentIcon,
  LayoutGrid,
  ListTree,
  MapPin,
  LayoutDashboard,
  PenLine,
  Network,
  CalendarDays,
  ChartColumn,
  ChartGantt,
  SquareKanban,
  Table2,
  Waypoints,
  type LucideIcon,
} from "@databaseme/ui";
import { lazy, type ComponentType } from "react";
import { DocumentView } from "./document/DocumentView";
import { GalleryView } from "./gallery/GalleryView";
import { MindMapView } from "./mindmap/MindMapView";
import { OutlineView } from "./outline/OutlineView";
import { TableView } from "./table/TableView";

export interface ViewProps {
  /** The object the view shows. */
  objectId: Id;
  /** Opens another object. */
  onOpen: (id: Id) => void;
}

export type ViewId =
  | "document"
  | "outline"
  | "table"
  | "gallery"
  | "map"
  | "graph"
  | "canvas"
  | "mindmap"
  | "kanban"
  | "calendar"
  | "timeline"
  | "charts"
  | "dashboard";

export interface ViewDefinition {
  id: ViewId;
  label: string;
  icon: LucideIcon;
  component: ComponentType<ViewProps>;
  /** Always a tab in the pane; the rest are in its "More views" menu until one is chosen (M8). */
  pinned?: boolean;
}

const DOCUMENT: ViewDefinition = {
  id: "document",
  label: "Document",
  icon: DocumentIcon,
  component: DocumentView,
  pinned: true,
};

export const VIEWS: readonly ViewDefinition[] = [
  DOCUMENT,
  { id: "outline", label: "Outline", icon: ListTree, component: OutlineView, pinned: true },
  { id: "table", label: "Table", icon: Table2, component: TableView, pinned: true },
  { id: "gallery", label: "Gallery", icon: LayoutGrid, component: GalleryView, pinned: true },
  { id: "mindmap", label: "Mind map", icon: Network, component: MindMapView, pinned: true },
  {
    id: "kanban",
    label: "Kanban",
    icon: SquareKanban,
    component: lazy(() =>
      import("./kanban/KanbanView").then((module) => ({ default: module.KanbanView })),
    ),
  },
  {
    id: "calendar",
    label: "Calendar",
    icon: CalendarDays,
    component: lazy(() =>
      import("./calendar/CalendarView").then((module) => ({ default: module.CalendarView })),
    ),
  },
  {
    id: "timeline",
    label: "Timeline",
    icon: ChartGantt,
    component: lazy(() =>
      import("./timeline/TimelineView").then((module) => ({ default: module.TimelineView })),
    ),
  },
  {
    id: "charts",
    label: "Charts",
    icon: ChartColumn,
    component: lazy(() =>
      import("./charts/ChartsView").then((module) => ({ default: module.ChartsView })),
    ),
  },
  {
    id: "map",
    label: "Map",
    icon: MapPin,
    component: lazy(() => import("./map/MapView").then((module) => ({ default: module.MapView }))),
  },
  {
    id: "graph",
    label: "Graph",
    icon: Waypoints,
    component: lazy(() =>
      import("./graph/GraphView").then((module) => ({ default: module.GraphView })),
    ),
  },
  {
    id: "canvas",
    label: "Canvas",
    icon: PenLine,
    component: lazy(() =>
      import("./canvas/CanvasView").then((module) => ({ default: module.CanvasView })),
    ),
  },
  {
    id: "dashboard",
    label: "Dashboard",
    icon: LayoutDashboard,
    component: lazy(() =>
      import("./dashboard/DashboardView").then((module) => ({ default: module.DashboardView })),
    ),
  },
];

export function viewById(id: ViewId): ViewDefinition {
  return VIEWS.find((view) => view.id === id) ?? DOCUMENT;
}

/** The views Simple mode leaves out (M7, M8): they need types and fields to mean much. */
const HIDDEN_IN_SIMPLE: readonly ViewId[] = [
  "table",
  "map",
  "graph",
  "canvas",
  "kanban",
  "calendar",
  "timeline",
  "charts",
  "dashboard",
];

/** The views a pane offers in `mode`, in the registry's order. */
export function visibleViews(mode: Mode): readonly ViewDefinition[] {
  return mode === "organizer" ? VIEWS : VIEWS.filter((view) => !HIDDEN_IN_SIMPLE.includes(view.id));
}
