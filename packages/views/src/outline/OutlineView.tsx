// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * The Outline view: the objects below one object as an outliner. Each line is an object's title.
 * Enter adds a line after this one, Tab and Shift+Tab indent and outdent, Alt+↑ and Alt+↓ move a
 * line among its siblings, Backspace on an empty line deletes it, Ctrl+Enter opens it, and
 * Ctrl+↓ and Ctrl+↑ expand and collapse it. Every change is an object move or create, so the
 * sidebar and the other views follow at once; sort keys are rebalanced by the vault.
 */
import { MANUAL, usePage, type Id, type ObjectSummary_Serialize } from "@databaseme/client";
import { Button, ChevronDown, ChevronRight, Plus, StatusMessage } from "@databaseme/ui";
import {
  createContext,
  useContext,
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent,
} from "react";
import { actions } from "../actions";
import type { ViewProps } from "../registry";

const LIMIT = 1000;
const AUTO_EXPAND_LEVELS = 3;
const SAVE_AFTER_MS = 600;

interface OutlineState {
  root: Id;
  helpId: string;
  isExpanded: (id: Id, level: number) => boolean;
  setExpanded: (id: Id, open: boolean) => void;
  /**
   * The line to focus once it appears under `parent`, after a create or a move. Right after a
   * move the line may still show at its old place for a moment; only the new place takes focus.
   */
  focusNext: FocusTarget | null;
  setFocusNext: (target: FocusTarget | null) => void;
  onOpen: (id: Id) => void;
}

interface FocusTarget {
  id: Id;
  parent: Id | null;
}

const Outline = createContext<OutlineState | null>(null);

function useOutline(): OutlineState {
  const state = useContext(Outline);
  if (!state) throw new Error("outline rows must be inside an OutlineView");
  return state;
}

export function OutlineView({ objectId, onOpen }: ViewProps) {
  const helpId = useId();
  const [open, setOpen] = useState<ReadonlySet<Id>>(new Set());
  const [closed, setClosed] = useState<ReadonlySet<Id>>(new Set());
  const [focusNext, setFocusNext] = useState<FocusTarget | null>(null);
  const state = useMemo<OutlineState>(
    () => ({
      root: objectId,
      helpId,
      isExpanded: (id, level) => open.has(id) || (!closed.has(id) && level < AUTO_EXPAND_LEVELS),
      setExpanded: (id, expand) => {
        setOpen((before) => toggled(before, id, expand));
        setClosed((before) => toggled(before, id, !expand));
      },
      focusNext,
      setFocusNext,
      onOpen,
    }),
    [objectId, helpId, open, closed, focusNext, onOpen],
  );

  const addLine = async () => {
    const id = await actions.create({ title: "", parent_id: objectId, position: "last" });
    if (id !== undefined) setFocusNext({ id, parent: objectId });
  };

  return (
    <Outline.Provider value={state}>
      <section className="dbme-outline" aria-label="Outline">
        <p id={helpId} className="dbme-visually-hidden">
          Enter adds a line. Tab indents, Shift+Tab outdents. Alt+Up and Alt+Down move the line.
          Control+Enter opens it. Control+Down and Control+Up expand and collapse it.
        </p>
        <Lines parent={objectId} grandparent={null} level={1} />
        <div className="dbme-view-bar">
          <Button onClick={() => void addLine()}>
            <Plus aria-hidden="true" size={16} /> Add line
          </Button>
        </div>
      </section>
    </Outline.Provider>
  );
}

function toggled(set: ReadonlySet<Id>, id: Id, present: boolean): ReadonlySet<Id> {
  if (set.has(id) === present) return set;
  const next = new Set(set);
  if (present) next.add(id);
  else next.delete(id);
  return next;
}

function Lines({
  parent,
  grandparent,
  level,
}: {
  parent: Id;
  grandparent: Id | null;
  level: number;
}) {
  const page = usePage({ kind: "children", parent }, MANUAL, 0, LIMIT);
  const lines = page.data ?? [];
  if (page.isError)
    return <StatusMessage tone="error">These lines could not be read.</StatusMessage>;
  if (level === 1 && page.isSuccess && lines.length === 0) {
    return <StatusMessage>No lines yet. Use Add line to start an outline.</StatusMessage>;
  }
  return (
    <ul className="dbme-outline__list">
      {lines.map((line, index) => (
        <Line
          key={line.object.id}
          line={line}
          parent={parent}
          grandparent={grandparent}
          previous={lines[index - 1]?.object.id ?? null}
          next={lines[index + 1]?.object.id ?? null}
          level={level}
        />
      ))}
      {lines.length === LIMIT ? (
        <li className="dbme-muted">Showing the first {LIMIT} lines.</li>
      ) : null}
    </ul>
  );
}

