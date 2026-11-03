// SPDX-License-Identifier: AGPL-3.0-or-later
export { actions, flushEditors, onActionError, registerFlusher, reportError } from "./actions";
export { BlockEditor, type BlockEditorProps } from "./editor/BlockEditor";
export { FilesDialog, type FilesDialogProps } from "./files/FilesDialog";
export { HoldBanner, type HoldBannerProps } from "./files/HoldBanner";
export { ImportDialog, type ImportDialogProps } from "./import/ImportDialog";
export { JOURNAL_TITLE, showsTypes, todayTitle, useMode, type Mode } from "./mode";
export { MoveDialog } from "./sidebar/MoveDialog";
export { Onboarding, type OnboardingProps } from "./onboarding/Onboarding";
export { vaultIn } from "./onboarding/paths";
export { SettingsDialog, type SettingsDialogProps } from "./settings/SettingsDialog";
export { Sidebar, type SidebarProps } from "./sidebar/Sidebar";
export {
  VIEWS,
  viewById,
  visibleViews,
  type ViewDefinition,
  type ViewId,
  type ViewProps,
} from "./registry";
export {
  CommandPalette,
  type CommandPaletteProps,
  type PaletteCommand,
} from "./search/CommandPalette";
export { ResultsView, type ResultsSource, type ResultsViewProps } from "./search/ResultsView";
