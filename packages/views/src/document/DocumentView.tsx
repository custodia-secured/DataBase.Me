// SPDX-License-Identifier: AGPL-3.0-or-later
/** The Document view: an object's title, types, properties, text and backlinks. */
import {
  commands,
  keys,
  unwrap,
  useBacklinks,
  useObject,
  useSummary,
  useTypes,
  type FieldRow,
  type Id,
  type ObjectRow_Serialize,
  type Value,
} from "@databaseme/client";
import { IconButton, StatusMessage, TypeIcon, X } from "@databaseme/ui";
import { useQueries } from "@tanstack/react-query";
import { useId, useState, type KeyboardEvent } from "react";
import { actions, attempt } from "../actions";
import { BlockEditor } from "../editor/BlockEditor";
import { FieldEditor } from "../fields";
import type { ViewProps } from "../registry";
import { displayValue, isEditable } from "../values";

export function DocumentView({ objectId, onOpen }: ViewProps) {
  const object = useObject(objectId);
  if (object.isError)
    return <StatusMessage tone="error">This object could not be found.</StatusMessage>;
  if (!object.data) return <StatusMessage>Loading…</StatusMessage>;
  const title = object.data.title === "" ? "Untitled" : object.data.title;
  return (
    <article className="dbme-document" aria-label={title}>
      <TitleField object={object.data} />
      <TypesLine objectId={objectId} />
      <Properties object={object.data} />
      <BlockEditor objectId={objectId} onOpen={onOpen} label={`Text of ${title}`} />
      <Backlinks objectId={objectId} onOpen={onOpen} />
    </article>
  );
}

/** The title, edited in place. Enter saves it and moves into the text. */
function TitleField({ object }: { object: ObjectRow_Serialize }) {
  const [draft, setDraft] = useState<string | null>(null);
  const save = (value: string) => {
    if (value !== object.title) void actions.rename(object.id, value);
  };
  const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === "Enter") {
      event.preventDefault();
      const editor = event.currentTarget
        .closest("article")
        ?.querySelector<HTMLElement>(".dbme-editor");
      editor?.focus();
    } else if (event.key === "Escape") {
      setDraft(object.title);
      event.currentTarget.blur();
    }
  };
  return (
    <input
      className="dbme-document__title"
      aria-label="Title"
      placeholder="Untitled"
      value={draft ?? object.title}
      onFocus={() => {
        setDraft(object.title);
      }}
      onChange={(event) => {
        setDraft(event.target.value);
      }}
      onBlur={(event) => {
        save(event.target.value);
        setDraft(null);
      }}
      onKeyDown={onKeyDown}
    />
  );
}

/** The object's types, with a way to add and remove them. */
function TypesLine({ objectId }: { objectId: Id }) {
  const summary = useSummary(objectId);
  const types = useTypes();
  const selectId = useId();
  const own = summary.data?.type_ids ?? [];
  const all = types.data ?? [];
  const typeName = (id: Id) => all.find((type) => type.id === id)?.name ?? "Unknown type";
  return (
    <div className="dbme-types">
      <ul className="dbme-types__list" aria-label="Types">
        {own.map((id) => {
          const type = all.find((candidate) => candidate.id === id);
          return (
            <li key={id} className="dbme-chip">
              <TypeIcon name={type?.icon} size={14} />
              <span>{typeName(id)}</span>
              <IconButton
                label={`Remove type ${typeName(id)}`}
                icon={X}
                className="dbme-chip__remove"
                onClick={() => void attempt(() => unwrap(commands.objectRemoveType(objectId, id)))}
              />
            </li>
          );
        })}
      </ul>
      <label className="dbme-visually-hidden" htmlFor={selectId}>
        Add a type
      </label>
      <select
        id={selectId}
        className="dbme-input dbme-types__add"
        value=""
        onChange={(event) => {
          const id = event.target.value;
          if (id) void attempt(() => unwrap(commands.objectAddType(objectId, id)));
        }}
      >
        <option value="">Add a type…</option>
        {all
          .filter((type) => !own.includes(type.id))
          .map((type) => (
            <option key={type.id} value={type.id}>
              {type.name}
            </option>
          ))}
      </select>
    </div>
  );
}

