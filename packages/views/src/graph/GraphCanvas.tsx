// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * The graph itself (M8, Task 11, ADR-0034): Sigma over graphology. Nodes start where their IDs put them,
 * so the first frame waits for no layout; ForceAtlas2 then runs in a worker of ours, at a pace that leaves
 * the page most of its time, and can be paused. Whatever this draws is also in the list, which is where
 * someone goes if this machine cannot draw it at all.
 */
import type { GraphData, Id } from "@databaseme/client";
import { Button, IconButton, Maximize, Minus, Plus, useMediaQuery } from "@databaseme/ui";
import forceAtlas2 from "graphology-layout-forceatlas2";
import Graph from "graphology";
import Sigma, { DEFAULT_STYLES } from "sigma";
import { extremityArrow, layerPlain, pathLine, pathLoop } from "sigma/rendering";
import { useEffect, useMemo, useRef, useState, type KeyboardEvent } from "react";
import { useThemeKey } from "../framework/useTheme";
import {
  colourSlot,
  layoutJob,
  passesFor,
  spreadOf,
  startOf,
  titleOf,
  TYPE_COLOURS,
  waitAfter,
} from "./graph";
import type { LayoutReply, LayoutRequest } from "./layout.worker";

/** How much of the graph one press of an arrow key moves, as a share of what is on screen. */
const PAN_STEP = 0.1;
const ZOOM_STEP = 1.5;
const MOVE_MS = 200;
/** How long the worker runs at a time when passes are short, so a small graph settles smoothly. */
const SLICE_MS = 30;
/** Above this, edges and labels stay out of the way of a pan or a zoom. */
const LARGE = 20_000;

/** The defaults the library leaves out of its own `inferSettings` (M8 gate 1c). */
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

export type LayoutState = "none" | "running" | "paused" | "done";

interface GraphLook {
  types: readonly string[];
  noType: string;
  relation: string;
  parent: string;
  ink: string;
  surface: string;
  font: string;
}

