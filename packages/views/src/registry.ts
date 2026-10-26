// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * The view registry (handoff brief §8). Every view shows one object and what belongs to it; the
 * shell picks one per pane. Later milestones and plugins add views here.
 */
import type { Id } from "@databaseme/client";
import { DocumentIcon, LayoutGrid, ListTree, Table2, type LucideIcon } from "@databaseme/ui";
import type { ComponentType } from "react";
import { DocumentView } from "./document/DocumentView";
import { GalleryView } from "./gallery/GalleryView";
import { OutlineView } from "./outline/OutlineView";
import { TableView } from "./table/TableView";

export interface ViewProps {
  /** The object the view shows. */
  objectId: Id;
  /** Opens another object. */
  onOpen: (id: Id) => void;
}

export type ViewId = "document" | "outline" | "table" | "gallery";

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
];

export function viewById(id: ViewId): ViewDefinition {
  return VIEWS.find((view) => view.id === id) ?? DOCUMENT;
}
