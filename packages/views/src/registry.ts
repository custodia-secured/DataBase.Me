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
  Network,
  Table2,
  type LucideIcon,
} from "@databaseme/ui";
import type { ComponentType } from "react";
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

export type ViewId = "document" | "outline" | "table" | "gallery" | "mindmap";

export interface ViewDefinition {
  id: ViewId;
  label: string;
  icon: LucideIcon;
  component: ComponentType<ViewProps>;
}

const DOCUMENT: ViewDefinition = {
  id: "document",
  label: "Document",
  icon: DocumentIcon,
  component: DocumentView,
};

export const VIEWS: readonly ViewDefinition[] = [
  DOCUMENT,
  { id: "outline", label: "Outline", icon: ListTree, component: OutlineView },
  { id: "table", label: "Table", icon: Table2, component: TableView },
  { id: "gallery", label: "Gallery", icon: LayoutGrid, component: GalleryView },
  { id: "mindmap", label: "Mind map", icon: Network, component: MindMapView },
];

export function viewById(id: ViewId): ViewDefinition {
  return VIEWS.find((view) => view.id === id) ?? DOCUMENT;
}

/** The views Simple mode leaves out (M7): the Table view needs types and fields to mean much. */
const HIDDEN_IN_SIMPLE: readonly ViewId[] = ["table"];

/** The views a pane offers in `mode`, in the registry's order. */
export function visibleViews(mode: Mode): readonly ViewDefinition[] {
  return mode === "organizer" ? VIEWS : VIEWS.filter((view) => !HIDDEN_IN_SIMPLE.includes(view.id));
}
