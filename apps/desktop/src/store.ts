// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * The shell's UI state (ADR-0017): which object is open, which view each pane shows, whether
 * the panes are split and whether the phone drawer is open. Vault data never lives here; it
 * comes from the queries in `@databaseme/client`.
 */
import type { Id } from "@databaseme/client";
import type { ViewId } from "@databaseme/views";
import { create } from "zustand";

export interface ShellState {
  selected: Id | null;
  views: [ViewId, ViewId];
  split: boolean;
  drawer: boolean;
  select: (id: Id | null) => void;
  setView: (pane: 0 | 1, view: ViewId) => void;
  setSplit: (split: boolean) => void;
  setDrawer: (open: boolean) => void;
}

export const useShell = create<ShellState>()((set) => ({
  selected: null,
  views: ["document", "table"],
  split: false,
  drawer: false,
  select: (id) => {
    set({ selected: id, drawer: false });
  },
  setView: (pane, view) => {
    set((state) => {
      const views: [ViewId, ViewId] = [...state.views];
      views[pane] = view;
      return { views };
    });
  },
  setSplit: (split) => {
    set({ split });
  },
  setDrawer: (open) => {
    set({ drawer: open });
  },
}));
