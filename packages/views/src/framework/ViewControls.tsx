// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * The bar every list-reading view shares (M8, ADR-0031): what it shows, the filter, saved views and
 * the switch to its accessible alternative. Dialogs rather than header controls, as the Table's
 * columns (ADR-0030), so every step works by keyboard and by touch. Simple mode has no types or
 * fields to filter by, so it keeps only the switch.
 */
import {
  useFields,
  useViews,
  type Condition_Deserialize,
  type Id,
  type Value,
  type ViewRow_Serialize,
} from "@databaseme/client";
import { Button, Dialog, IconButton, Plus, StatusMessage, X } from "@databaseme/ui";
import { useId, useMemo, useRef, useState, type ReactNode, type SyntheticEvent } from "react";
import { actions } from "../actions";
import { showsTypes, useMode } from "../mode";
import { SourcePicker } from "../SourcePicker";
import {
  comparisonsFor,
  conditionToRow,
  fieldNamed,
  filterFields,
  newRow,
  rowToCondition,
  startingText,
  takesValue,
  type FilterField,
  type FilterRow,
} from "./filters";
import { listingQuery, type Listing } from "./listing";
import type { SavedConfig, ViewState } from "./useViewState";

export function ViewControls<O>({
  state,
  objectId,
  renderer,
  alternative = false,
  children,
}: {
  state: ViewState<O>;
  objectId: Id;
  /** The view's ID, which a saved view records as its renderer. */
  renderer: string;
  /** Whether the view has an accessible alternative to switch to. */
  alternative?: boolean;
  children?: ReactNode;
}) {
  const organizing = showsTypes(useMode());
  const switchId = useId();
  return (
    <div className="dbme-view-bar">
      <SourcePicker
        objectId={objectId}
        source={state.listing.source}
        onChange={(source) => {
          // Another source has other fields, so what filtered this one may mean nothing there.
          state.setListing({ source, conditions: [] });
        }}
      />
      {organizing ? (
        <FilterButton
          listing={state.listing}
          onChange={(conditions) => {
            state.setListing({ ...state.listing, conditions });
          }}
        />
      ) : null}
      {children}
      {alternative ? (
        <span className="dbme-source">
          <input
            id={switchId}
            type="checkbox"
            checked={state.asList}
            onChange={(event) => {
              state.setAsList(event.target.checked);
            }}
          />
          <label htmlFor={switchId}>Show as list</label>
        </span>
      ) : null}
      {organizing ? <SavedViews state={state} objectId={objectId} renderer={renderer} /> : null}
    </div>
  );
}

function FilterButton({
  listing,
  onChange,
}: {
  listing: Listing;
  onChange: (conditions: readonly Condition_Deserialize[]) => void;
}) {
  const [open, setOpen] = useState(false);
  const count = listing.conditions.length;
  return (
    <>
      <Button
        onClick={() => {
          setOpen(true);
        }}
      >
        {count === 0 ? "Filter" : `Filter (${String(count)})`}
      </Button>
      {/* Mounted only while open, so it starts from what the listing holds now. */}
      {open ? (
        <FilterDialog
          listing={listing}
          onApply={(conditions) => {
            onChange(conditions);
            setOpen(false);
          }}
          onClose={() => {
            setOpen(false);
          }}
        />
      ) : null}
    </>
  );
}

