// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * The hierarchy sidebar: every object in its one home, as an ARIA tree. Children load when a
 * branch opens and rows are virtualised, so a large vault stays fast.
 *
 * Keyboard (tree pattern): ↑ ↓ move, → opens or goes to the first child, ← closes or goes to
 * the parent, Home and End, Enter opens the object, Alt+↑ and Alt+↓ move it among its siblings.
 * The Actions menu offers New page inside, Move to…, and Move to Trash for the focused row.
 * With a pointer or a long press on touch, rows drag: onto the top or bottom edge of a row to
 * go before or after it, onto its middle to go inside it.
 */
import {
  commands,
  keys,
  MANUAL,
  unwrap,
  useTypes,
  type Id,
  type ObjectSummary_Serialize,
} from "@databaseme/client";
import {
  Ellipsis,
  FolderInput,
  IconButton,
  Menu,
  Plus,
  StatusMessage,
  Trash2,
  TypeIcon,
  useMediaQuery,
  ChevronDown,
  ChevronRight,
  ArrowUp,
  ArrowDown,
} from "@databaseme/ui";
import {
  DndContext,
  DragOverlay,
  getClientRect,
  MouseSensor,
  pointerWithin,
  TouchSensor,
  useDraggable,
  useDroppable,
  useSensor,
  useSensors,
  type DragEndEvent,
  type DragMoveEvent,
  type DragStartEvent,
} from "@dnd-kit/core";
import { useQueries } from "@tanstack/react-query";
import { useVirtualizer } from "@tanstack/react-virtual";
import { useEffect, useId, useMemo, useRef, useState, type KeyboardEvent } from "react";
import { actions } from "../actions";
import { MoveDialog } from "./MoveDialog";
import { dropZone, expandedBelow, flattenTree, isWithin, type TreeRow } from "./tree";

const CHILD_LIMIT = 500;

export interface SidebarProps {
  /** The object shown in the main pane. */
  selected: Id | null;
  onSelect: (id: Id) => void;
}

interface Drop {
  id: Id;
  zone: "before" | "inside" | "after";
}

function pointerY(event: Event | null): number {
  if (!event) return 0;
  if ("touches" in event) {
    const touch = (event as TouchEvent).touches[0] ?? (event as TouchEvent).changedTouches[0];
    return touch?.clientY ?? 0;
  }
  return (event as MouseEvent).clientY;
}

