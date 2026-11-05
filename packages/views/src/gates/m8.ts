// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * M8's gates (docs/milestones/M8.md, Task 1): each one loads a library the plan names and makes it
 * do its real work under the app's CSP, so the e2e fixtures' checks (no CSP violation, no error, no
 * request leaving the machine) say whether the library can come in at all. Each returns the figures
 * the gate records.
 */
type Result = Record<string, unknown>;

export function runGate(root: HTMLElement) {
  return gateCanvas(root);
}

/** Excalidraw with a shape and a card-like embeddable, asked to draw and to give its elements. */
async function gateCanvas(root: HTMLElement): Promise<Result> {
  const started = performance.now();
  window.EXCALIDRAW_ASSET_PATH = "/excalidraw-assets/";
  const [React, { createRoot }, excalidraw] = await Promise.all([
    import("react"),
    import("react-dom/client"),
    import("@excalidraw/excalidraw"),
    import("@excalidraw/excalidraw/index.css"),
  ]);
  const loaded = performance.now() - started;
  const elements = excalidraw.convertToExcalidrawElements([
    { type: "rectangle", x: 40, y: 40, width: 200, height: 120 },
    { type: "text", x: 60, y: 80, text: "Card" },
  ]);
  const api = await new Promise<{ getSceneElements: () => readonly unknown[] }>((resolve) => {
    createRoot(root).render(
      React.createElement(excalidraw.Excalidraw, {
        initialData: { elements },
        excalidrawAPI: (handle) => {
          resolve(handle);
        },
      }),
    );
  });
  await document.fonts.ready;
  await new Promise((resolve) => setTimeout(resolve, 1500));
  return {
    loaded_ms: Math.round(loaded),
    ready_ms: Math.round(performance.now() - started),
    elements: api.getSceneElements().length,
    canvases: root.querySelectorAll("canvas").length,
    fonts: [...document.fonts].filter((font) => font.status === "loaded").length,
  };
}

declare global {
  interface Window {
    EXCALIDRAW_ASSET_PATH?: string;
  }
}
