// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * The theme (M7, ADR-0027): `data-theme` on `<html>` picks the light or dark tokens regardless
 * of the OS; no attribute leaves the choice to `prefers-color-scheme` (tokens.css). The chosen
 * theme is also kept in `localStorage` as a hint, applied before the UI mounts, so the first
 * paint already has the right colours; the settings, read over IPC a moment later, confirm or
 * correct it.
 */

/** What a person can choose. The settings' `Theme` is this union, in its JSON form. */
export type ThemeChoice = "system" | "light" | "dark";

/** Where the hint is kept. */
export const THEME_HINT_KEY = "dbme-theme";

const CHOICES: readonly ThemeChoice[] = ["system", "light", "dark"];

function isChoice(value: unknown): value is ThemeChoice {
  return typeof value === "string" && (CHOICES as readonly string[]).includes(value);
}

/** Sets the theme on the document and remembers it as the hint. */
export function applyTheme(theme: ThemeChoice, root: HTMLElement = document.documentElement): void {
  if (theme === "system") delete root.dataset.theme;
  else root.dataset.theme = theme;
  try {
    localStorage.setItem(THEME_HINT_KEY, theme);
  } catch {
    // Storage can be unavailable (a private window, a locked-down webview); the setting still
    // arrives over IPC.
  }
}

/** The theme the hint names, or `system` when there is none or it is not a choice. */
export function readThemeHint(): ThemeChoice {
  try {
    const hint: unknown = localStorage.getItem(THEME_HINT_KEY);
    return isChoice(hint) ? hint : "system";
  } catch {
    return "system";
  }
}

/** The theme in effect: the chosen one, or what the OS asks for under `system`. */
export function effectiveTheme(theme: ThemeChoice): "light" | "dark" {
  if (theme !== "system") return theme;
  return typeof matchMedia === "function" && matchMedia("(prefers-color-scheme: dark)").matches
    ? "dark"
    : "light";
}