export function Sidebar({ selected, onSelect }: SidebarProps) {
  const treeId = useId();
  const [expanded, setExpanded] = useState<ReadonlySet<Id>>(new Set());
  const [active, setActive] = useState<Id | null>(selected);
  const [moving, setMoving] = useState<{ id: Id; title: string } | null>(null);
  const [dragging, setDragging] = useState<TreeRow | null>(null);
  const [drop, setDrop] = useState<Drop | null>(null);
  const startY = useRef(0);
  const scroller = useRef<HTMLDivElement>(null);
  const coarse = useMediaQuery("(pointer: coarse)");
  const types = useTypes();

  const parents = useMemo(() => [null, ...expanded], [expanded]);
  const pages = useQueries({
    queries: parents.map((parent) => {
      const source = { kind: "children" as const, parent };
      return {
        queryKey: keys.page(source, MANUAL, 0, CHILD_LIMIT),
        queryFn: () => unwrap(commands.objectsPage(source, MANUAL, 0, CHILD_LIMIT)),
      };
    }),
  });
  const byParent = new Map<Id | null, ObjectSummary_Serialize[] | undefined>(
    parents.map((parent, i) => [parent, pages[i]?.data]),
  );
  const rows = flattenTree((parent) => byParent.get(parent), expanded);
  const topLevel = pages[0];

  // When another view selects an object, the tree's focus follows it.
  const [followed, setFollowed] = useState(selected);
  if (followed !== selected) {
    setFollowed(selected);
    if (selected !== null) setActive(selected);
  }

  const activeIndex = rows.findIndex((row) => row.id === active);
  const current = rows[activeIndex];

  // eslint-disable-next-line react-hooks/incompatible-library -- TanStack Virtual returns new functions each render; nothing here memoises them
  const virtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => scroller.current,
    estimateSize: () => (coarse ? 44 : 32),
    overscan: 10,
  });
  useEffect(() => {
    virtualizer.measure();
  }, [virtualizer, coarse]);

  const rowId = (id: Id) => `${treeId}-${id}`;
  const go = (index: number) => {
    const row = rows[Math.max(0, Math.min(rows.length - 1, index))];
    if (!row) return;
    setActive(row.id);
    virtualizer.scrollToIndex(rows.indexOf(row), { align: "auto" });
  };
  const expand = (id: Id, open: boolean) => {
    setExpanded((before) => {
      const next = new Set(before);
      if (open) next.add(id);
      else {
        next.delete(id);
        for (const below of expandedBelow(rows, id)) next.delete(below);
      }
      return next;
    });
  };

  const reorder = (row: TreeRow, direction: -1 | 1) => {
    const siblings = byParent.get(row.parent) ?? [];
    const neighbour = siblings[row.position - 1 + direction];
    if (!neighbour) return;
    void actions.move(
      row.id,
      row.parent,
      direction < 0 ? { before: neighbour.object.id } : { after: neighbour.object.id },
    );
  };

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (!current) {
      if (rows.length > 0 && event.key.startsWith("Arrow")) {
        event.preventDefault();
        go(0);
      }
      return;
    }
    let handled = true;
    switch (event.key) {
      case "ArrowDown":
        if (event.altKey) reorder(current, 1);
        else go(activeIndex + 1);
        break;
      case "ArrowUp":
        if (event.altKey) reorder(current, -1);
        else go(activeIndex - 1);
        break;
      case "ArrowRight":
        if (current.hasChildren && !current.expanded) expand(current.id, true);
        else if (current.expanded) go(activeIndex + 1);
        break;
      case "ArrowLeft":
        if (current.expanded) expand(current.id, false);
        else if (current.parent !== null) go(rows.findIndex((row) => row.id === current.parent));
        break;
      case "Home":
        go(0);
        break;
      case "End":
        go(rows.length - 1);
        break;
      case "Enter":
      case " ":
        onSelect(current.id);
        break;
      default:
        handled = false;
    }
    if (handled) event.preventDefault();
  };

  const newPage = async (parent: Id | null) => {
    const id = await actions.create({ title: "", parent_id: parent, position: "last" });
    if (id === undefined) return;
    if (parent !== null) expand(parent, true);
    setActive(id);
    onSelect(id);
  };

  const sensors = useSensors(
    useSensor(MouseSensor, { activationConstraint: { distance: 6 } }),
    useSensor(TouchSensor, { activationConstraint: { delay: 300, tolerance: 8 } }),
  );

  const zoneOf = (event: DragMoveEvent | DragEndEvent): Drop | null => {
    const over = event.over;
    if (!over || over.id === event.active.id) return null;
    const y = startY.current + event.delta.y;
    return { id: String(over.id), zone: dropZone(y, over.rect.top, over.rect.height) };
  };

  const onDragStart = (event: DragStartEvent) => {
    startY.current = pointerY(event.activatorEvent);
    setDragging(rows.find((row) => row.id === event.active.id) ?? null);
  };

  const onDragEnd = (event: DragEndEvent) => {
    const target = zoneOf(event);
    const id = String(event.active.id);
    setDragging(null);
    setDrop(null);
    if (!target || isWithin(rows, target.id, id)) return;
    const over = rows.find((row) => row.id === target.id);
    if (!over) return;
    if (target.zone === "inside") {
      expand(over.id, true);
      void actions.move(id, over.id, "last");
    } else {
      void actions.move(
        id,
        over.parent,
        target.zone === "before" ? { before: over.id } : { after: over.id },
      );
    }
  };

  const typeOf = (row: TreeRow) =>
    types.data?.find((type) => type.id === row.summary.type_ids[0])?.icon ?? null;

  return (
    <nav className="dbme-sidebar" aria-label="Pages">
      <div className="dbme-sidebar__bar">
        <h2 className="dbme-sidebar__heading">Pages</h2>
        <IconButton label="New page" icon={Plus} onClick={() => void newPage(null)} />
        <Menu
          label={current ? `Actions for ${current.summary.object.title || "Untitled"}` : "Actions"}
          icon={Ellipsis}
          items={[
            {
              label: "New page inside",
              icon: Plus,
              disabled: !current,
              onSelect: () => void newPage(current?.id ?? null),
            },
            {
              label: "Move to…",
              icon: FolderInput,
              disabled: !current,
              onSelect: () => {
                if (current) setMoving({ id: current.id, title: current.summary.object.title });
              },
            },
            {
              label: "Move up",
              icon: ArrowUp,
              disabled: !current || current.position === 1,
              onSelect: () => {
                if (current) reorder(current, -1);
              },
            },
            {
              label: "Move down",
              icon: ArrowDown,
              disabled: !current || current.position === current.siblings,
              onSelect: () => {
                if (current) reorder(current, 1);
              },
            },
            {
              label: "Move to Trash",
              icon: Trash2,
              danger: true,
              disabled: !current,
              onSelect: () => {
                if (current) void actions.trash(current.id);
              },
            },
          ]}
        />
      </div>
      {topLevel?.isSuccess && rows.length === 0 ? (
        <StatusMessage>No pages yet. Use New page to make one.</StatusMessage>
      ) : null}
      <DndContext
        sensors={sensors}
        // The row under the pointer is the target, as the drop zone is worked out from the
        // pointer too. Rows are placed with transforms (virtualised), so measure them with the
        // transforms; dnd-kit leaves transforms out by default, which puts every row at the top.
        collisionDetection={pointerWithin}
        measuring={{ droppable: { measure: getClientRect } }}
        onDragStart={onDragStart}
        onDragMove={(event) => {
          setDrop(zoneOf(event));
        }}
        onDragEnd={onDragEnd}
        onDragCancel={() => {
          setDragging(null);
          setDrop(null);
        }}
      >
        <div
          ref={scroller}
          className="dbme-tree"
          role="tree"
          aria-label="Pages"
          tabIndex={0}
          aria-activedescendant={current ? rowId(current.id) : undefined}
          onKeyDown={onKeyDown}
          onFocus={() => {
            if (!current && rows[0]) setActive(rows[0].id);
          }}
        >
          <div
            role="presentation"
            className="dbme-tree__body"
            style={{ height: `${String(virtualizer.getTotalSize())}px` }}
          >
            {virtualizer.getVirtualItems().map((item) => {
              const row = rows[item.index];
              if (!row) return null;
              return (
                <TreeItem
                  key={row.id}
                  id={rowId(row.id)}
                  row={row}
                  icon={typeOf(row)}
                  top={item.start}
                  height={item.size}
                  active={row.id === active}
                  selected={row.id === selected}
                  drop={drop?.id === row.id ? drop.zone : null}
                  onToggle={() => {
                    expand(row.id, !row.expanded);
                  }}
                  onSelect={() => {
                    setActive(row.id);
                    onSelect(row.id);
                  }}
                />
              );
            })}
          </div>
        </div>
        <DragOverlay>
          {dragging ? (
            <div className="dbme-tree__drag">{dragging.summary.object.title || "Untitled"}</div>
          ) : null}
        </DragOverlay>
      </DndContext>
      <MoveDialog
        object={moving}
        onClose={() => {
          setMoving(null);
        }}
      />
    </nav>
  );
}

