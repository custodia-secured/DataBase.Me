// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * The Graph view (M8, Task 11, ADR-0034): how what is below a page, or the whole vault, joins up, by its
 * relations and its parent links. Up to 200,000 nodes on WebGL. "Show as list" has every node with its
 * neighbours, and a phone starts there.
 */
import { MANUAL, useGraph, useTypes, type GraphData, type Id } from "@databaseme/client";
import { Button, StatusMessage, useMediaQuery } from "@databaseme/ui";
import { useId, useMemo, useState } from "react";
import { useViewState } from "../framework/useViewState";
import { ViewControls } from "../framework/ViewControls";
import type { ViewProps } from "../registry";
import {
  colourSlot,
  graphQuery,
  graphSummary,
  indexLinks,
  linksOf,
  MOST_NODES,
  titleOf,
  TYPE_COLOURS,
  type GraphOptions,
  type GraphScope,
  type LinkIndex,
} from "./graph";
import { GraphCanvas } from "./GraphCanvas";
import { NodeList } from "./NodeList";

const OPTIONS: GraphOptions = { scope: "here" };

function Chosen({
  data,
  links,
  node,
  typeName,
  onOpen,
}: {
  data: GraphData;
  links: LinkIndex;
  node: number;
  typeName: (node: number) => string;
  onOpen: (id: Id) => void;
}) {
  const { related, parent, children } = linksOf(links, node);
  const parts = [
    typeName(node),
    `${related.length.toLocaleString("en")} related`,
    ...(parent === null ? [] : [`inside ${titleOf(data.titles, parent)}`]),
    ...(children.length === 0 ? [] : [`holds ${children.length.toLocaleString("en")}`]),
  ];
  const id = data.ids[node];
  return (
    <div className="dbme-graph__selected" role="status">
      <span className="dbme-graph__selected-name">
        <strong>{titleOf(data.titles, node)}</strong> <span>{parts.join(", ")}</span>
      </span>
      <Button
        onClick={() => {
          if (id !== undefined) onOpen(id);
        }}
      >
        Open
      </Button>
    </div>
  );
}

/** What the colours and lines stand for. The list says all of it again in words. */
function Key({ data, names }: { data: GraphData; names: ReadonlyMap<Id, string> }) {
  const shown = data.types.slice(0, TYPE_COLOURS);
  const hasOther = data.node_types.some((type) => colourSlot(type) < 0);
  const hasLinks = data.edges.length + data.parent_edges.length > 0;
  if (shown.length === 0 && !hasLinks) return null;
  return (
    <div className="dbme-graph__key">
      {shown.length > 0 ? (
        <ul className="dbme-graph__types">
          {shown.map((type, slot) => (
            <li key={type}>
              <span
                className="dbme-graph__swatch"
                style={{ backgroundColor: `var(--dbme-chart-${String(slot + 1)})` }}
              />
              {names.get(type) ?? "A type"}
            </li>
          ))}
          {hasOther ? (
            <li>
              <span
                className="dbme-graph__swatch"
                style={{ backgroundColor: "var(--dbme-color-fg-muted)" }}
              />
              Other
            </li>
          ) : null}
        </ul>
      ) : null}
      {hasLinks ? (
        <p>
          An arrow is a relation, from the one that made it. A plain line joins a page to what is in
          it.
        </p>
      ) : null}
    </div>
  );
}

export function GraphView({ objectId, onOpen }: ViewProps) {
  const phone = useMediaQuery("(max-width: 767px)");
  const state = useViewState<GraphOptions>(objectId, OPTIONS, MANUAL, phone);
  const { listing, options } = state;
  const scopeId = useId();
  const query = useMemo(() => graphQuery(listing, options.scope), [listing, options.scope]);
  const graph = useGraph(query, MOST_NODES);
  const types = useTypes();
  const [selected, setSelected] = useState<Id | null>(null);
  const [cannotDraw, setCannotDraw] = useState(false);
  const data = graph.data;
  const links = useMemo(() => (data ? indexLinks(data) : null), [data]);
  const nodeOf = useMemo(
    () => new Map<Id, number>((data?.ids ?? []).map((id, node) => [id, node])),
    [data?.ids],
  );
  const names = useMemo(
    () => new Map((types.data ?? []).map((type) => [type.id, type.name])),
    [types.data],
  );
  const typeName = (node: number) => {
    const type = data?.types[data.node_types[node] ?? -1];
    return type === undefined ? "No type" : (names.get(type) ?? "A type");
  };
  const summary = data ? graphSummary(data) : "";
  const chosen = selected === null ? undefined : nodeOf.get(selected);
  const empty = data?.ids.length === 0;

  return (
    <section className="dbme-graph" aria-label="Graph">
      <ViewControls state={state} objectId={objectId} renderer="graph" alternative>
        {listing.source.kind === "type" ? null : (
          <span className="dbme-source">
            <label htmlFor={scopeId}>Reach</label>
            <select
              id={scopeId}
              className="dbme-input"
              value={options.scope}
              onChange={(event) => {
                state.setOptions({ ...options, scope: event.target.value as GraphScope });
                setSelected(null);
              }}
            >
              <option value="here">All below this page</option>
              <option value="vault">The whole vault</option>
            </select>
          </span>
        )}
      </ViewControls>
      <p className="dbme-graph__count" role="status">
        {data ? summary : "Reading the graph…"}
      </p>
      {graph.isError ? (
        <StatusMessage tone="error">The graph could not be read.</StatusMessage>
      ) : null}
      {data?.truncated ? (
        <StatusMessage>
          This shows the first {data.ids.length.toLocaleString("en")}, and there are more. A filter
          narrows it.
        </StatusMessage>
      ) : null}
      {cannotDraw ? (
        <StatusMessage>This device cannot draw the graph, so here is the list.</StatusMessage>
      ) : null}
      {empty ? (
        <StatusMessage>
          There is nothing here to draw yet. The whole vault may have more.
        </StatusMessage>
      ) : data && links ? (
        state.asList || cannotDraw ? (
          <NodeList
            data={data}
            links={links}
            typeName={typeName}
            selected={selected}
            onSelect={setSelected}
            onOpen={onOpen}
          />
        ) : (
          <>
            {chosen === undefined ? null : (
              <Chosen data={data} links={links} node={chosen} typeName={typeName} onOpen={onOpen} />
            )}
            <GraphCanvas
              key={objectId}
              data={data}
              selected={selected}
              summary={summary}
              onSelect={setSelected}
              onFailed={() => {
                setCannotDraw(true);
              }}
            />
            <Key data={data} names={names} />
          </>
        )
      ) : null}
    </section>
  );
}