function FilterDialog({
  listing,
  onApply,
  onClose,
}: {
  listing: Listing;
  onApply: (conditions: readonly Condition_Deserialize[]) => void;
  onClose: () => void;
}) {
  const own = useFields(listing.source);
  const fields = useMemo(() => filterFields(own.data ?? []), [own.data]);
  const nextId = useRef(listing.conditions.length);
  const [rows, setRows] = useState<FilterRow[]>(() =>
    listing.conditions.map((condition, index) => conditionToRow(index, condition, fields)),
  );
  const [errors, setErrors] = useState<ReadonlyMap<number, string>>(new Map());

  const change = (id: number, patch: Partial<FilterRow>) => {
    setRows((current) => current.map((row) => (row.id === id ? { ...row, ...patch } : row)));
    setErrors(new Map());
  };

  const apply = (event: SyntheticEvent) => {
    event.preventDefault();
    const conditions: Condition_Deserialize[] = [];
    const found = new Map<number, string>();
    for (const row of rows) {
      const result = rowToCondition(row, fieldNamed(fields, row.field));
      if ("error" in result) found.set(row.id, result.error);
      else conditions.push(result.condition);
    }
    setErrors(found);
    if (found.size === 0) onApply(conditions);
  };

  return (
    <Dialog open onClose={onClose} title="Filter">
      <form className="dbme-filter" onSubmit={apply}>
        {rows.length === 0 ? (
          <p className="dbme-muted">Nothing is filtered out. Add a condition to narrow the list.</p>
        ) : null}
        {rows.map((row, index) => (
          <ConditionRow
            key={row.id}
            row={row}
            position={index + 1}
            fields={fields}
            error={errors.get(row.id)}
            onChange={(patch) => {
              change(row.id, patch);
            }}
            onRemove={() => {
              setRows((current) => current.filter((other) => other.id !== row.id));
              setErrors(new Map());
            }}
          />
        ))}
        <div>
          <Button
            onClick={() => {
              const id = nextId.current++;
              const field = fields[0];
              if (field) setRows((current) => [...current, newRow(id, field)]);
            }}
          >
            <Plus aria-hidden="true" size={16} /> Add condition
          </Button>
        </div>
        <div className="dbme-dialog__actions">
          <Button onClick={onClose}>Cancel</Button>
          <Button variant="primary" type="submit">
            Apply
          </Button>
        </div>
      </form>
    </Dialog>
  );
}

function ConditionRow({
  row,
  position,
  fields,
  error,
  onChange,
  onRemove,
}: {
  row: FilterRow;
  position: number;
  fields: readonly FilterField[];
  error: string | undefined;
  onChange: (patch: Partial<FilterRow>) => void;
  onRemove: () => void;
}) {
  const base = useId();
  const field = fieldNamed(fields, row.field);
  const comparisons = comparisonsFor(field.kind);
  return (
    <fieldset className="dbme-filter__row">
      <legend className="dbme-visually-hidden">Condition {position}</legend>
      <label className="dbme-visually-hidden" htmlFor={`${base}-field`}>
        Field
      </label>
      <select
        id={`${base}-field`}
        className="dbme-input"
        value={row.field}
        onChange={(event) => {
          const next = fieldNamed(fields, event.target.value);
          onChange({
            field: next.key,
            op: comparisonsFor(next.kind)[0]?.op ?? "is_not_empty",
            text: startingText(next),
          });
        }}
      >
        {fields.map((option) => (
          <option key={option.key} value={option.key}>
            {option.label}
          </option>
        ))}
      </select>
      <label className="dbme-visually-hidden" htmlFor={`${base}-op`}>
        Comparison
      </label>
      <select
        id={`${base}-op`}
        className="dbme-input"
        value={row.op}
        onChange={(event) => {
          const op = comparisons.find((comparison) => comparison.op === event.target.value)?.op;
          if (op) onChange({ op });
        }}
      >
        {comparisons.map((comparison) => (
          <option key={comparison.op} value={comparison.op}>
            {comparison.label}
          </option>
        ))}
      </select>
      {takesValue(row.op) ? (
        <ValueInput
          id={`${base}-value`}
          field={field}
          text={row.text}
          invalid={error !== undefined}
          onChange={(text) => {
            onChange({ text });
          }}
        />
      ) : null}
      <IconButton label={`Remove condition ${String(position)}`} icon={X} onClick={onRemove} />
      {error === undefined ? null : <StatusMessage tone="error">{error}</StatusMessage>}
    </fieldset>
  );
}