function TreeItem({
  id,
  row,
  icon,
  top,
  height,
  active,
  selected,
  drop,
  onToggle,
  onSelect,
}: {
  id: string;
  row: TreeRow;
  icon: string | null;
  top: number;
  height: number;
  active: boolean;
  selected: boolean;
  drop: Drop["zone"] | null;
  onToggle: () => void;
  onSelect: () => void;
}) {
  const draggable = useDraggable({ id: row.id });
  const droppable = useDroppable({ id: row.id });
  const title = row.summary.object.title || "Untitled";
  const classes = [
    "dbme-tree__item",
    active ? "is-active" : "",
    selected ? "is-selected" : "",
    draggable.isDragging ? "is-dragging" : "",
    drop ? `drop-${drop}` : "",
  ]
    .filter(Boolean)
    .join(" ");
  return (
    <div
      ref={(element) => {
        draggable.setNodeRef(element);
        droppable.setNodeRef(element);
      }}
      id={id}
      role="treeitem"
      aria-level={row.level}
      aria-setsize={row.siblings}
      aria-posinset={row.position}
      aria-expanded={row.hasChildren ? row.expanded : undefined}
      aria-selected={selected}
      className={classes}
      style={{
        height: `${String(height)}px`,
        transform: `translateY(${String(top)}px)`,
        paddingInlineStart: `${String((row.level - 1) * 1)}rem`,
      }}
      onClick={onSelect}
      {...draggable.listeners}
    >
      {row.hasChildren ? (
        <span
          className="dbme-tree__toggle"
          aria-hidden="true"
          onClick={(event) => {
            event.stopPropagation();
            onToggle();
          }}
        >
          {row.expanded ? <ChevronDown size={16} /> : <ChevronRight size={16} />}
        </span>
      ) : (
        <span className="dbme-tree__toggle" aria-hidden="true" />
      )}
      <TypeIcon name={icon} size={16} className="dbme-tree__icon" />
      <span className="dbme-tree__title">{title}</span>
    </div>
  );
}
