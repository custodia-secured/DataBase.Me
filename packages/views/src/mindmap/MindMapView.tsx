// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * The Mind map view (ADR-0019): the objects below one object as a tree on a canvas, with the
 * relations among them as dashed edges. Positions are computed, never stored: a drag drops a node
 * onto another (reparent) or between siblings (reorder). Every edit is a command, and the map
 * redraws from `vault-changed`, so the other views follow at once and the other way round.
 *
 * The map is an ARIA tree, like the sidebar, and the Outline stays its text alternative. Unchanged
 * nodes keep their objects from one read to the next, so React Flow redraws only what moved.
 */
import { useTree, type Id, type Position as Place } from "@databaseme/client";
import {
  Button,
  ChevronDown,
  ChevronRight,
  DocumentIcon,
  FolderInput,
  IconButton,
  Link2,
  Maximize,
  Plus,
  StatusMessage,
  Trash2,
  useMediaQuery,
  X,
  ZoomIn,
  ZoomOut,
} from "@databaseme/ui";
import {
  Handle,
  MarkerType,
  Position,
  ReactFlow,
  ReactFlowProvider,
  useReactFlow,
  useStore,
  type Connection,
  type Viewport,
  type Edge,
  type Node,
  type NodeChange,
  type NodeProps,
} from "@xyflow/react";
import {
  createContext,
  memo,
  useCallback,
  useContext,
  useEffect,
  useId,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent,
  type MouseEvent as ReactMouseEvent,
} from "react";
import { actions } from "../actions";
import type { ViewProps } from "../registry";
import { MoveDialog } from "../sidebar/MoveDialog";
import { mapCommand, type MapCommand } from "./keys";
import {
  dropAt,
  fitViewport,
  isWithin,
  layoutMap,
  NODE_HEIGHT,
  NODE_WIDTH,
  nodeAt,
  sibling,
  type Drop,
  type MapNode,
} from "./layout";
import { OverviewCanvas } from "./Overview";
import type { OverviewScene } from "./paint";
import { RelateDialog } from "./RelateDialog";

/** Below this zoom a node's text can't be read, so the canvas overview draws the map (ADR-0019). */
const FAR_ZOOM = 0.35;
/** The keyboard works at a zoom where the node it reaches can be read. */
const READING_ZOOM = 0.8;
/** Low enough that "fit" fits: 1,900 leaves stand about 114,000 px tall. */
const MIN_ZOOM = 0.002;
/** How far the root stands from the left edge when a big map opens. */
const OPEN_MARGIN = 24;
const LONG_PRESS_MS = 450;
const LONG_PRESS_SLOP = 10;

interface TopicData extends Record<string, unknown> {
  node: MapNode;
  renaming: boolean;
  drop: Drop["where"] | null;
}

type TopicNode = Node<TopicData, "topic">;

interface MapApi {
  base: string;
  toggle: (id: Id) => void;
  rename: (id: Id, title: string | null) => void;
}

const MapContext = createContext<MapApi | null>(null);

const nodeDomId = (base: string, id: Id) => `${base}-node-${id}`;
const titleOf = (title: string) => (title === "" ? "Untitled" : title);