function ValueInput({
  id,
  field,
  text,
  invalid,
  onChange,
}: {
  id: string;
  field: FilterField;
  text: string;
  invalid: boolean;
  onChange: (text: string) => void;
}) {
  const label = (
    <label className="dbme-visually-hidden" htmlFor={id}>
      Value
    </label>
  );
  if (field.kind === "boolean" || field.kind === "select" || field.kind === "multi_select") {
    const choices =
      field.kind === "boolean"
        ? [
            { id: "true", label: "Yes" },
            { id: "false", label: "No" },
          ]
        : field.options;
    return (
      <>
        {label}
        <select
          id={id}
          className="dbme-input"
          value={text}
          aria-invalid={invalid}
          onChange={(event) => {
            onChange(event.target.value);
          }}
        >
          {choices.map((choice) => (
            <option key={choice.id} value={choice.id}>
              {choice.label}
            </option>
          ))}
        </select>
      </>
    );
  }
  const dated = field.kind === "date" || field.kind === "datetime" || field.kind === "event";
  return (
    <>
      {label}
      <input
        id={id}
        className="dbme-input"
        type="text"
        autoComplete="off"
        placeholder={dated ? "YYYY-MM-DD" : undefined}
        value={text}
        aria-invalid={invalid}
        onChange={(event) => {
          onChange(event.target.value);
        }}
      />
    </>
  );
}

function SavedViews<O>({
  state,
  objectId,
  renderer,
}: {
  state: ViewState<O>;
  objectId: Id;
  renderer: string;
}) {
  const views = useViews();
  const pickerId = useId();
  const [naming, setNaming] = useState(false);
  const saved = useMemo(
    () =>
      (views.data ?? []).filter(
        (view) => view.renderer === renderer && view.owner_object_id === objectId,
      ),
    [views.data, renderer, objectId],
  );
  const current: ViewRow_Serialize | undefined = saved.find((view) => view.id === state.savedId);

  const asNew = (name: string) => {
    const config: SavedConfig<O> = { options: state.options, as_list: state.asList };
    return {
      name,
      renderer,
      query: listingQuery(state.listing, state.sort),
      config: config as Value,
      owner_object_id: objectId,
    };
  };

  return (
    <>
      {saved.length === 0 ? null : (
        <span className="dbme-source">
          <label htmlFor={pickerId}>Saved view</label>
          <select
            id={pickerId}
            className="dbme-input"
            value={current?.id ?? ""}
            onChange={(event) => {
              const chosen = saved.find((view) => view.id === event.target.value);
              if (chosen) state.load(chosen);
              else state.reset();
            }}
          >
            <option value="">None</option>
            {saved.map((view) => (
              <option key={view.id} value={view.id}>
                {view.name}
              </option>
            ))}
          </select>
        </span>
      )}
      {current ? (
        <>
          <Button
            onClick={() => {
              void actions.updateView(current.id, asNew(current.name));
            }}
          >
            Update view
          </Button>
          <Button
            onClick={() => {
              void actions.deleteView(current.id);
              state.reset();
            }}
          >
            Delete view
          </Button>
        </>
      ) : null}
      <Button
        onClick={() => {
          setNaming(true);
        }}
      >
        Save view
      </Button>
      {naming ? (
        <SaveDialog
          onClose={() => {
            setNaming(false);
          }}
          onSave={async (name) => {
            const id = await actions.saveView(asNew(name));
            if (id !== undefined) state.setSavedId(id);
            return id !== undefined;
          }}
        />
      ) : null}
    </>
  );
}

function SaveDialog({
  onSave,
  onClose,
}: {
  onSave: (name: string) => Promise<boolean>;
  onClose: () => void;
}) {
  const id = useId();
  const [name, setName] = useState("");
  const [failed, setFailed] = useState(false);
  const submit = async (event: SyntheticEvent) => {
    event.preventDefault();
    if (name.trim() === "") return;
    if (await onSave(name.trim())) onClose();
    else setFailed(true);
  };
  return (
    <Dialog open onClose={onClose} title="Save view">
      <form className="dbme-columns__form" onSubmit={(event) => void submit(event)}>
        <label htmlFor={id}>Name</label>
        <input
          id={id}
          className="dbme-input"
          type="text"
          autoComplete="off"
          value={name}
          onChange={(event) => {
            setName(event.target.value);
            setFailed(false);
          }}
        />
        {failed ? <StatusMessage tone="error">The view was not saved.</StatusMessage> : null}
        <div className="dbme-dialog__actions">
          <Button onClick={onClose}>Cancel</Button>
          <Button variant="primary" type="submit" disabled={name.trim() === ""}>
            Save
          </Button>
        </div>
      </form>
    </Dialog>
  );
}