/** The other title inputs of the outline, in reading order. */
function lineInputs(from: HTMLElement): HTMLInputElement[] {
  const outline = from.closest(".dbme-outline");
  return outline
    ? [...outline.querySelectorAll<HTMLInputElement>("input.dbme-outline__title")]
    : [];
}

function Line({
  line,
  parent,
  grandparent,
  previous,
  next,
  level,
}: {
  line: ObjectSummary_Serialize;
  parent: Id;
  grandparent: Id | null;
  previous: Id | null;
  next: Id | null;
  level: number;
}) {
  const outline = useOutline();
  const { id, title } = line.object;
  const [draft, setDraft] = useState<string | null>(null);
  const input = useRef<HTMLInputElement>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const hasChildren = line.child_count > 0;
  const expanded = hasChildren && outline.isExpanded(id, level);

  const { focusNext, setFocusNext } = outline;
  useEffect(() => {
    if (focusNext?.id === id && focusNext.parent === parent) {
      input.current?.focus();
      setFocusNext(null);
    }
  }, [focusNext, setFocusNext, id, parent]);

  const save = async () => {
    if (timer.current !== null) clearTimeout(timer.current);
    timer.current = null;
    const text = input.current?.value ?? title;
    if (text !== title) await actions.rename(id, text);
  };

  const move = async (to: Id | null, position: Parameters<typeof actions.move>[2]) => {
    await save();
    await actions.move(id, to, position);
    setFocusNext({ id, parent: to });
  };

  const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    const ctrl = event.ctrlKey || event.metaKey;
    const key = event.key;
    let handled = true;
    if (key === "Enter" && ctrl) {
      void save().then(() => {
        outline.onOpen(id);
      });
    } else if (key === "Enter") {
      void save().then(async () => {
        const created = await actions.create({
          title: "",
          parent_id: parent,
          position: { after: id },
        });
        if (created !== undefined) setFocusNext({ id: created, parent });
      });
    } else if (key === "Tab" && !event.shiftKey) {
      if (previous !== null) {
        outline.setExpanded(previous, true);
        void move(previous, "last");
      }
    } else if (key === "Tab" && event.shiftKey) {
      if (parent !== outline.root) void move(grandparent, { after: parent });
    } else if (key === "ArrowUp" && event.altKey) {
      if (previous !== null) void move(parent, { before: previous });
    } else if (key === "ArrowDown" && event.altKey) {
      if (next !== null) void move(parent, { after: next });
    } else if (key === "ArrowDown" && ctrl) {
      if (hasChildren) outline.setExpanded(id, true);
    } else if (key === "ArrowUp" && ctrl) {
      if (hasChildren) outline.setExpanded(id, false);
    } else if (key === "ArrowUp" || key === "ArrowDown") {
      const inputs = lineInputs(event.currentTarget);
      const at = inputs.indexOf(event.currentTarget);
      inputs[at + (key === "ArrowUp" ? -1 : 1)]?.focus();
    } else if (key === "Backspace" && event.currentTarget.value === "" && !hasChildren) {
      const inputs = lineInputs(event.currentTarget);
      inputs[inputs.indexOf(event.currentTarget) - 1]?.focus();
      void actions.trash(id);
    } else {
      handled = false;
    }
    if (handled) event.preventDefault();
  };

  const label = title === "" ? "Untitled" : title;
  return (
    <li className="dbme-outline__item">
      <div className="dbme-outline__line">
        {hasChildren ? (
          <button
            type="button"
            className="dbme-outline__toggle"
            tabIndex={-1}
            aria-expanded={expanded}
            aria-label={`${expanded ? "Collapse" : "Expand"} ${label}`}
            onClick={() => {
              outline.setExpanded(id, !expanded);
            }}
          >
            {expanded ? (
              <ChevronDown aria-hidden="true" size={16} />
            ) : (
              <ChevronRight aria-hidden="true" size={16} />
            )}
          </button>
        ) : (
          <span className="dbme-outline__bullet" aria-hidden="true" />
        )}
        <input
          ref={input}
          className="dbme-outline__title"
          value={draft ?? title}
          placeholder="Untitled"
          aria-label={`${label}, level ${String(level)}${hasChildren ? `, ${expanded ? "expanded" : "collapsed"}` : ""}`}
          aria-describedby={outline.helpId}
          onFocus={() => {
            setDraft(title);
          }}
          onChange={(event) => {
            setDraft(event.target.value);
            if (timer.current !== null) clearTimeout(timer.current);
            timer.current = setTimeout(() => void save(), SAVE_AFTER_MS);
          }}
          onBlur={() => {
            void save().then(() => {
              setDraft(null);
            });
          }}
          onKeyDown={onKeyDown}
        />
      </div>
      {expanded ? <Lines parent={id} grandparent={parent} level={level + 1} /> : null}
    </li>
  );
}
