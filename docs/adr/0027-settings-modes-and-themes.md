# ADR-0027: Settings, modes and themes

- **Status:** Proposed
- **Date:** 2026-11-03
- **Milestone:** M7

## Context

Up to M6 the app had no settings surface: it opened one default vault in its data folder
(ADR-0017), followed the operating system's colour scheme through a media query, and showed every
feature to everyone. The MVP's acceptance (brief §8) asks for Simple and Organizer modes, settings,
onboarding, light and dark themes, and a Settings and Help dialog with the diagnostic report. The
brief's principles bear on where those things live: a vault is portable and must carry nothing
about one person's screen or habits (§2 principle 6), and Simple mode must feel like a plain notes
app while the rest of the product stays one step away (§2.9).

FORMAT.md reserves a `themes/` folder in the vault for user themes. The MVP needs light and dark,
not a theme editor, and a choice between two built-in palettes is a preference, not content.

## Decision

**One settings file, the person's, outside the vault.** `settings.json` in the app's configuration
folder (beside the identity and the files-folder override), read and written by `dbme-api`
(`settings.rs`). Its keys in 0.1: `version` (1), `mode` (`simple` | `organizer`), `theme`
(`system` | `light` | `dark`), `update_checks` (off by default), `last_vault` (a path, or null)
and `onboarded`. Every key has a default and a missing key takes it, so an older file always loads;
a file that cannot be read or parsed is the defaults until the next write and is never touched by a
read. Writes go whole, through a temporary file and a rename. Two commands, `settings_get` and
`settings_set(patch)`; the second returns the settings as they are and the client puts them in its
cache, since one window exists in 0.1 (the planned `settings-changed` event waits for a second
window). Nothing about the screen goes into the vault; a vault copied to another machine opens as it
did, in whatever mode and theme that machine's person chose.

**Modes are presets of what is shown**, changed any time, the same on every platform. _Simple_
shows the Document, Outline, Mind map and Gallery views, the pages tree, search, the files folder,
import, and a daily journal; it hides what belongs to structure: the types line and the properties
of the Document view, the Table view, the type picker of the Gallery and Table views, and the type
facets of the results. _Organizer_ shows all of it. The mode is a filter in the interface
(`visibleViews(mode)` in the registry, `showsTypes(mode)` in the views) and never a change to the
vault: a page typed in Organizer keeps its types and fields in Simple, unseen. Researcher and
Data/Power modes are later milestones.

**The daily journal** is "Today": a Note titled `YYYY-MM-DD` (the local date) under a top-level
page named "Journal", both made on first use and opened after. It is a convention over ordinary
pages, not a kind of its own, so it exports, mirrors and imports like any page.

**Themes.** The setting puts `data-theme="light"` or `"dark"` on `<html>`; `system` removes the
attribute and the tokens' media query decides. The tokens file (`packages/ui/src/tokens.css`)
carries both palettes under the attribute and the media query, so a theme is a set of custom
properties and nothing in the components changes. No flash of the wrong theme: the shell writes the
last applied theme as a hint in `localStorage` and the page reads it before the interface mounts;
the setting then confirms or corrects it. Both palettes pass WCAG 2.2 AA contrast for text,
controls and focus rings, and axe runs in dark on the Document and Table views in the e2e suite.
The vault's `themes/` folder stays reserved for the theme editor of a later version; 0.1 ships the
two built-in palettes and FORMAT.md says so.

**Settings and Help** are one dialog, from the header and from the palette: the vault (its path,
open another, make a new one, export to a folder), the files folder (the M5 dialog, linked), the
mode, the theme, update checks, "Set up again", and Help: the version and the format version, the
documentation and issue-tracker addresses as text to copy (the app opens no browser in 0.1, so no
opener plugin and no request), and _Copy diagnostic report_. Opening or making another vault drops
the shell's selection, forgets what it read, remounts for the new vault and records it as
`last_vault`.

**The diagnostic report** is plain text: when it was made, the app and format versions, the OS
and architecture, the webview and its version, the mode, theme and update-checks setting, the
vault's object and blob counts and schema version, whether the files folder is on, and the last
twenty failures as a time and an error kind each. Never a path, a title or a message, since a
message can carry a path or a title; the client keeps the kinds in a ring of twenty. The webview's
clipboard takes it where the page is given one; where it is not (gate 1c found the write refused
under the smoke launch's conditions), the dialog shows the text to copy by hand.

## Consequences

- A setting is a key with a default, added by extending the struct; no migration of the file. A
  key whose meaning changes gets a new name.
- Hiding a feature in Simple mode is a change to the mode's filter, not a second code path. A view
  or control added later says which modes show it.
- A second window, or a change to the settings by another process, needs the `settings-changed`
  event this ADR deferred; the client's cache is the only copy now.
- The theme hint in `localStorage` is a convenience, not a source of truth; a cleared storage costs
  one frame in the wrong palette, then the setting applies.
- The report's rule, never content, holds against every future field: a field that could carry a
  path or a title does not go in.

## Alternatives considered

- **Settings in the vault.** Simpler to find, and the vault would then carry one person's screen
  to another machine and another person. Rejected by principle 6.
- **Modes as separate interfaces.** A plain-notes app and a structured one, each built for its
  users. Two interfaces to keep in step, and the step from one to the other, which the brief wants
  to be a setting, becomes a migration.
- **A theme editor in 0.1.** FORMAT.md's `themes/` folder was designed for it, and nothing in the
  MVP's acceptance needs it. The two palettes ship; the editor waits.
- **Opening the docs in the browser from Help.** One plugin and one request away from the
  zero-network promise (ADR-0028); the addresses are text to copy instead.
