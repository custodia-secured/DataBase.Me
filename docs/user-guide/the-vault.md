# The vault and where it lives

A vault is one folder whose name ends in `.dbme`. Everything you put in the app is in it, and
nothing about you is anywhere else except the settings file (your mode, theme and the like, which
belong to you and your screen, not to the data).

```
Main.dbme/
  vault.sqlite        the database: every page, type, field, relation, block and the change log
  vault.sqlite-wal    the database's write-ahead log; part of the database while the app runs
  vault.lock          held by the app that has the vault open
  blobs/              attached files, named by their content hash
  files/              the files folder: every page as Markdown (optional; can live elsewhere)
  FORMAT.md           the specification of all of the above, written by the app
```

`vault.sqlite` and `blobs/` together are the vault. The files folder is a second copy in a form
every other tool can read, and can be regenerated from the database; an export folder, when you
make one, is a third.

## Moving and copying

- **Move a vault** by moving the folder while the app is closed, then open it from Settings
  (**Open another vault**, its folder). The app records the last vault opened and goes back to it.
- **Copy a vault** by copying the folder while the app is closed, or with the files folder
  included while it is open. A copy taken while the app runs must include `vault.sqlite-wal`
  beside `vault.sqlite`, or use SQLite's own backup; a plain copy of `vault.sqlite` alone can
  miss the last changes.
- **Back it up** like any folder. The change log inside the database means a backup from last
  week opens as the vault was last week, with its full history to that point.
- **Two computers at once** is not supported in 0.1: one app holds the lock on a vault at a time,
  and a vault in a syncing cloud folder is only safe if one machine uses it at a time. Sync is a
  later milestone; until then, the files folder in a cloud folder is the way to read your pages
  elsewhere, and the app detects a cloud drive's conflicting copies and puts them in the
  Conflicts inbox rather than guessing.

## Several vaults

Settings makes a new vault in a folder you name (**Make a new vault in a folder**: the vault is
made inside it as `Main.dbme`) and opens another by its folder. One vault is open at a time; the
app forgets what it showed when it switches, so the second vault starts on its own first page.

## Reading a vault without the app

`FORMAT.md` inside the vault is the complete specification, licensed so that anyone may write a
reader: SQLite and a Markdown parser are enough to recover every object, field, relation, block
and file. The files folder is the quickest way for a person, and the export (Settings, **Export
this vault to a folder**) writes the types, saved views and one CSV per type for a spreadsheet,
with the change log and the attached files added by the full export, from which the command-line
tool rebuilds the vault to the byte.

## What is not in the vault

Your settings (mode, theme, update checks, the last vault, whether setup ran) are in
`settings.json` in the app's configuration folder ([getting started](getting-started.md) lists
it per system). A vault copied to another person opens in their mode and theme, not yours.
