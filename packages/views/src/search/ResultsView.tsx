// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * The results surface (ADR-0020): every result of a query, with facets to narrow it by type and by
 * the select fields of those types. A search from the palette lands here and can be saved as a
 * view; a saved view opens here, where it can be narrowed further, renamed, saved or deleted.
 */
import {
  commands,
  keys,
  TYPE_FACET,
  unwrap,
  useFacets,
  useQueryCount,
  useQueryResults,
  useTypes,
  useView,
  type Condition_Deserialize,
  type FacetBucket,
  type FieldRow,
  type Id,
  type Query_Deserialize,
  type Value,
} from "@databaseme/client";
import { Button, StatusMessage, TypeIcon } from "@databaseme/ui";
import { useQueries } from "@tanstack/react-query";
import { useId, useState } from "react";
import { actions } from "../actions";
import { showsTypes, useMode } from "../mode";

/** What the surface shows: a saved view, or a query that is not saved yet. */
export type ResultsSource = { kind: "view"; id: Id } | { kind: "search"; query: Query_Deserialize };

export interface ResultsViewProps {
  source: ResultsSource;
  onOpen: (id: Id) => void;
  /** The surface was closed, or its view deleted. */
  onClose: () => void;
  /** A search was saved as a view. */
  onSaved: (id: Id) => void;
}

/** Whether two queries ask the same thing. */
const same = (a: Query_Deserialize, b: Query_Deserialize) =>
  JSON.stringify(a) === JSON.stringify(b);

/** A facet value as text: a string as it is, anything else as JSON. */
const text = (value: Value) => (typeof value === "string" ? value : JSON.stringify(value));

function toggleType(query: Query_Deserialize, typeId: Id): Query_Deserialize {
  const types = query.from?.types ?? [];
  const next = types.includes(typeId) ? types.filter((id) => id !== typeId) : [...types, typeId];
  return { ...query, from: { ...query.from, types: next } };
}

function toggleCondition(
  query: Query_Deserialize,
  condition: Condition_Deserialize,
): Query_Deserialize {
  const conditions = query.where ?? [];
  const key = JSON.stringify(condition);
  const without = conditions.filter((c) => JSON.stringify(c) !== key);
  return {
    ...query,
    where: without.length === conditions.length ? [...conditions, condition] : without,
  };
}

const hasCondition = (query: Query_Deserialize, condition: Condition_Deserialize) =>
  (query.where ?? []).some((c) => JSON.stringify(c) === JSON.stringify(condition));

