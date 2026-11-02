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
  /** What the results surface shows in place of the panes, if anything. */
  results: ResultsSource | null;
  select: (id: Id | null) => void;
  setView: (pane: 0 | 1, view: ViewId) => void;
  setSplit: (split: boolean) => void;
  setDrawer: (open: boolean) => void;
  setPalette: (open: boolean) => void;
  showResults: (results: ResultsSource | null) => void;
}

export const useShell = create<ShellState>()((set) => ({
  selected: null,
  views: ["document", "table"],
  split: false,
  drawer: false,
  palette: false,
  results: null,
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
  showResults: (results) => {
    set({ results, drawer: false });
  },
}));
