# ADR-0033: The map: land from the app, and a file on this device

- **Status:** Proposed
- **Date:** 2026-11-05
- **Milestone:** M8

## Context

The brief asks for a map view with user-chosen tile sources. The app promises no network request
unless a feature the person turned on needs one (ADR-0010, ADR-0028), and its CSP is strict and
static: `connect-src` names the IPC channel and nothing else. M8's gate 1b put MapLibre GL 6.16.0 on
a page with that CSP in Edge and WebKit, with its worker served from `'self'`, and it drew with no
violation. It also showed that text on a map is a request: MapLibre fetches glyphs for any layer that
draws text.

## Decision

- **The basemap ships with the app.** Natural Earth land at 1:110m (`world-atlas`, 55 kB), drawn by
  MapLibre, so that a map works offline and asks for nothing.
- **A local PMTiles file can draw over it.** The person chooses one in Settings. The file stays where
  it is and is read in ranges over IPC, through a `pmtiles://` protocol the page registers. A raster
  file draws as it is. A vector file has no style of its own here, so each of its layers draws in the
  land's colour, or the sea's if its name says it is water, with no labels.
- **The webview does not name the file it reads.** `map_file_set` takes a path and refuses what is not
  a PMTiles version 3 archive. `map_file_read` takes only a range, and reads whichever file is set,
  checking again each time that it is still an archive, and returning no more than 8 MiB. The app can
  therefore read PMTiles files by this route and no other kind of file.
- **Remote tiles wait (a deviation from the plan, Decision 5).** A tile URL needs a looser CSP or a
  request made through the shell, and either is a decision about the network of its own.
- **No geocoder (Decision 6).** It would be a request. A location is set by a click or typed as a
  latitude and longitude, in one undo, with `object_set_geo`.
- **Points cluster, and the counts are images.** Text would take glyphs, so the page draws each count on
  a canvas when the map asks for it (`setMissingStyleImageResolver`). A listener on `styleimagemissing`
  is too late for the request that fired it, and misses some counts.
- **The land is made flat before it is drawn.** `world-atlas` keeps its polygons for a sphere: one that
  crosses the antimeridian goes straight from 180 to -180. `flatRing` carries it past 180 instead, keeps
  each ring in the world most of it is in, and closes a ring that goes round a pole along that pole.
  Keeping rings in one world matters as much as the rest: Eurasia's ring starts at -180, and left a world
  to the west of the Caspian, its hole, the two were joined by a sliver around the Earth at low zoom.
- **The alternative is the list.** "Show as list" has every item with its coordinates, or "No location".
  It is also what someone gets if the machine cannot draw a map, and it says so. Setting a location has
  the same route with or without the map, in the "Place on the map" dialog.
- **A map shows at most 5,000 places,** and says so when there are more. The list has them all.

## Consequences

- A map works with no network, and a request leaving the machine fails the e2e suite as it does for
  every other view.
- The built-in map is coarse. It is enough to see where things are, and not to find a street, which is
  what the map file is for. Fitting places stops at zoom 4 on the land alone, past which it has no more to
  show.
- A vector file looks plain, since a useful style would have to know its schema. Labels would take
  glyphs, which could come from the same file in a later version.
- Places at exactly the same spot stay stacked however far in the map goes. Only the top one can be
  clicked there, and the list has them all.
- The coordinates of a place are shown to five decimal places, about a metre, whatever was typed.

## Alternatives considered

- **A tile URL with a looser CSP.** Rejected for now: a static CSP cannot name a host the person
  chooses later, and `connect-src https:` would give up the guarantee for every other view too.
- **Passing a path to each read.** Simpler, but it would make the command a way for the webview to ask
  for ranges of any file named to it, checked or not, on every call.
- **Copying the map file into the vault.** Rejected: map files run to many gigabytes, and they are not
  the person's data.
- **HTML markers for cluster counts.** They need no images, but mean a DOM node for each cluster,
  repositioned on every frame of a pan.
- **`d3-geo` to cut the land at the antimeridian.** It would do the same with a new dependency, for
  what twenty lines and a test do here.