export function ResultsView({ source, onOpen, onClose, onSaved }: ResultsViewProps) {
  const view = useView(source.kind === "view" ? source.id : null);
  const base: Query_Deserialize | null =
    source.kind === "search" ? source.query : (view.data?.query_json ?? null);
  const [draft, setDraft] = useState<{ from: Query_Deserialize; query: Query_Deserialize } | null>(
    null,
  );
  // The draft narrows the query it was made from; a new base starts afresh.
  const query = draft && base && same(draft.from, base) ? draft.query : base;
  const narrow = (next: Query_Deserialize) => {
    if (base) setDraft({ from: base, query: next });
  };

  const results = useQueryResults(query);
  const count = useQueryCount(query);
  const types = useTypes();
  const typed = showsTypes(useMode());
  const typeFacet = useFacets(query, TYPE_FACET);
  const typeIds = (typeFacet.data ?? []).map((bucket) => text(bucket.value));
  const fieldLists = useQueries({
    queries: typeIds.map((typeId) => {
      const typeSource = { kind: "type" as const, type_id: typeId };
      return {
        queryKey: keys.fields(typeSource),
        queryFn: () => unwrap(commands.fieldsList(typeSource)),
      };
    }),
  });
  const facetFields: FieldRow[] = [];
  for (const list of fieldLists) {
    for (const field of list.data ?? []) {
      if (
        (field.kind === "select" || field.kind === "multi_select") &&
        !facetFields.some((known) => known.key === field.key)
      ) {
        facetFields.push(field);
      }
    }
  }
  const fieldFacets = useQueries({
    queries: facetFields.map((field) => ({
      queryKey: keys.facets(query ?? {}, field.key),
      queryFn: () => unwrap(commands.facets(query ?? {}, field.key)),
      enabled: query !== null,
    })),
  });

  const [naming, setNaming] = useState(false);
  const [name, setName] = useState("");
  const nameId = useId();
  const title =
    source.kind === "view"
      ? (view.data?.name ?? "")
      : `Search: ${source.query.search ?? ""}`.trim();

  const save = async () => {
    if (!query || name.trim() === "") return;
    if (source.kind === "view") {
      // A rename keeps everything else the view has, the query as narrowed on screen included.
      await actions.updateView(source.id, {
        name: name.trim(),
        renderer: view.data?.renderer ?? "list",
        query,
        config: view.data?.config_json ?? {},
        owner_object_id: view.data?.owner_object_id ?? null,
      });
      setDraft(null);
    } else {
      const id = await actions.saveView({ name: name.trim(), renderer: "list", query, config: {} });
      if (id !== undefined) onSaved(id);
    }
    setNaming(false);
    setName("");
  };

  const typeName = (id: string) => {
    const type = types.data?.find((candidate) => candidate.id === id);
    return type ? { name: type.name, icon: type.icon } : { name: "Untyped", icon: null };
  };
  const labelOf = (field: FieldRow, value: Value) => {
    const options = (field.options_json as { options?: { id: string; label?: string }[] }).options;
    return options?.find((option) => option.id === value)?.label ?? text(value);
  };

  if (source.kind === "view" && view.isError) {
    return (
      <section className="dbme-results" aria-label="Results">
        <StatusMessage tone="error">This view no longer exists.</StatusMessage>
        <Button onClick={onClose}>Close</Button>
      </section>
    );
  }

  return (
    <section className="dbme-results" aria-label="Results">
      <header className="dbme-results__header">
        <h2 className="dbme-results__title">{title}</h2>
        <span className="dbme-results__count" role="status">
          {count.data === undefined
            ? ""
            : `${String(count.data)} ${count.data === 1 ? "result" : "results"}`}
        </span>
        <span className="dbme-results__actions">
          {source.kind === "view" && draft && base && same(draft.from, base) ? (
            <Button
              variant="primary"
              onClick={() => {
                if (query && view.data) {
                  void actions
                    .updateView(source.id, {
                      name: view.data.name,
                      renderer: view.data.renderer,
                      query,
                      config: view.data.config_json,
                      owner_object_id: view.data.owner_object_id,
                    })
                    .then(() => {
                      setDraft(null);
                    });
                }
              }}
            >
              Save changes
            </Button>
          ) : null}
          <Button
            onClick={() => {
              setName(source.kind === "view" ? (view.data?.name ?? "") : "");
              setNaming(true);
            }}
          >
            {source.kind === "view" ? "Rename view" : "Save as view…"}
          </Button>
          {source.kind === "view" ? (
            <Button
              onClick={() => {
                void actions.deleteView(source.id).then(() => {
                  onClose();
                });
              }}
            >
              Delete view
            </Button>
          ) : null}
          <Button onClick={onClose}>Close</Button>
        </span>
      </header>
      {naming ? (
        <form
          className="dbme-results__name"
          onSubmit={(event) => {
            event.preventDefault();
            void save();
          }}
        >
          <label htmlFor={nameId}>View name</label>
          <input
            id={nameId}
            className="dbme-input"
            // The form appeared because the name was asked for.
            autoFocus
            value={name}
            onChange={(event) => {
              setName(event.target.value);
            }}
          />
          <Button type="submit" variant="primary" disabled={name.trim() === ""}>
            Save
          </Button>
          <Button
            onClick={() => {
              setNaming(false);
            }}
          >
            Cancel
          </Button>
        </form>
      ) : null}
      {query ? (
        <div className="dbme-results__facets" role="group" aria-label="Narrow the results">
          {(typed ? (typeFacet.data ?? []) : []).map((bucket: FacetBucket) => {
            const id = text(bucket.value);
            const { name: label, icon } = typeName(id);
            const pressed = (query.from?.types ?? []).includes(id);
            return (
              <button
                key={id}
                type="button"
                className="dbme-chip"
                aria-pressed={pressed}
                onClick={() => {
                  narrow(toggleType(query, id));
                }}
              >
                <TypeIcon name={icon} size={14} />
                {label} ({bucket.count})
              </button>
            );
          })}
          {facetFields.map((field, i) =>
            (fieldFacets[i]?.data ?? []).map((bucket) => {
              const condition: Condition_Deserialize = {
                field: field.key,
                op: field.kind === "multi_select" ? "has" : "eq",
                value: bucket.value,
              };
              return (
                <button
                  key={`${field.key}:${text(bucket.value)}`}
                  type="button"
                  className="dbme-chip"
                  aria-pressed={hasCondition(query, condition)}
                  onClick={() => {
                    narrow(toggleCondition(query, condition));
                  }}
                >
                  {field.label}: {labelOf(field, bucket.value)} ({bucket.count})
                </button>
              );
            }),
          )}
        </div>
      ) : null}
      {results.isSuccess && results.data.length === 0 ? (
        <StatusMessage>No results.</StatusMessage>
      ) : null}
      <ol className="dbme-results__list">
        {(results.data ?? []).map((summary) => {
          const { name: label, icon } = typeName(summary.type_ids[0] ?? "");
          return (
            <li key={summary.object.id}>
              <button
                type="button"
                className="dbme-results__row"
                onClick={() => {
                  onOpen(summary.object.id);
                }}
              >
                <TypeIcon name={icon} size={16} className="dbme-results__icon" />
                <span className="dbme-results__row-title">
                  {summary.object.title || "Untitled"}
                </span>{" "}
                {typed ? <span className="dbme-results__row-type">{label}</span> : null}
              </button>
            </li>
          );
        })}
      </ol>
    </section>
  );
}