const TopicNodeView = memo(function TopicNodeView({ id, data, selected }: NodeProps<TopicNode>) {
  const map = useContext(MapContext);
  const { node, renaming, drop } = data;
  const hasChildren = node.children.length > 0;
  const unread = node.childCount - node.children.length;
  const classes = ["dbme-map__node"];
  if (selected) classes.push("dbme-map__node--active");
  if (drop) classes.push(`dbme-map__node--drop-${drop}`);
  return (
    <>
      <Handle type="target" position={Position.Left} className="dbme-map__target" />
      <div
        id={map ? nodeDomId(map.base, id) : undefined}
        role="treeitem"
        aria-label={titleOf(node.title)}
        aria-level={node.level}
        aria-posinset={node.index + 1}
        aria-setsize={node.siblings}
        aria-selected={selected}
        aria-expanded={hasChildren ? !node.collapsed : undefined}
        className={classes.join(" ")}
      >
        {hasChildren ? (
          <button
            type="button"
            tabIndex={-1}
            className="dbme-map__toggle nodrag nopan"
            aria-label={`${node.collapsed ? "Expand" : "Collapse"} ${titleOf(node.title)}`}
            onClick={(event) => {
              event.stopPropagation();
              map?.toggle(id);
            }}
          >
            {node.collapsed ? (
              <ChevronRight aria-hidden="true" size={14} />
            ) : (
              <ChevronDown aria-hidden="true" size={14} />
            )}
          </button>
        ) : null}
        {renaming ? (
          <input
            // The new node takes the keyboard from the map, and gives it back when done.
            autoFocus
            className="dbme-map__rename nodrag nopan nowheel"
            aria-label={`Rename ${titleOf(node.title)}`}
            defaultValue={node.title}
            onFocus={(event) => {
              event.currentTarget.select();
            }}
            onKeyDown={(event) => {
              event.stopPropagation();
              if (event.key === "Enter") map?.rename(id, event.currentTarget.value);
              else if (event.key === "Escape") map?.rename(id, null);
            }}
            onBlur={(event) => {
              map?.rename(id, event.currentTarget.value);
            }}
          />
        ) : (
          <span className="dbme-map__title">{titleOf(node.title)}</span>
        )}
        {unread > 0 ? (
          <span className="dbme-map__count" title={`${String(unread)} more below, not shown`}>
            +{unread}
          </span>
        ) : null}
        {node.outsideRelations > 0 ? (
          <span
            className="dbme-map__count"
            title={`${String(node.outsideRelations)} relations to pages not on this map`}
          >
            <Link2 aria-hidden="true" size={12} />
            {node.outsideRelations}
          </span>
        ) : null}
      </div>
      <Handle type="source" position={Position.Right} className="dbme-map__source" />
    </>
  );
});

/**
 * Keeps the items that didn't change identical from one list to the next, so that React Flow
 * redraws only what moved. The list is adjusted during render, as React allows for state that
 * follows a prop, so no commit ever shows the fresh objects.
 */
function useStable<T extends { id: string }>(
  fresh: T[],
  same: (before: T, now: T) => boolean,
): T[] {
  const [kept, setKept] = useState<{ from: T[]; items: T[] }>({ from: [], items: [] });
  if (kept.from === fresh) return kept.items;
  const before = new Map(kept.items.map((item) => [item.id, item]));
  const items = fresh.map((item) => {
    const old = before.get(item.id);
    return old && same(old, item) ? old : item;
  });
  setKept({ from: fresh, items });
  return items;
}

function sameTopic(before: TopicNode, now: TopicNode): boolean {
  const a = before.data;
  const b = now.data;
  return (
    before.position.x === now.position.x &&
    before.position.y === now.position.y &&
    before.selected === now.selected &&
    before.draggable === now.draggable &&
    a.renaming === b.renaming &&
    a.drop === b.drop &&
    a.node.title === b.node.title &&
    a.node.childCount === b.node.childCount &&
    a.node.outsideRelations === b.node.outsideRelations &&
    a.node.collapsed === b.node.collapsed &&
    a.node.level === b.node.level &&
    a.node.index === b.node.index &&
    a.node.siblings === b.node.siblings &&
    a.node.parent === b.node.parent &&
    a.node.children.length === b.node.children.length
  );
}

function sameEdge(before: Edge, now: Edge): boolean {
  return (
    before.source === now.source &&
    before.target === now.target &&
    before.selected === now.selected &&
    before.label === now.label
  );
}