/** The fields of the object's types, one per key, with their values. */
function useObjectFields(objectId: Id): FieldRow[] {
  const summary = useSummary(objectId);
  const lists = useQueries({
    queries: (summary.data?.type_ids ?? []).map((typeId) => {
      const source = { kind: "type" as const, type_id: typeId };
      return {
        queryKey: keys.fields(source),
        queryFn: () => unwrap(commands.fieldsList(source)),
      };
    }),
  });
  const fields: FieldRow[] = [];
  for (const list of lists) {
    for (const field of list.data ?? []) {
      if (!fields.some((known) => known.key === field.key)) fields.push(field);
    }
  }
  return fields;
}

function Properties({ object }: { object: ObjectRow_Serialize }) {
  const fields = useObjectFields(object.id);
  const [editing, setEditing] = useState<string | null>(null);
  if (fields.length === 0) return null;
  const save = (key: string, value: Value | null) => void actions.setProp(object.id, key, value);
  return (
    <dl className="dbme-properties" aria-label="Properties">
      {fields.map((field) => {
        const value = object.props_json[field.key];
        return (
          <div key={field.key} className="dbme-properties__row">
            <dt>{field.label}</dt>
            <dd>
              {field.kind === "boolean" ? (
                <input
                  type="checkbox"
                  aria-label={field.label}
                  checked={value === true}
                  onChange={(event) => {
                    save(field.key, event.target.checked);
                  }}
                />
              ) : editing === field.key ? (
                <FieldEditor
                  field={field}
                  value={value}
                  onCommit={(next) => {
                    save(field.key, next);
                  }}
                  onCancel={() => undefined}
                  onDone={() => {
                    setEditing(null);
                  }}
                />
              ) : (
                <button
                  type="button"
                  className="dbme-properties__value"
                  disabled={!isEditable(field.kind)}
                  aria-label={`${field.label}: ${displayValue(field, value) || "empty"}`}
                  onClick={() => {
                    setEditing(field.key);
                  }}
                >
                  {displayValue(field, value) || <span className="dbme-muted">Empty</span>}
                </button>
              )}
            </dd>
          </div>
        );
      })}
    </dl>
  );
}

/** The plain text of a block's runs, for a snippet. */
function snippetOf(content: Value | undefined): string {
  const inline = (content as { inline?: unknown } | null)?.inline;
  if (!Array.isArray(inline)) return "";
  return inline
    .map((run: unknown) => {
      const text = (run as { text?: unknown }).text;
      return typeof text === "string" ? text : "…";
    })
    .join("")
    .trim();
}

function Backlinks({ objectId, onOpen }: { objectId: Id; onOpen: (id: Id) => void }) {
  const backlinks = useBacklinks(objectId);
  const headingId = useId();
  const list = backlinks.data ?? [];
  return (
    <section className="dbme-backlinks" aria-labelledby={headingId}>
      <h2 id={headingId} className="dbme-backlinks__heading">
        Linked from {list.length > 0 ? `(${String(list.length)})` : ""}
      </h2>
      {list.length === 0 ? (
        <p className="dbme-muted">
          Nothing links here yet. Type [[ in another page to link to this one.
        </p>
      ) : (
        <ul className="dbme-backlinks__list">
          {list.map((link) => (
            <li key={link.relation.id}>
              <button
                type="button"
                className="dbme-backlinks__item"
                onClick={() => {
                  onOpen(link.source.id);
                }}
              >
                <span className="dbme-backlinks__title">{link.source.title || "Untitled"}</span>
                {link.block ? (
                  <span className="dbme-backlinks__snippet">
                    {snippetOf(link.block.content_json)}
                  </span>
                ) : null}
              </button>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
