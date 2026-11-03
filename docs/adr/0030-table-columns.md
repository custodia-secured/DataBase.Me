# ADR-0030: Adding columns to a Table

- **Status:** Proposed
- **Date:** 2026-11-03
- **Milestone:** after M7 (issue #15)

## Context

The brief promises user-defined types and fields with no code, but no milestone built a way to make
them. M2 scoped the Table to editing field values and sorting. The 25 built-in types are seeded with
no fields, so only an import could create a field, and a Table of ordinary pages was a list with one
column. The Table's tests did not see this, because the e2e seed writes fields straight to the store.

A column is not the Table's to own. It is a field, every field belongs to one type, and the Table
shows the fields of the types its rows have (ADR-0017). That is what makes a row in a Table the same
object in every other view, and it stays. When **Show** names a type, a new column has an obvious
home. "Pages inside this one" names no type, and those pages may have no type, or several.

## Decision

**New column** adds a field, and says which type it goes on before it is added. The rule is in one
place, `Reader::column_target`:

1. A Table of a type: that type, built-in or not. The person chose it.
2. A Table of children: the first non-system type, by name, that every live child has.
3. Otherwise a type named after the parent page, made if no type has that name, and given to every
   child. A page with no title gives `Untitled`. A page named like a built-in type gives
   `<title> pages`, so that a page called Journal does not add columns to every Journal.

Children that share only a built-in type therefore get a second type. A Table of Notes does not
change what every Note in the vault shows unless **Show** says all Notes.

A column shows through the types of the rows, so a Table of children with no rows also gets a first
row, in the same step. **New row** uses `row_add`, which gives a new row the type the Table lists, or
the types all its siblings share, so the columns stay on it.

Adding a column is one transaction and so one undo step, whatever it made. `field_rename` keeps the
key, so values stay with the field. `field_delete` leaves values on the objects, as `delete_field`
always has. A label cannot repeat one the Table shows, ignoring case, or be `Title`. A column can
hold the kinds a Table's cells edit: text, number, date, yes or no, a choice from a list, web
address, email and phone.

The controls are buttons beside **New row** that open dialogs. The grid stays one tab stop
(ADR-0017), and every step works by keyboard and touch.

## Consequences

- Nothing about the vault's format changes: these are the type, field and object-type ops that exist.
- Adding a column to a type changes every page of that type, including pages outside the Table. The
  dialog says so.
- A type or field op sets `full_rewrite_needed`, so a new column rewrites the files folder.
- A Table of many untyped children writes one op per child, in one undo step.
- If every row of a Table of children is deleted, its columns stop showing there, since nothing has
  their type. They are still on the type: **Show** finds them, and adding a column with a label that
  type already has says so.
- A column's kind is fixed, as a field's has been since M1, and 0.1's dialogs do not edit a list's
  choices. Both are limits to lift, not decisions against it.
- A child made elsewhere, such as from the pages list, has no type, so no type is shared until the
  next column gives one to all. Its cells still show the column.
- Any test that needs a field a person can make should make it through the UI, as
  `table-columns.spec.ts` does, not through the seed.

## Alternatives considered

- **A `+` at the end of the header row.** Where a spreadsheet has it, and the first proposal. It adds
  a control inside a grid that is one tab stop with `aria-activedescendant`, on a header row whose
  cells sort when activated, and it scrolls out of view on a wide Table.
- **Columns owned by the Table, on a saved view.** Values would need somewhere to live outside the
  types, and a column would show in the Table and nowhere else, which is the lock-in the product
  exists to avoid.
- **Always add to the shared type, built-in or not.** Fewer types, but a column on one page's Table of
  Notes would appear on every Note in the vault, from a dialog about one Table.
- **A column remembered on the parent page, so an empty Table keeps its columns.** It needs a new
  stored relationship, with a migration and a format change, to solve the rarest case.
- **A separate type editor first.** Worth having, but a person who wants a column looks at the Table.
