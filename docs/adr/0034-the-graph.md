# ADR-0034: The graph: WebGL, a paced layout and a list

- **Status:** Proposed
- **Date:** 2026-11-05
- **Milestone:** M8

## Context

The brief asks for a WebGL graph that renders 100,000 nodes, with an accessible alternative and a phone
layout. M8's gate 1c put Sigma 4.0.4 and graphology 0.26.0 on a page with the app's CSP. They drew 100,000 nodes
and 199,996 edges in Edge and WebKit, and on CI's runners in software. ForceAtlas2's own worker is a `blob:` URL
the CSP refuses, so its iteration has to run in a worker of ours. A pass over 100,000 nodes took about half a
second there, and a Sigma refresh at that size takes about one and a half seconds on the main thread.

## Decision

- **Sigma draws what the store's `graph_read` gives** (M8 Task 2): flat arrays of nodes, relations and parent
  links, up to 200,000 nodes, and the view says when there are more.
- **A graph reaches all the way down.** "All below this page" reads `from.under`, where every other view reads one
  level, since one level has no parent links to draw. "The whole vault" is the other reach, and a type as the source
  replaces both. The reach is one of the view's saved settings, and filters apply to any of them.
- **The first frame waits for no layout.** A node starts at a place worked out from its ID alone, so it stays where
  it is when others come and go. A place from its position in the list would move everything each time one was added.
- **The layout runs in a worker of ours and is paced by the page.** The worker runs only when asked. The page asks
  again after it has drawn the last answer, and waits nine times as long as that took, so it keeps nine tenths of its
  time at any size. Pausing is not asking, so a paused layout costs nothing, and a step on its way when the pause
  came is dropped. A run stops after a number of passes that falls with size, 1,000 up to 2,000 nodes and 100 from
  20,000; "Run layout" goes on from there. **Decision 4:** the layout is not stored, and a new read keeps nodes where
  they are.
- **With reduced motion, the layout waits to be asked,** and the camera moves without animating.
- **Only a change in who is joined to whom builds the graph again.** Structural sharing keeps each array of a read
  identical if nothing in it changed, so a renamed node changes one label, and the 1.5 seconds go only on a real
  change. Every change in the vault still reads the graph again, about 200 ms at 100,000 objects.
- **Sizes are in pixels** (`itemSizesReference: "screen"`). Sigma's default ties them to the layout's positions, so a
  node grows and shrinks as the graph spreads. Links fade as the graph grows, or a large one is one solid patch.
- **An arrow is a relation and a plain line a parent link,** so the two differ in shape as well as colour. A node's
  colour is its first type, with eight colours and a shared one past them. The key says so, and the list says it all
  in words.
- **The alternative is the list.** Every node, with its type, what it is related to, what it is inside and how many it
  holds, read from the graph already in memory. It searches titles, and below it "Within N relations of this one"
  asks the store, by `related_to` with a depth (M4 Decision 5). A phone starts on the list, and it is what a machine
  that cannot draw a graph gets, and it says so. The arrow keys move through it and Enter opens.
- **On the picture, the keyboard moves the camera** (arrows, plus, minus, 0), and choosing a node is what the list is
  for. A click or tap chooses one, which says what it joins and offers Open.
- **The tests read Sigma through a handle only the e2e build has** (`window.__dbmeSigma`, behind
  `import.meta.env.MODE`, so the production build drops it). A canvas shows them nothing else.

## Consequences

- At 100,000 nodes the page stops for about 1.5 seconds roughly every 15 seconds while the layout runs, until it
  finishes or is paused. Pausing it is one press. The alternative was a layout that starts only when asked, which
  leaves a graph that large as a disc until someone knows to ask.
- The first frame at 100,000 nodes is about 5.4 s here, against about 3 s in the gate. The gate had no IPC read, no
  list index and no arrows. The brief sets no figure for it. The budget test holds it under 10 s on Edge and prints it
  elsewhere, since CI draws in software.
- A run that ends is not a converged layout at 100,000 nodes. 100 passes is about a minute of work and starts to show
  structure, and "Run layout" gives it more.
- `related_to` follows relations, and not parent links, so "Within N relations" can be empty for a node the picture
  shows with lines. The list's columns keep the two apart for this reason.
- "All below this page" leaves out the page itself, since `from.under` does. The whole vault has it.
- The real app's WebView2 window has not drawn this view. Edge stands in for it, on the same engine, as for the other
  views of this milestone.

## Alternatives considered

- **A layout that does not auto-start on large graphs.** See above. It would save the pauses and lose the plan's
  "then runs in a worker", and a person shown a disc has no reason to look for a button.
- **Applying each answer as it arrives.** At 100,000 nodes the page would spend all its time redrawing.
- **Plain lines above some size.** Arrows are not visible on a graph that large anyway, but they would then mean
  something at one size and not another, and removing them was not needed to meet the budget.
- **Cytoscape.js** (optional in the plan). It stays out (Decision 4): nothing here needs its analysis yet.
