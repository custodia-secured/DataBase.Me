// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * Icons. Only the icons named here reach the bundle: the 25 system types' icons (FORMAT.md §3.2,
 * by their Lucide names) and the icons the UI itself uses. A type whose icon is not known here
 * shows {@link FallbackTypeIcon}.
 */
import {
  AudioLines,
  BookOpen,
  Building2,
  Calendar,
  Clapperboard,
  Contact,
  File,
  FileText,
  FolderKanban,
  Globe,
  Highlighter,
  Image,
  Layers,
  LayoutTemplate,
  Library,
  Mail,
  MapPin,
  MessageSquare,
  NotebookPen,
  SquareCheck,
  StickyNote,
  Table,
  Tag,
  User,
  Video,
  type LucideIcon,
} from "lucide-react";

export type { LucideIcon } from "lucide-react";
export {
  ArrowDown,
  ArrowUp,
  Bold,
  ChevronDown,
  ChevronRight,
  Columns2,
  Ellipsis,
  FileText as DocumentIcon,
  FolderInput,
  GripVertical,
  Heading2,
  Italic,
  LayoutGrid,
  List,
  ListOrdered,
  ListTodo,
  ListTree,
  PanelLeft,
  Plus,
  Redo2,
  Search,
  SquareCode,
  Table2,
  TextQuote,
  Trash2,
  Undo2,
  X,
} from "lucide-react";

const TYPE_ICONS: Readonly<Record<string, LucideIcon>> = {
  "sticky-note": StickyNote,
  file: File,
  "square-check": SquareCheck,
  user: User,
  "building-2": Building2,
  "map-pin": MapPin,
  calendar: Calendar,
  "folder-kanban": FolderKanban,
  layers: Layers,
  "book-open": BookOpen,
  clapperboard: Clapperboard,
  globe: Globe,
  "file-text": FileText,
  image: Image,
  "audio-lines": AudioLines,
  video: Video,
  mail: Mail,
  "message-square": MessageSquare,
  contact: Contact,
  table: Table,
  tag: Tag,
  library: Library,
  "layout-template": LayoutTemplate,
  highlighter: Highlighter,
  "notebook-pen": NotebookPen,
};

/** The icon of an object with no type, or of a type whose icon is unknown. */
export const FallbackTypeIcon: LucideIcon = File;

/** The icon component for a type's icon name. */
export function typeIcon(name: string | null | undefined): LucideIcon {
  return (name ? TYPE_ICONS[name] : undefined) ?? FallbackTypeIcon;
}

/** The names of the icons {@link typeIcon} knows. */
export const TYPE_ICON_NAMES: readonly string[] = Object.keys(TYPE_ICONS);
