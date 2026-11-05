// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * M8's gates (docs/milestones/M8.md, Task 1): each one loads a library the plan names and makes it
 * do its real work under the app's CSP, so the e2e fixtures' checks (no CSP violation, no error, no
 * request leaving the machine) say whether the library can come in at all. Each returns the figures
 * the gate records.
 */
export type GateName = "graph" | "layout" | "canvas";

type Result = Record<string, unknown>;

export function runGate(name: GateName, root: HTMLElement, params: URLSearchParams) {
  switch (name) {
    case "graph":
      return gateGraph(root, Number(params.get("nodes") ?? 100_000));
    case "layout":
      return gateLayout(Number(params.get("nodes") ?? 100_000));
    case "canvas":
      return gateCanvas(root);
    default:
      return Promise.reject(new Error(`no gate ${String(name)}`));
  }
}

/** The nodes and edges of a test graph: scattered by index, each joined to a few before it. */
async function makeGraph(nodes: number) {
  const { default: Graph } = await import("graphology");
  const graph = new Graph({ multi: true, type: "directed" });
  for (let index = 0; index < nodes; index += 1) {
    const angle = index * 2.399963;
    const radius = Math.sqrt(index);
    graph.addNode(String(index), {
      x: Math.cos(angle) * radius,
      y: Math.sin(angle) * radius,
      size: 2,
      color: "#3b6ea5",
      label: `Node ${String(index)}`,
    });
    if (index > 0) graph.addEdge(String(index), String((index * 7) % index));
    if (index > 2) graph.addEdge(String(index), String(index >> 1));
  }
  return graph;
}

/** Sigma's first frame of a large graph, and whether anything reached the canvas. */
async function gateGraph(root: HTMLElement, nodes: number): Promise<Result> {
  const started = performance.now();
  const { default: Sigma } = await import("sigma");
  const loaded = performance.now() - started;
  const graph = await makeGraph(nodes);
  const built = performance.now() - started;
  const framesTo = new Promise<number>((resolve) => {
    requestAnimationFrame(() => {
      requestAnimationFrame(() => {
        resolve(performance.now() - started);
      });
    });
  });
  const sigma = new Sigma(graph, root);
  sigma.refresh();
  const rendered = performance.now() - started;
  const painted = await framesTo;
  const canvases = [...root.querySelectorAll("canvas")];
  const ink = canvases.some((canvas) => {
    const gl = canvas.getContext("webgl2") ?? canvas.getContext("webgl");
    if (!gl) return false;
    sigma.refresh();
    const pixels = new Uint8Array(4 * 64 * 64);
    const x = Math.floor(gl.drawingBufferWidth / 2) - 32;
    const y = Math.floor(gl.drawingBufferHeight / 2) - 32;
    gl.readPixels(x, y, 64, 64, gl.RGBA, gl.UNSIGNED_BYTE, pixels);
    return pixels.some((value) => value !== 0);
  });
  const result = {
    nodes: sigma.getGraph().order,
    edges: sigma.getGraph().size,
    loaded_ms: Math.round(loaded),
    built_ms: Math.round(built),
    rendered_ms: Math.round(rendered),
    painted_ms: Math.round(painted),
    canvases: canvases.length,
    ink,
  };
  sigma.kill();
  return result;
}

// `inferSettings` returns only the settings it infers; the rest are the library's defaults.
const FA2_DEFAULTS = {
  linLogMode: false,
  outboundAttractionDistribution: false,
  adjustSizes: false,
  edgeWeightInfluence: 1,
  scalingRatio: 1,
  strongGravityMode: false,
  gravity: 1,
  slowDown: 1,
  barnesHutOptimize: false,
  barnesHutTheta: 0.5,
};

/** One ForceAtlas2 pass in a worker of ours: the library's own worker is a `blob:` URL. */
async function gateLayout(nodes: number): Promise<Result> {
  const graph = await makeGraph(nodes);
  const { default: forceAtlas2 } = await import("graphology-layout-forceatlas2");
  const settings = { ...FA2_DEFAULTS, ...forceAtlas2.inferSettings(graph) };
  const data = {
    nodes: graph.mapNodes((_, attributes) => [attributes.x as number, attributes.y as number]),
    edges: graph.mapEdges((_edge, _attributes, source, target) => [Number(source), Number(target)]),
    settings,
  };
  const worker = new Worker(new URL("./m8-layout.worker.ts", import.meta.url), { type: "module" });
  const started = performance.now();
  const reply = await new Promise<{ moved: number }>((resolve, reject) => {
    worker.onmessage = (event: MessageEvent<{ moved: number }>) => {
      resolve(event.data);
    };
    worker.onerror = (event) => {
      reject(new Error(event.message));
    };
    worker.postMessage(data);
  });
  worker.terminate();
  return { nodes, pass_ms: Math.round(performance.now() - started), ...reply };
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