const NODE_TYPES = { topic: TopicNodeView };
const PRO_OPTIONS = { hideAttribution: true };
/** Nodes have one size. Without this, React Flow measures a node again each time its object changes. */
const NODE_SIZE = { width: NODE_WIDTH, height: NODE_HEIGHT };
/** A node's wrapper is only a wrapper: a role description would make it an item of the tree. */
const PLAIN_WRAPPER = { "aria-roledescription": undefined };

/**
 * Leaves only the nodes for assistive technology, so that the tree owns its items. React Flow names
 * its root an application, and its edges are drawings that the levels and the relations list say in
 * words.
 */
function fitIntoTree(element: HTMLDivElement | null): void {
  element?.setAttribute("role", "none");
  element?.querySelector(".react-flow__edges")?.setAttribute("aria-hidden", "true");
}

export function MindMapView({ objectId, onOpen }: ViewProps) {
  return (
    <ReactFlowProvider key={objectId}>
      <MindMap objectId={objectId} onOpen={onOpen} />
    </ReactFlowProvider>
  );
}

function MindMap({ objectId, onOpen }: ViewProps) {
  const base = useId();
  const helpId = useId();
  const tree = useTree(objectId);
  const flow = useReactFlow<TopicNode>();
  const coarse = useMediaQuery("(pointer: coarse)");
  const far = useStore((state) => state.transform[2] < FAR_ZOOM);
  // A jump to far away shrinks what React Flow holds first, or it would mount every node on the way.
  const [goingFar, setGoingFar] = useState(false);
  if (far && goingFar) setGoingFar(false);
  const overview = far || goingFar;
  const [initial, setInitial] = useState<Viewport | null>(null);

  const [collapsed, setCollapsed] = useState<ReadonlySet<Id>>(new Set());
  const [active, setActive] = useState<Id>(objectId);
  const [renaming, setRenaming] = useState<Id | null>(null);
  const [chosenRelation, setChosenRelation] = useState<Id | null>(null);
  const [dragged, setDragged] = useState<{ id: Id; x: number; y: number } | null>(null);
  const [drop, setDrop] = useState<Drop | null>(null);
  const [moving, setMoving] = useState<MapNode | null>(null);
  const [relating, setRelating] = useState<MapNode | null>(null);
  const treeElement = useRef<HTMLDivElement>(null);
  /** A node just made, which the next read will bring. */
  const pending = useRef<Id | null>(null);

  const nodes = tree.data?.nodes;
  const relations = tree.data?.relations;
  const layout = useMemo(() => layoutMap(nodes ?? [], collapsed), [nodes, collapsed]);
  const current = layout.byId.get(active) ?? null;
  const rootId = layout.nodes[0]?.id ?? null;

  // The first view fits the map, worked out here so that it starts in the right mode.
  useLayoutEffect(() => {
    const box = treeElement.current?.getBoundingClientRect();
    if (initial || !box || layout.nodes.length === 0) return;
    const fitted = fitViewport(layout, box.width, box.height, MIN_ZOOM, 1);
    const root = layout.nodes[0];
    // All of a map that fits at a zoom it can be read at; otherwise its root, ready to explore.
    setInitial(
      fitted.zoom >= FAR_ZOOM || !root
        ? fitted
        : {
            x: OPEN_MARGIN - root.x * READING_ZOOM,
            y: box.height / 2 - (root.y + NODE_HEIGHT / 2) * READING_ZOOM,
            zoom: READING_ZOOM,
          },
    );
  }, [initial, layout]);

  const fit = () => {
    const box = treeElement.current?.getBoundingClientRect();
    if (!box) return;
    const fitted = fitViewport(layout, box.width, box.height, MIN_ZOOM, 1);
    if (fitted.zoom < FAR_ZOOM) setGoingFar(true);
    requestAnimationFrame(() => {
      void flow.setViewport(fitted);
    });
  };

  // The node with the keyboard went away, by Undo or from another view: fall back to the root.
  useEffect(() => {
    if (nodes === undefined || rootId === null) return;
    if (layout.byId.has(active)) {
      if (pending.current === active) pending.current = null;
    } else if (pending.current !== active) {
      setActive(rootId);
    }
  }, [nodes, layout, active, rootId]);

  // The focused node stays where it is on screen when the layout changes around it.
  const anchor = useRef<{ id: Id; x: number; y: number } | null>(null);
  useLayoutEffect(() => {
    if (!current) return;
    const last = anchor.current;
    anchor.current = { id: current.id, x: current.x, y: current.y };
    if (last?.id !== current.id) return;
    const dx = current.x - last.x;
    const dy = current.y - last.y;
    if (dx === 0 && dy === 0) return;
    const viewport = flow.getViewport();
    void flow.setViewport({
      x: viewport.x - dx * viewport.zoom,
      y: viewport.y - dy * viewport.zoom,
      zoom: viewport.zoom,
    });
  }, [current, flow]);

  const focusMap = useCallback(() => {
    treeElement.current?.focus({ preventScroll: true });
  }, []);

  /** Brings a node into view at a zoom it can be read at, if the keyboard just reached it. */
  const reveal = useCallback(
    (id: Id) => {
      const node = layout.byId.get(id);
      const box = treeElement.current?.getBoundingClientRect();
      if (!node || !box) return;
      const { x, y, zoom } = flow.getViewport();
      const left = node.x * zoom + x;
      const top = node.y * zoom + y;
      const inView =
        left >= 0 &&
        top >= 0 &&
        left + NODE_WIDTH * zoom <= box.width &&
        top + NODE_HEIGHT * zoom <= box.height;
      if (inView && zoom >= FAR_ZOOM) return;
      void flow.setCenter(node.x + NODE_WIDTH / 2, node.y + NODE_HEIGHT / 2, {
        zoom: Math.max(zoom, READING_ZOOM),
      });
    },
    [flow, layout],
  );

  // A node being named has to be seen: a new one can land far down its family, off the screen and
  // so out of the page, and F2 can come from the overview, where a node is too small to read.
  useLayoutEffect(() => {
    if (renaming !== null) reveal(renaming);
  }, [renaming, reveal]);

  const setExpanded = useCallback((id: Id, open: boolean) => {
    setCollapsed((before) => {
      if (before.has(id) !== open) return before;
      const next = new Set(before);
      if (open) next.delete(id);
      else next.add(id);
      return next;
    });
  }, []);

  const add = async (parent: Id, position: Place) => {
    const id = await actions.create({ title: "", parent_id: parent, position });
    if (id === undefined) return;
    pending.current = id;
    setExpanded(parent, true);
    setActive(id);
    setRenaming(id);
  };

  const trash = async (node: MapNode) => {
    const next = sibling(layout, node, 1) ?? sibling(layout, node, -1) ?? node.parent;
    if (next !== null) {
      setActive(next);
      reveal(next);
    }
    await actions.trash(node.id);
    focusMap();
  };

  const run = (command: MapCommand) => {
    if (!current) return;
    switch (command.kind) {
      case "focus":
        setActive(command.id);
        setChosenRelation(null);
        reveal(command.id);
        break;
      case "expand":
        setExpanded(command.id, true);
        break;
      case "toggle":
        setExpanded(current.id, current.collapsed);
        break;
      case "add-child":
        void add(current.id, "last");
        break;
      case "add-sibling":
        if (current.parent !== null) void add(current.parent, { after: current.id });
        break;
      case "rename":
        setRenaming(current.id);
        break;
      case "reorder":
        if (current.parent !== null)
          void actions.move(current.id, current.parent, command.position);
        break;
      case "trash":
        void trash(current);
        break;
      case "open":
        onOpen(current.id);
        break;
      case "stay":
        break;
    }
  };

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.target instanceof HTMLInputElement) return;
    if (chosenRelation !== null && (event.key === "Delete" || event.key === "Backspace")) {
      event.preventDefault();
      void actions.unrelate(chosenRelation);
      setChosenRelation(null);
      return;
    }
    if (!current) return;
    const command = mapCommand(event, current, layout);
    if (!command) return;
    event.preventDefault();
    run(command);
  };

  const finishDrag = useCallback(
    (id: Id, target: Drop | null) => {
      setDragged(null);
      setDrop(null);
      if (target) {
        setExpanded(target.parent, true);
        void actions.move(id, target.parent, target.position);
      }
    },
    [setExpanded],
  );

  // A long press, then a drag, moves a node on a touch screen, so a pan that starts on a node
  // doesn't move it by accident. Until then a finger on a node pans like one on the background.
  const layoutRef = useRef(layout);
  const overviewRef = useRef(overview);
  useLayoutEffect(() => {
    layoutRef.current = layout;
    overviewRef.current = overview;
  }, [layout, overview]);
  useEffect(() => {
    const element = treeElement.current;
    if (!coarse || !element) return;
    let timer: ReturnType<typeof setTimeout> | null = null;
    let start: { x: number; y: number } | null = null;
    let armed: Id | null = null;
    let target: Drop | null = null;
    const clear = () => {
      if (timer !== null) clearTimeout(timer);
      timer = null;
    };
    const onStart = (event: TouchEvent) => {
      clear();
      const touch = event.touches[0];
      if (event.touches.length !== 1 || !touch || !(event.target instanceof Element)) return;
      const point = flow.screenToFlowPosition({ x: touch.clientX, y: touch.clientY });
      const id =
        event.target.closest(".react-flow__node")?.getAttribute("data-id") ??
        (overviewRef.current ? nodeAt(layoutRef.current, point.x, point.y)?.id : null);
      if (!id || layoutRef.current.byId.get(id)?.parent == null) return;
      start = { x: touch.clientX, y: touch.clientY };
      timer = setTimeout(() => {
        armed = id;
        setActive(id);
      }, LONG_PRESS_MS);
    };
    const onMove = (event: TouchEvent) => {
      const touch = event.touches[0];
      if (!touch) return;
      if (armed === null) {
        const moved = start && Math.hypot(touch.clientX - start.x, touch.clientY - start.y);
        if (moved && moved > LONG_PRESS_SLOP) clear();
        return;
      }
      // The map's own pan must not see this touch.
      event.stopPropagation();
      event.preventDefault();
      const point = flow.screenToFlowPosition({ x: touch.clientX, y: touch.clientY });
      target = dropAt(layoutRef.current, armed, point.x, point.y);
      setDragged({
        id: armed,
        x: point.x - NODE_WIDTH / 2,
        y: point.y - NODE_HEIGHT / 2,
      });
      setDrop(target);
    };
    const onEnd = () => {
      clear();
      if (armed !== null) finishDrag(armed, target);
      armed = null;
      target = null;
      start = null;
    };
    element.addEventListener("touchstart", onStart, { capture: true, passive: true });
    element.addEventListener("touchmove", onMove, { capture: true, passive: false });
    element.addEventListener("touchend", onEnd, { capture: true, passive: true });
    element.addEventListener("touchcancel", onEnd, { capture: true, passive: true });
    return () => {
      clear();
      element.removeEventListener("touchstart", onStart, { capture: true });
      element.removeEventListener("touchmove", onMove, { capture: true });
      element.removeEventListener("touchend", onEnd, { capture: true });
      element.removeEventListener("touchcancel", onEnd, { capture: true });
    };
  }, [coarse, flow, finishDrag]);

  const api = useMemo<MapApi>(
    () => ({
      base,
      toggle: (id) => {
        const node = layoutRef.current.byId.get(id);
        if (!node) return;
        if (!node.collapsed) {
          // The keyboard can't stay on a node that is being folded away.
          setActive((at) => (isWithin(layoutRef.current, at, id) ? id : at));
        }
        setExpanded(id, node.collapsed);
      },
      rename: (id, title) => {
        setRenaming(null);
        const node = layoutRef.current.byId.get(id);
        if (title !== null && node && title !== node.title) void actions.rename(id, title);
        focusMap();
      },
    }),
    [base, focusMap, setExpanded],
  );

  const freshNodes = useMemo(
    () =>
      (overview ? layout.nodes.filter((node) => node.id === active) : layout.nodes).map(
        (node): TopicNode => {
          const isDragged = dragged?.id === node.id;
          const isRenaming = renaming === node.id;
          return {
            id: node.id,
            type: "topic",
            position: { x: isDragged ? dragged.x : node.x, y: isDragged ? dragged.y : node.y },
            width: NODE_WIDTH,
            height: NODE_HEIGHT,
            measured: NODE_SIZE,
            selected: node.id === active,
            draggable: !coarse && node.parent !== null && !isRenaming,
            ariaRole: "presentation",
            domAttributes: PLAIN_WRAPPER,
            data: {
              node,
              renaming: isRenaming,
              drop: drop?.target === node.id ? drop.where : null,
            },
          };
        },
      ),
    [layout, overview, active, dragged, drop, renaming, coarse],
  );
  const flowNodes = useStable(freshNodes, sameTopic);

  const freshEdges = useMemo(() => {
    const edges: Edge[] = [];
    if (overview) return edges;
    for (const node of layout.nodes) {
      if (node.parent === null) continue;
      edges.push({
        id: `branch-${node.id}`,
        source: node.parent,
        target: node.id,
        selectable: false,
        focusable: false,
        className: "dbme-map__branch",
      });
    }
    for (const relation of relations ?? []) {
      if (!layout.byId.has(relation.src_id) || !layout.byId.has(relation.dst_id)) continue;
      edges.push({
        id: relation.id,
        source: relation.src_id,
        target: relation.dst_id,
        label: relation.rel_type,
        selected: relation.id === chosenRelation,
        selectable: true,
        focusable: false,
        markerEnd: { type: MarkerType.ArrowClosed },
        className: "dbme-map__relation",
      });
    }
    return edges;
  }, [layout, overview, relations, chosenRelation]);
  const flowEdges = useStable(freshEdges, sameEdge);

  const onNodesChange = useCallback((changes: NodeChange<TopicNode>[]) => {
    for (const change of changes) {
      if (change.type === "position" && change.dragging && change.position) {
        setDragged({ id: change.id, x: change.position.x, y: change.position.y });
      }
    }
  }, []);

  const scene = useMemo<OverviewScene>(
    () => ({ layout, relations: relations ?? [], active, dragged, drop }),
    [layout, relations, active, dragged, drop],
  );

  /** A press on a box on the canvas drags it; a press on the background pans, as usual. */
  const onOverviewPress = (event: ReactMouseEvent<HTMLDivElement>) => {
    if (!overview || coarse || event.button !== 0) return;
    if (!(event.target instanceof Element) || event.target.closest(".react-flow__node")) return;
    const start = flow.screenToFlowPosition({ x: event.clientX, y: event.clientY });
    const node = nodeAt(layout, start.x, start.y);
    if (!node) return;
    event.stopPropagation();
    event.preventDefault();
    setActive(node.id);
    focusMap();
    if (node.parent === null) return;
    const id = node.id;
    let target: Drop | null = null;
    const onMove = (move: MouseEvent) => {
      const point = flow.screenToFlowPosition({ x: move.clientX, y: move.clientY });
      target = dropAt(layout, id, point.x, point.y);
      setDragged({ id, x: point.x - NODE_WIDTH / 2, y: point.y - NODE_HEIGHT / 2 });
      setDrop(target);
    };
    const onUp = () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
      finishDrag(id, target);
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
  };

  const relationsHere = useMemo(
    () => (relations ?? []).filter((r) => r.src_id === active || r.dst_id === active),
    [relations, active],
  );

  if (tree.isError) return <StatusMessage tone="error">This map could not be read.</StatusMessage>;
  if (!tree.data) return <StatusMessage>Loading the map…</StatusMessage>;
  if (layout.nodes.length === 0)
    return <StatusMessage>There is nothing to show here.</StatusMessage>;

  return (
    <MapContext.Provider value={api}>
      <section className="dbme-map" aria-label="Mind map view">
        <p id={helpId} className="dbme-visually-hidden">
          Arrow keys move between pages. Tab adds a child and Enter a sibling. F2 renames. Alt+Up
          and Alt+Down reorder. Delete sends a page to the Trash. Space folds a branch.
          Control+Enter opens a page. Shift+Tab leaves the map. The Outline shows the same pages as
          text.
        </p>
        <div className="dbme-view-bar dbme-map__bar">
          <Button
            disabled={!current}
            onClick={() => {
              if (current) void add(current.id, "last");
            }}
          >
            <Plus aria-hidden="true" size={16} />
            <span className="dbme-map__label">Add child</span>
          </Button>
          <Button
            disabled={current?.parent == null}
            onClick={() => {
              setMoving(current);
            }}
          >
            <FolderInput aria-hidden="true" size={16} />
            <span className="dbme-map__label">Move to…</span>
          </Button>
          <Button
            disabled={!current}
            onClick={() => {
              setRelating(current);
            }}
          >
            <Link2 aria-hidden="true" size={16} />
            <span className="dbme-map__label">Relate to…</span>
          </Button>
          <Button
            disabled={!current}
            onClick={() => {
              if (current) onOpen(current.id);
            }}
          >
            <DocumentIcon aria-hidden="true" size={16} />
            <span className="dbme-map__label">Open</span>
          </Button>
          <Button
            disabled={current?.parent == null}
            onClick={() => {
              if (current) void trash(current);
            }}
          >
            <Trash2 aria-hidden="true" size={16} />
            <span className="dbme-map__label">Trash</span>
          </Button>
          <span className="dbme-map__zoom">
            <IconButton
              label="Zoom in"
              icon={ZoomIn}
              onClick={() => {
                void flow.zoomIn();
              }}
            />
            <IconButton
              label="Zoom out"
              icon={ZoomOut}
              onClick={() => {
                void flow.zoomOut();
              }}
            />
            <IconButton label="Fit the map" icon={Maximize} onClick={fit} />
          </span>
        </div>
        {tree.data.truncated ? (
          <StatusMessage>
            This map shows part of the tree. Open a branch to see what is below it.
          </StatusMessage>
        ) : null}
        <div className="dbme-map__stage">
          <div
            ref={treeElement}
            className="dbme-map__canvas"
            role="tree"
            aria-label="Mind map"
            aria-describedby={helpId}
            aria-activedescendant={current ? nodeDomId(base, current.id) : undefined}
            tabIndex={0}
            onKeyDown={onKeyDown}
            onMouseDownCapture={onOverviewPress}
            onDoubleClick={(event) => {
              if (!overview) return;
              const point = flow.screenToFlowPosition({ x: event.clientX, y: event.clientY });
              const node = nodeAt(layout, point.x, point.y);
              if (node) {
                setActive(node.id);
                reveal(node.id);
              }
            }}
          >
            {overview ? <OverviewCanvas scene={scene} /> : null}
            {initial ? (
              <ReactFlow<TopicNode>
                ref={fitIntoTree}
                nodes={flowNodes}
                edges={flowEdges}
                nodeTypes={NODE_TYPES}
                onNodesChange={onNodesChange}
                defaultViewport={initial}
                minZoom={MIN_ZOOM}
                maxZoom={1.5}
                proOptions={PRO_OPTIONS}
                onlyRenderVisibleElements
                nodesFocusable={false}
                edgesFocusable={false}
                disableKeyboardA11y
                elevateNodesOnSelect={false}
                elevateEdgesOnSelect={false}
                selectNodesOnDrag={false}
                zoomOnDoubleClick={false}
                nodeDragThreshold={4}
                deleteKeyCode={null}
                selectionKeyCode={null}
                multiSelectionKeyCode={null}
                onNodeClick={(_, node) => {
                  setActive(node.id);
                  setChosenRelation(null);
                  focusMap();
                }}
                onNodeDoubleClick={(_, node) => {
                  setActive(node.id);
                  setRenaming(node.id);
                }}
                onEdgeClick={(_, edge) => {
                  if (edge.selectable) setChosenRelation(edge.id);
                  focusMap();
                }}
                onPaneClick={() => {
                  setChosenRelation(null);
                  focusMap();
                }}
                onNodeDragStart={(_, node) => {
                  setActive(node.id);
                }}
                onNodeDrag={(event, node) => {
                  const { clientX, clientY } =
                    "touches" in event ? (event.touches[0] ?? { clientX: 0, clientY: 0 }) : event;
                  const point = flow.screenToFlowPosition({ x: clientX, y: clientY });
                  setDrop(dropAt(layout, node.id, point.x, point.y));
                }}
                onNodeDragStop={(event, node) => {
                  const { clientX, clientY } =
                    "changedTouches" in event
                      ? (event.changedTouches[0] ?? { clientX: 0, clientY: 0 })
                      : event;
                  const point = flow.screenToFlowPosition({ x: clientX, y: clientY });
                  finishDrag(node.id, dropAt(layout, node.id, point.x, point.y));
                }}
                onConnectEnd={(event, state) => {
                  // Letting go anywhere on a node will do, not only on its small handle.
                  const from = state.fromNode?.id;
                  if (state.isValid || !from) return;
                  const { clientX, clientY } =
                    "changedTouches" in event
                      ? (event.changedTouches[0] ?? { clientX: 0, clientY: 0 })
                      : event;
                  const point = flow.screenToFlowPosition({ x: clientX, y: clientY });
                  const over = nodeAt(layout, point.x, point.y);
                  if (over && over.id !== from) void actions.relate(from, over.id);
                }}
                onConnect={(connection: Connection) => {
                  if (connection.source !== connection.target) {
                    void actions.relate(connection.source, connection.target);
                  }
                }}
              />
            ) : null}
          </div>
          {/* Over the map, not above it: a list that comes and goes must not make the map jump. */}
          {relationsHere.length > 0 ? (
            <ul
              className="dbme-map__relations"
              aria-label={`Relations of ${titleOf(current?.title ?? "")}`}
            >
              {relationsHere.map((relation) => {
                const out = relation.src_id === active;
                const other = layout.byId.get(out ? relation.dst_id : relation.src_id);
                const owned = relation.field_id !== null || relation.src_block_id !== null;
                const name = titleOf(other?.title ?? "");
                return (
                  <li key={relation.id} className="dbme-map__relation-item">
                    <span>
                      {out ? "→" : "←"} {name} ({relation.rel_type})
                    </span>
                    {owned ? null : (
                      <IconButton
                        label={`Remove the relation with ${name}`}
                        icon={X}
                        onClick={() => {
                          void actions.unrelate(relation.id);
                        }}
                      />
                    )}
                  </li>
                );
              })}
            </ul>
          ) : null}
        </div>
        <MoveDialog
          object={moving && { id: moving.id, title: moving.title }}
          onClose={() => {
            setMoving(null);
            focusMap();
          }}
        />
        <RelateDialog
          object={relating && { id: relating.id, title: relating.title }}
          onClose={() => {
            setRelating(null);
            focusMap();
          }}
        />
      </section>
    </MapContext.Provider>
  );
}
