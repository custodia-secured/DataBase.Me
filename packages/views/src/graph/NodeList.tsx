// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * The Graph's alternative (M8, Task 11, ADR-0034): every node, searchable, with what the picture draws
 * written out in columns, and what lies within a few relations of the chosen one, read from the store by
 * `related_to` with a depth (M4 Decision 5). It is also what a machine that cannot draw a graph gets, and
 * what a phone starts on.
 */
import {
  MANUAL,
  useQueryCount,
  useQueryResults,
  type GraphData,
  type Id,
  type Query_Deserialize,
} from "@databaseme/client";
import { Button, StatusMessage } from "@databaseme/ui";
import { useDeferredValue, useEffect, useId, useMemo, useState } from "react";
import { useVirtualGrid } from "../framework/useVirtualGrid";
import { VirtualGrid } from "../framework/VirtualGrid";
import { linksOf, namesOf, search, titleOf, type LinkIndex } from "./graph";

/** The store's own bound on how far `related_to` reaches. */
const DEPTHS = [1, 2, 3, 4, 5, 6] as const;
/** How many of those results the panel lists. */
const MOST_NEARBY = 200;

function Nearby({ id, title, onOpen }: { id: Id; title: string; onOpen: (id: Id) => void }) {
  const base = useId();
  const [depth, setDepth] = useState<number>(DEPTHS[0]);
  const query = useMemo<Query_Deserialize>(
    () => ({
      from: {},
      where: [],
      related_to: { id, direction: "either", depth },
      sort: MANUAL,
      limit: MOST_NEARBY,
      offset: 0,
    }),
    [id, depth],
  );
  const rows = useQueryResults(query);
  const count = useQueryCount(query);
  const total = count.data ?? 0;
  return (
    <section className="dbme-graph__nearby" aria-label={`Near ${title}`}>
      <div className="dbme-view-bar">
        <label htmlFor={`${base}-depth`}>Within</label>
        <select
          id={`${base}-depth`}
          className="dbme-input"
          value={depth}
          onChange={(event) => {
            setDepth(Number(event.target.value));
          }}
        >
          {DEPTHS.map((option) => (
            <option key={option} value={option}>
              {option === 1 ? "1 relation" : `${String(option)} relations`}
            </option>
          ))}
        </select>
        <span>of {title}</span>
        <span className="dbme-graph__count" role="status">
          {count.isSuccess ? `${total.toLocaleString("en")} ${total === 1 ? "item" : "items"}` : ""}
        </span>
      </div>
      {rows.isError ? <StatusMessage tone="error">This could not be read.</StatusMessage> : null}
      {total > MOST_NEARBY ? (
        <StatusMessage>This shows the first {MOST_NEARBY} of them.</StatusMessage>
      ) : null}
      {(rows.data ?? []).length > 0 ? (
        <ul className="dbme-graph__nearby-list">
          {(rows.data ?? []).map(({ object }) => (
            <li key={object.id}>
              <Button
                onClick={() => {
                  onOpen(object.id);
                }}
              >
                {object.title || "Untitled"}
              </Button>
            </li>
          ))}
        </ul>
      ) : null}
    </section>
  );
}

export function NodeList({
  data,
  links,
  typeName,
  selected,
  onSelect,
  onOpen,
}: {
  data: GraphData;
  links: LinkIndex;
  /** A node's type, in words. */
  typeName: (node: number) => string;
  selected: Id | null;
  onSelect: (id: Id | null) => void;
  onOpen: (id: Id) => void;
}) {
  const base = useId();
  const [text, setText] = useState("");
  const deferred = useDeferredValue(text);
  const found = useMemo(() => search(data.titles, deferred), [data.titles, deferred]);
  const total = found?.length ?? data.ids.length;
  const nodeAt = (row: number) => (found ? found[row] : row);
  const startRow = useMemo(() => {
    const at = selected === null ? -1 : data.ids.indexOf(selected);
    return at < 0 ? 0 : at;
    // Where the list begins, so it is read once: from then on the list is what chooses.
    // eslint-disable-next-line react-hooks/exhaustive-deps -- see above
  }, []);
  const grid = useVirtualGrid(total, 5, startRow);
  const activeNode = nodeAt(grid.active.row);
  const activeId = activeNode === undefined ? undefined : data.ids[activeNode];
  const { scrollToRow } = grid;

  useEffect(() => {
    if (startRow > 0) scrollToRow(startRow);
    // Once, when the list opens on the node the picture had chosen.
    // eslint-disable-next-line react-hooks/exhaustive-deps -- see above
  }, []);

  useEffect(() => {
    onSelect(activeId ?? null);
    // eslint-disable-next-line react-hooks/exhaustive-deps -- `onSelect` is a new function each time
  }, [activeId]);

  return (
    <div className="dbme-graph__list">
      <div className="dbme-view-bar">
        <label htmlFor={`${base}-find`}>Find</label>
        <input
          id={`${base}-find`}
          className="dbme-input"
          type="search"
          autoComplete="off"
          value={text}
          onChange={(event) => {
            setText(event.target.value);
            grid.setActive({ row: 0, col: 0 });
          }}
        />
        <span className="dbme-graph__count" role="status">
          {found ? `${found.length.toLocaleString("en")} found` : ""}
        </span>
      </div>
      <VirtualGrid
        grid={grid}
        label="Nodes"
        headers={["Title", "Type", "Related to", "Inside", "Holds"]}
        row={(row) => {
          const node = nodeAt(row);
          if (node === undefined) return undefined;
          const { related, parent, children } = linksOf(links, node);
          return [
            titleOf(data.titles, node),
            typeName(node),
            namesOf(data.titles, related),
            parent === null ? "" : titleOf(data.titles, parent),
            children.length === 0 ? "" : children.length.toLocaleString("en"),
          ];
        }}
        onOpen={(row) => {
          const node = nodeAt(row);
          const id = node === undefined ? undefined : data.ids[node];
          if (id !== undefined) onOpen(id);
        }}
        emptyWhen={total === 0}
      />
      {activeId !== undefined && activeNode !== undefined ? (
        <Nearby id={activeId} title={titleOf(data.titles, activeNode)} onOpen={onOpen} />
      ) : null}
    </div>
  );
}
