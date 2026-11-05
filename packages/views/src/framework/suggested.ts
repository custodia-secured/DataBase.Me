// SPDX-License-Identifier: AGPL-3.0-or-later
/** What the store says about a page's contents, in the terms the view recommender reads (M8, Task 9). */
import type { DataProfile_Serialize, FieldKind, FieldRow } from "@databaseme/client";
import type { FieldShape, FieldShapeKind, Shape } from "@databaseme/recommender";

const MS_PER_DAY = 86_400_000;
/** The store's limit on the fields of one profile (`MAX_PROFILE_FIELDS`). */
const MOST_FIELDS = 64;

const KINDS: Partial<Record<FieldKind, FieldShapeKind>> = {
  text: "text",
  rich_text: "text",
  number: "number",
  currency: "number",
  duration: "number",
  rating: "number",
  boolean: "boolean",
  date: "date",
  datetime: "date",
  select: "select",
  multi_select: "multi_select",
  file: "file",
  url: "url",
  email: "contact",
  phone: "contact",
};

/** Different fields can share a key across types, and a profile needs to hear about each key once. */
export function profileKeys(fields: readonly FieldRow[]): string[] {
  return [...new Set(fields.map((field) => field.key))].slice(0, MOST_FIELDS);
}

const whenOf = (value: unknown): number => {
  if (typeof value === "number") return value;
  return typeof value === "string" ? Date.parse(value) : Number.NaN;
};

function daysBetween(first: unknown, last: unknown): number | undefined {
  const days = (whenOf(last) - whenOf(first)) / MS_PER_DAY;
  return Number.isFinite(days) ? Math.max(days, 0) : undefined;
}

/** A formula or a rollup is whatever it works out to, so its values say what it is. */
function kindOf(kind: FieldKind, numeric: number, nonEmpty: number): FieldShapeKind {
  if (kind === "formula" || kind === "rollup") {
    return nonEmpty > 0 && numeric * 2 >= nonEmpty ? "number" : "other";
  }
  return KINDS[kind] ?? "other";
}

export function shapeOf(profile: DataProfile_Serialize, fields: readonly FieldRow[]): Shape {
  const share = (count: number) => (profile.sampled === 0 ? 0 : count / profile.sampled);
  const kinds = new Map(fields.map((field) => [field.key, field.kind]));
  return {
    objects: profile.objects,
    withEvent: share(profile.with_event),
    eventRangeDays: daysBetween(profile.event_first, profile.event_last) ?? 0,
    withSpan: share(profile.with_span),
    withGeo: share(profile.with_geo),
    withChildren: share(profile.with_children),
    withRelations: share(profile.with_relations),
    fields: profile.fields.flatMap((field): FieldShape[] => {
      const kind = kinds.get(field.key);
      if (kind === undefined) return [];
      const shapeKind = kindOf(kind, field.numeric, field.non_empty);
      const rangeDays = shapeKind === "date" ? daysBetween(field.min, field.max) : undefined;
      return [
        {
          kind: shapeKind,
          filled: share(field.non_empty),
          distinct: field.distinct,
          ...(rangeDays === undefined ? {} : { rangeDays }),
        },
      ];
    }),
  };
}
