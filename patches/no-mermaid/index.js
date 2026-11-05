// SPDX-License-Identifier: AGPL-3.0-or-later
// See package.json. Excalidraw imports this only when its Mermaid dialog opens, which the app does
// not offer, so nothing is expected to call it.
export function parseMermaidToExcalidraw() {
  return Promise.reject(new Error("The Mermaid importer is not part of DataBase.Me."));
}
