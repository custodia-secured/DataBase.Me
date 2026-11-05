// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * Stands in for `@excalidraw/mermaid-to-excalidraw` (see `vite-plugin-excalidraw.ts`). Excalidraw
 * imports it only when someone opens its Mermaid dialog, which the app does not offer.
 */
export function parseMermaidToExcalidraw(): Promise<never> {
  return Promise.reject(new Error("The Mermaid importer is not part of DataBase.Me."));
}
