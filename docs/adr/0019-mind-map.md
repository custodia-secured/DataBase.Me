# ADR-0019: The mind map

- **Status:** Proposed
- **Date:** 2026-11-01
- **Milestone:** M3

## Context

M3 adds a mind map over the hierarchy and relations. Edits write back, the layout is automatic, it
takes touch gestures, and it round-trips with the Outline. The brief asks for a 2,000-node map that
stays interactive, and names React Flow and either ELK or dagre.

## Decision

- **React Flow (`@xyflow/react` 12, MIT) draws the map up close.** Its stylesheet is imported as a
  file and node positions go through the CSSOM, so `style-src 'self'` holds. Its attribution link is
  hidden, because a click on it would navigate the app's webview away.
- **`d3-hierarchy` lays it out.** ELK's licences (EPL-2.0 or GPL-3.0-or-later) aren't on the
  allowlist. dagre (MIT) took 300–395 ms on 2,000 nodes, which alone is three times the 100 ms edit
  budget; the `d3-hierarchy` tidy tree took 0.3–0.5 ms.
- **Positions are computed, never stored.** The root is on the left and branches grow right. Nodes
  have a fixed size, so the layout needs no measuring pass. Free placement belongs to the canvas
  view.
- **A drag only reparents or reorders.** The middle of a node reparents onto it, and its top and
  bottom quarters place the dragged node before or after it. A node can't go into its own branch.
- **One canvas draws the map from far away.** React Flow costs per mounted node and edge on every
  frame: with all 2,011 nodes on screen it panned at 46 fps and dragged at 13 on the reference
  machine, with memoized nodes. Below zoom 0.35 no title can be read, so `Overview.tsx` draws the
  whole map as plain boxes on one canvas, and React Flow keeps only the node with the keyboard. Its
  pan and zoom still drive the view. A press on a box selects or drags it, and a double click comes
  back to reading zoom.
- **A map opens where it can be read.** It opens fitted if that fit is at reading zoom or closer.
  Otherwise it opens on the root at zoom 0.8, because a 2,011-node map fitted to a window is a sliver
  a few pixels wide. "Fit the map" still shows all of it.
- **One read for the whole map.** `object_tree(root, limit)` is one recursive query, depth first in
  sort-key order, with the relations among the nodes and a count of those that leave the map. Past
  5,000 nodes the map says it shows part of the tree.
- **Relations are dashed edges, labelled by type.** Dragging between handles or "Relate to…" makes a
  `related` relation (FORMAT.md §3.1). `relation_create` refuses a self-link and returns an existing
  relation unchanged. `relation_trash` refuses relations held by a block or a field, because they
  belong to what holds them. The list of the active node's relations floats over the map, so it
  never moves the map under the pointer.
- **Every pointer action has a keyboard and a touch way.**
  - The map is an ARIA tree with `aria-activedescendant`. React Flow's `application` role on its
    root is replaced, or it would cut the tree from its items.
  - Arrows move. Tab adds a child and Enter a sibling, as mind map tools do, so the keys differ from
    the Outline's. F2 renames, Alt+↑/↓ reorders, Delete trashes, Space folds, Ctrl+Enter opens.
  - One finger pans and a pinch zooms. A long press and a drag moves a node. "Move to…" and "Relate
    to…" need no drag at all.
  - The Outline stays the text alternative.

## Consequences

- The canvas shows no titles, and a screen reader never needs it: the keyboard's node stays mounted,
  and moving to another brings the map back to reading zoom.
- Every edit reads the tree and runs the layout again. That is cheap, and unchanged nodes keep their
  objects, so React Flow redraws only what moved.
- Up close, the budget depends on how many nodes are on screen, which reading zoom limits.
- Relation edges run from the right of one node to the left of another, so between siblings they
  loop and pass behind other nodes. They can be read, but they could be routed better.
- Zustand 4 comes with React Flow, alongside the app's Zustand 5.

## Alternatives considered

- **React Flow alone, with plain DOM boxes at low zoom.** Measured above: too slow with every node on
  screen, because the cost is per mounted element and not in its content.
- **A canvas-only map.** It would give up React Flow's handles, gestures and accessible DOM nodes up
  close, and all of that would have to be written here.
- **Stored positions.** They would add a format change, a sync surface and a free-placement feature
  that belongs to the canvas view.
