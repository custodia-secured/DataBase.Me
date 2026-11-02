// SPDX-License-Identifier: AGPL-3.0-or-later
export { actions, flushEditors, onActionError, registerFlusher, reportError } from "./actions";
export { BlockEditor, type BlockEditorProps } from "./editor/BlockEditor";
export { MoveDialog } from "./sidebar/MoveDialog";
export { Sidebar, type SidebarProps } from "./sidebar/Sidebar";
export { VIEWS, viewById, type ViewDefinition, type ViewId, type ViewProps } from "./registry";
export {
  CommandPalette,
  type CommandPaletteProps,
  type PaletteCommand,
} from "./search/CommandPalette";
export { ResultsView, type ResultsSource, type ResultsViewProps } from "./search/ResultsView";