function useGraphLook(): GraphLook {
  const theme = useThemeKey();
  return useMemo(() => {
    const style = getComputedStyle(document.documentElement);
    const token = (name: string) => style.getPropertyValue(`--dbme-${name}`).trim();
    return {
      types: Array.from({ length: TYPE_COLOURS }, (_, slot) => token(`chart-${String(slot + 1)}`)),
      noType: token("color-fg-muted"),
      relation: token("graph-relation"),
      parent: token("graph-parent"),
      ink: token("color-fg"),
      surface: token("color-bg"),
      font: getComputedStyle(document.body).fontFamily,
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- the theme is what these all read from
  }, [theme]);
}

/** A node's radius on screen, smaller the more of them share it. */
const nodeSize = (nodes: number) => (nodes <= 200 ? 6 : nodes <= 5000 ? 3 : 1.5);

/** A link's colour: the more there are, the fainter, or a large graph is one solid patch of lines. */
function linkColour(colour: string, nodes: number): string {
  if (nodes <= 200) return colour;
  return `${colour}${nodes <= 5000 ? "66" : "22"}`;
}

interface Position {
  x: number;
  y: number;
}

/** The graph to draw, with nodes it had before kept where they were. */
function buildGraph(data: GraphData, look: GraphLook, before: Graph | null): Graph {
  const graph = new Graph({ multi: true, type: "directed" });
  const spread = spreadOf(data.ids.length);
  const size = nodeSize(data.ids.length);
  const colours = data.node_types.map((type) => {
    const slot = colourSlot(type);
    return slot < 0 ? look.noType : (look.types[slot] ?? look.noType);
  });
  data.ids.forEach((id, node) => {
    let place: Position;
    if (before?.hasNode(id)) {
      place = {
        x: before.getNodeAttribute(id, "x") as number,
        y: before.getNodeAttribute(id, "y") as number,
      };
    } else {
      const start = startOf(id);
      place = { x: start.x * spread, y: start.y * spread };
    }
    graph.addNode(id, {
      ...place,
      size,
      color: colours[node] ?? look.noType,
      label: titleOf(data.titles, node),
    });
  });
  const join = (pairs: readonly number[], attributes: Record<string, unknown>) => {
    for (let at = 0; at + 1 < pairs.length; at += 2) {
      const source = data.ids[pairs[at] ?? -1];
      const target = data.ids[pairs[at + 1] ?? -1];
      if (source !== undefined && target !== undefined) {
        graph.addDirectedEdge(source, target, attributes);
      }
    }
  };
  const thin = data.ids.length > 5000 ? 0.5 : 1;
  const nodes = data.ids.length;
  join(data.edges, { size: thin, color: linkColour(look.relation, nodes), head: "arrow" });
  join(data.parent_edges, { size: thin, color: linkColour(look.parent, nodes), head: "none" });
  return graph;
}

function sigmaOptions(look: GraphLook, large: boolean) {
  const hot = ["isHovered", "isHighlighted"] as ("isHovered" | "isHighlighted")[];
  return {
    primitives: {
      edges: {
        paths: [pathLine(), pathLoop()],
        extremities: [extremityArrow()],
        layers: [layerPlain()],
      },
    },
    styles: {
      nodes: {
        ...DEFAULT_STYLES.nodes,
        labelColor: look.ink,
        labelFont: look.font,
        labelVisibility: { whenState: hot, then: "visible", else: "auto" },
        backdropVisibility: { whenState: hot, then: "visible", else: "hidden" },
        backdropColor: look.surface,
        backdropBorderColor: look.ink,
        backdropBorderWidth: 1,
        depth: { whenState: hot, then: "topNodes", else: "nodes" },
      },
      edges: { ...DEFAULT_STYLES.edges, head: { attribute: "head" } },
      stage: { whenState: "hasHovered", then: { cursor: "pointer" }, else: { cursor: "grab" } },
    },
    settings: {
      // A pane may have no size for a moment, which is not worth a throw.
      allowInvalidContainer: true,
      // Sizes in pixels: left to the positions, a node grows and shrinks as the layout spreads.
      itemSizesReference: "screen",
      hideEdgesOnMove: large,
      hideLabelsOnMove: large,
      labelDensity: 0.5,
    },
  };
}

export interface GraphCanvasProps {
  data: GraphData;
  selected: Id | null;
  /** The graph in words, for a screen reader. */
  summary: string;
  onSelect: (id: Id | null) => void;
  /** This machine cannot draw a graph. */
  onFailed: () => void;
}

export function GraphCanvas({ data, selected, summary, onSelect, onFailed }: GraphCanvasProps) {
  const host = useRef<HTMLDivElement>(null);
  const sigmaRef = useRef<Sigma | null>(null);
  const graphRef = useRef<Graph | null>(null);
  const workerRef = useRef<Worker | null>(null);
  const shown = useRef<GraphData | null>(null);
  const highlighted = useRef<Id | null>(null);
  const look = useGraphLook();
  const large = data.ids.length > LARGE;
  const reduced = useMediaQuery("(prefers-reduced-motion: reduce)");
  const [layout, setLayout] = useState<LayoutState>("none");
  // What the layout's callbacks see, so that a pause reaches a reply already on its way.
  const layoutRef = useRef<LayoutState>("none");
  const passes = useRef(0);
  const waiting = useRef<ReturnType<typeof setTimeout> | null>(null);
  const callbacks = useRef({ onSelect, onFailed });
  useEffect(() => {
    callbacks.current = { onSelect, onFailed };
  });

  const setLayoutState = (state: LayoutState) => {
    layoutRef.current = state;
    setLayout(state);
    if (host.current) host.current.dataset.layout = state;
  };

  const askForNext = () => {
    workerRef.current?.postMessage({ kind: "next", budgetMs: SLICE_MS } satisfies LayoutRequest);
  };

  const stopWaiting = () => {
    if (waiting.current !== null) clearTimeout(waiting.current);
    waiting.current = null;
  };

  /** Puts a reply's positions on the graph, then waits long enough to leave the page its time. */
  const apply = ({ positions, passes: done }: LayoutReply) => {
    const graph = graphRef.current;
    const sigma = sigmaRef.current;
    if (!graph || !sigma || layoutRef.current !== "running") return;
    if (positions.length !== graph.order * 2) return;
    let at = 0;
    const began = performance.now();
    graph.updateEachNodeAttributes(
      (_node, attributes) => {
        attributes.x = positions[at] ?? 0;
        attributes.y = positions[at + 1] ?? 0;
        at += 2;
        return attributes;
      },
      { attributes: ["x", "y"] },
    );
    sigma.once("afterRender", () => {
      if (host.current) host.current.dataset.passes = String(done);
      if (layoutRef.current !== "running") return;
      if (done >= passes.current) {
        setLayoutState("done");
        return;
      }
      waiting.current = setTimeout(askForNext, waitAfter(performance.now() - began));
    });
  };

  /** Starts a run of the layout from where the nodes are now. */
  const runLayout = () => {
    const graph = graphRef.current;
    const current = shown.current;
    if (!graph || !current || graph.size === 0) return;
    stopWaiting();
    if (!workerRef.current) {
      const worker = new Worker(new URL("./layout.worker.ts", import.meta.url), { type: "module" });
      worker.onmessage = (event: MessageEvent<LayoutReply>) => {
        apply(event.data);
      };
      worker.onerror = (event) => {
        // A layout that stops leaves the graph where it is, which is still a graph.
        console.warn(event.message);
        setLayoutState("paused");
      };
      workerRef.current = worker;
    }
    const job = layoutJob(current, (id) => ({
      x: graph.getNodeAttribute(id, "x") as number,
      y: graph.getNodeAttribute(id, "y") as number,
    }));
    passes.current = passesFor(graph.order);
    const start: LayoutRequest = {
      kind: "start",
      positions: job.positions,
      edges: job.edges,
      settings: { ...FA2_DEFAULTS, ...forceAtlas2.inferSettings(graph.order) },
    };
    workerRef.current.postMessage(start, [job.positions.buffer, job.edges.buffer]);
    if (host.current) host.current.dataset.passes = "0";
    setLayoutState("running");
    askForNext();
  };

  const pauseLayout = () => {
    stopWaiting();
    setLayoutState("paused");
  };

  // Sigma, once per look: its styles are fixed when it is made, and a theme changes rarely.
  useEffect(() => {
    const container = host.current;
    if (!container) return;
    let sigma: Sigma;
    try {
      sigma = new Sigma(
        new Graph({ multi: true, type: "directed" }),
        container,
        // The library types these against literal tuples, and they are built from values here.
        sigmaOptions(look, large) as unknown as ConstructorParameters<typeof Sigma>[2],
      );
    } catch (error) {
      // Not a fault: the view says so and shows the list.
      console.warn(error);
      callbacks.current.onFailed();
      return;
    }
    sigmaRef.current = sigma;
    shown.current = null;
    if (import.meta.env.MODE === "e2e") {
      // A canvas shows the tests nothing else, and only the e2e build has this.
      (window as unknown as { __dbmeSigma?: Sigma }).__dbmeSigma = sigma;
    }
    sigma.on("clickNode", ({ node }) => {
      callbacks.current.onSelect(node);
    });
    sigma.on("clickStage", () => {
      callbacks.current.onSelect(null);
    });
    // Sigma follows the window, and a pane changes size without it.
    const observer = new ResizeObserver(() => {
      sigma.resize();
      sigma.scheduleRender();
    });
    observer.observe(container);
    return () => {
      observer.disconnect();
      sigmaRef.current = null;
      sigma.kill();
    };
  }, [look, large]);

  // What it draws follows the data; only a change in who is joined to whom builds it again.
  useEffect(() => {
    const sigma = sigmaRef.current;
    if (!sigma) return;
    const before = shown.current;
    const sameShape =
      before?.ids === data.ids &&
      before.edges === data.edges &&
      before.parent_edges === data.parent_edges &&
      before.node_types === data.node_types;
    const graph = graphRef.current;
    if (sameShape && graph) {
      if (before.titles !== data.titles) {
        data.titles.forEach((title, node) => {
          const id = data.ids[node];
          if (id !== undefined && before.titles[node] !== title) {
            graph.setNodeAttribute(id, "label", title || "Untitled");
          }
        });
      }
      shown.current = data;
      return;
    }
    const wasRunning = layoutRef.current === "running";
    const first = before === null;
    stopWaiting();
    const next = buildGraph(data, look, graph);
    graphRef.current = next;
    shown.current = data;
    highlighted.current = null;
    // This also draws it, and a second refresh would pay for all of that again.
    sigma.setGraph(next);
    const container = host.current;
    if (container) {
      container.dataset.nodes = String(next.order);
      container.dataset.edges = String(next.size);
    }
    // Painted, for whoever is timing it: the browser has shown a frame once two have begun.
    requestAnimationFrame(() => {
      requestAnimationFrame(() => {
        if (container) container.dataset.drawn = "true";
      });
    });
    if (next.size === 0) setLayoutState("none");
    else if (wasRunning || (first && !reduced)) runLayout();
    else setLayoutState("paused");
    // eslint-disable-next-line react-hooks/exhaustive-deps -- the layout's functions read only refs
  }, [data, look, reduced]);

  useEffect(() => {
    const sigma = sigmaRef.current;
    const graph = graphRef.current;
    if (!sigma || !graph) return;
    const before = highlighted.current;
    if (before !== null && before !== selected && graph.hasNode(before)) {
      sigma.setNodeState(before, { isHighlighted: false });
    }
    if (selected !== null && graph.hasNode(selected)) {
      sigma.setNodeState(selected, { isHighlighted: true });
    }
    highlighted.current = selected;
  }, [selected, data, look]);

  // The worker lasts as long as the view.
  useEffect(
    () => () => {
      stopWaiting();
      layoutRef.current = "none";
      workerRef.current?.terminate();
      workerRef.current = null;
    },
    [],
  );

  const camera = () => sigmaRef.current?.getCamera();
  const zoom = (by: number) => {
    const view = camera();
    if (view)
      void view.animate({ ratio: view.getState().ratio * by }, { duration: moveMs(reduced) });
  };
  const showAll = () => {
    void camera()?.animate({ x: 0.5, y: 0.5, ratio: 1, angle: 0 }, { duration: moveMs(reduced) });
  };
  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const view = camera();
    if (!view) return;
    const { x, y, ratio } = view.getState();
    const step = PAN_STEP * ratio;
    const moves: Record<string, () => void> = {
      ArrowLeft: () => view.setState({ x: x - step }),
      ArrowRight: () => view.setState({ x: x + step }),
      ArrowUp: () => view.setState({ y: y + step }),
      ArrowDown: () => view.setState({ y: y - step }),
      "+": () => {
        zoom(1 / ZOOM_STEP);
      },
      "=": () => {
        zoom(1 / ZOOM_STEP);
      },
      "-": () => {
        zoom(ZOOM_STEP);
      },
      "0": showAll,
    };
    const move = moves[event.key];
    if (move) {
      event.preventDefault();
      move();
    }
  };

  return (
    <div className="dbme-graph__stage">
      <div
        ref={host}
        className="dbme-graph__host"
        role="img"
        aria-label={`Graph. ${summary}`}
        tabIndex={0}
        onKeyDown={onKeyDown}
      />
      <div className="dbme-graph__tools" role="group" aria-label="Graph controls">
        <IconButton
          label="Zoom in"
          icon={Plus}
          onClick={() => {
            zoom(1 / ZOOM_STEP);
          }}
        />
        <IconButton
          label="Zoom out"
          icon={Minus}
          onClick={() => {
            zoom(ZOOM_STEP);
          }}
        />
        <IconButton label="Show all" icon={Maximize} onClick={showAll} />
        {layout === "none" ? null : layout === "running" ? (
          <Button onClick={pauseLayout}>Pause layout</Button>
        ) : (
          <Button onClick={runLayout}>Run layout</Button>
        )}
      </div>
    </div>
  );
}

function moveMs(reduced: boolean): number {
  return reduced ? 0 : MOVE_MS;
}
