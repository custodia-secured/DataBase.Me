// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * The shell's UI state (ADR-0017): which object is open, which view each pane shows, whether
 * the panes are split, whether the phone drawer is open, whether the palette is open, and what
 * the results surface shows (ADR-0020). Vault data never lives here; it comes from the queries
 * in `@databaseme/client`.
 */
import type { Id } from "@databaseme/client";
import type { ResultsSource, ViewId } from "@databaseme/views";
import { create } from "zustand";

export interface ShellState {
  selected: Id | null;
  views: [ViewId, ViewId];
  split: boolean;
  drawer: boolean;
  palette: boolean;
  /** Whether the files folder dialog (settings, status and the Conflicts inbox) is open. */
  files: boolean;
  /** What the results surface shows in place of the panes, if anything. */
  results: ResultsSource | null;
  /**
   * The Import dialog (M6): closed when null, else open with these paths to start from (what
   * was dropped on the window, or none when opened from the header or the palette).
   */
  importing: string[] | null;
  /** Whether onboarding was asked for again from Settings or the palette (M7). */
  setupAgain: boolean;
  /** Whether the Settings dialog is open (M7). */
  settingsOpen: boolean;
  select: (id: Id | null) => void;
  setView: (pane: 0 | 1, view: ViewId) => void;
  setSplit: (split: boolean) => void;
  setDrawer: (open: boolean) => void;
  setPalette: (open: boolean) => void;
  setFiles: (open: boolean) => void;
  setImporting: (paths: string[] | null) => void;
  setSetupAgain: (again: boolean) => void;
  setSettingsOpen: (open: boolean) => void;
  showResults: (results: ResultsSource | null) => void;
}

export const useShell = create<ShellState>()((set) => ({
  selected: null,
  views: ["document", "table"],
  split: false,
  drawer: false,
  palette: false,
  files: false,
  results: null,
  importing: null,
  setupAgain: false,
  settingsOpen: false,
  select: (id) => {
    set({ selected: id, drawer: false, results: null });
  },
  setView: (pane, view) => {
    set((state) => {
      const views: [ViewId, ViewId] = [...state.views];
      views[pane] = view;
      return { views, results: null };
    });
  },
  setSplit: (split) => {
    set({ split });
  },
  setDrawer: (open) => {
    set({ drawer: open });
  },
  setPalette: (open) => {
    set({ palette: open });
  },
  setFiles: (open) => {
    set({ files: open });
  },
  setImporting: (paths) => {
    set({ importing: paths });
  },
  setSetupAgain: (again) => {
    set({ setupAgain: again });
  },
  setSettingsOpen: (open) => {
    set({ settingsOpen: open });
  },
  showResults: (results) => {
    set({ results, drawer: false });
  },
}));
