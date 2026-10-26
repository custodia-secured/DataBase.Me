// SPDX-License-Identifier: AGPL-3.0-or-later
import { useSyncExternalStore } from "react";

/** Whether the CSS media query `query` matches, kept current as the window changes. */
export function useMediaQuery(query: string): boolean {
  // Environments without media queries (jsdom) match nothing.
  const supported = typeof window.matchMedia === "function";
  return useSyncExternalStore(
    (onChange) => {
      if (!supported) return () => undefined;
      const list = window.matchMedia(query);
      list.addEventListener("change", onChange);
      return () => {
        list.removeEventListener("change", onChange);
      };
    },
    () => supported && window.matchMedia(query).matches,
    () => false,
  );
}
