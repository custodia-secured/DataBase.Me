# Settings, modes and themes

**Settings** in the header, or `> Settings` in the palette. Everything here is yours, not the
vault's: it is kept in `settings.json` in the app's configuration folder
([getting started](getting-started.md) lists it per system), so a vault copied to another
computer carries none of it.

## Vault

- **This vault:** the path of the open vault.
- **Open another vault (its folder).** Type or pick the folder that holds a `.dbme` vault. The
  app switches to it and remembers it as the last vault opened.
- **Make a new vault in a folder.** A new `Main.dbme` is made inside the folder you name and
  opened.
- **Export this vault to a folder (new or empty).** Writes the vault's types as
  `schema/types.json`, each saved view under `views/`, and one CSV per type under `data/`, which
  any spreadsheet opens. With **Full export** checked (the default) it adds the change log
  (`ops.jsonl`), the vault's facts (`vault_meta.json`) and every attached file under `blobs/`,
  from which `dbme import` rebuilds the vault to the byte. The status line says what was written.
- **Files folder…** opens the files folder dialog ([the files folder](files-folder.md)).
- **Set up again** runs the first-launch setup once more; with a vault open it keeps the vault.

## Mode

- **Simple: a plain notes app.** Pages, text, links, search, the files folder and import; the
  Document, Outline, Mind map and Gallery views; **Today** for a dated page under Journal. Types,
  fields and the Table view are hidden.
- **Organizer: types, fields and tables too.** Everything in Simple, and the types line and
  fields on each page, the Table view, the type picker in the Gallery and Table views, and type
  facets in search results.

The mode is a filter on what is shown. Switching never changes the vault: a page typed in
Organizer keeps its types in Simple, unseen, and shows them again when you switch back.

## Theme

**Follow the system**, **Light** or **Dark**. The choice applies at once and is kept; the app opens
in the chosen theme from its first frame. Both palettes meet the WCAG 2.2 AA contrast rules for
text, controls and focus rings.

## Check for updates

Off, and in 0.1 without effect: this version has no updater, so neither choice causes a request.
The choice is kept for the first version that has one, which will send the app's version and your
address to the download host when checking. Turning it on is the only way the app will ever reach
the network on its own, and it is off until you say otherwise ([privacy](privacy.md)).

## Help

The app's version and the vault format version it writes; the addresses of the documentation and
the issue tracker, as text to copy (the app opens no browser); and **Copy diagnostic report**,
described in [reporting a problem](reporting-a-problem.md).
