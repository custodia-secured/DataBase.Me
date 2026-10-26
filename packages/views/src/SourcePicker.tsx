// SPDX-License-Identifier: AGPL-3.0-or-later
/** Chooses what a Table or Gallery lists: the object's children, or every object of a type. */
import { useTypes, type Id, type Source } from "@databaseme/client";
import { useId } from "react";
import { childrenOf } from "./paging";

export function SourcePicker({
  objectId,
  source,
  onChange,
}: {
  objectId: Id;
  source: Source;
  onChange: (source: Source) => void;
}) {
  const types = useTypes();
  const id = useId();
  const value = source.kind === "type" ? source.type_id : "";
  return (
    <span className="dbme-source">
      <label htmlFor={id}>Show</label>
      <select
        id={id}
        className="dbme-input"
        value={value}
        onChange={(event) => {
          const typeId = event.target.value;
          onChange(typeId ? { kind: "type", type_id: typeId } : childrenOf(objectId));
        }}
      >
        <option value="">Pages inside this one</option>
        {(types.data ?? []).map((type) => (
          <option key={type.id} value={type.id}>
            All {type.plural ?? type.name}
          </option>
        ))}
      </select>
    </span>
  );
}
