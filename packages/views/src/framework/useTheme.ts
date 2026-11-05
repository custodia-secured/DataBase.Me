// SPDX-License-Identifier: AGPL-3.0-or-later
import { useSyncExternalStore } from "react";

/** Whichever theme is showing, as what changes when it does. */
function themeSnapshot(): string {
  return getComputedStyle(document.documentElement).getPropertyValue("--dbme-color-bg");
}

function subscribeToTheme(onChange: () => void): () => void {
  const observer = new MutationObserver(onChange);
  observer.observe(document.documentElement, {
    attributes: true,
    attributeFilter: ["data-theme"],
  });
  const media = window.matchMedia("(prefers-color-scheme: dark)");
  media.addEventListener("change", onChange);
  return () => {
    observer.disconnect();
    media.removeEventListener("change", onChange);
  };
}

function darkSnapshot(): boolean {
  const { theme } = document.documentElement.dataset;
  return theme === undefined
    ? window.matchMedia("(prefers-color-scheme: dark)").matches
    : theme === "dark";
}

/** Whether the dark theme is showing, for a library that takes a theme by name. */
export function useIsDark(): boolean {
  return useSyncExternalStore(subscribeToTheme, darkSnapshot);
}

/**
 * The page's background colour, which changes when the theme does. For what is drawn in colours read
 * from the tokens, in a canvas or a library that cannot use a stylesheet's.
 */
export function useThemeKey(): string {
  return useSyncExternalStore(subscribeToTheme, themeSnapshot);
}
