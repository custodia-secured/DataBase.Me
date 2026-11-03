# Keyboard and accessibility

Everything in the app can be done from the keyboard, and every screen is checked against WCAG 2.2
level AA (with axe, on every change, on a desktop window and a phone-sized one, in light and dark).
The interface works down to 360 pixels wide; below 768 pixels the pages list is a drawer.

## Everywhere

| Keys                           | Does                                                                                                        |
| ------------------------------ | ----------------------------------------------------------------------------------------------------------- |
| Ctrl+K (⌘K)                    | The palette: search, or a command after `>`; Up and Down choose, Enter opens, Ctrl+Enter shows every result |
| Ctrl+Z, Ctrl+Shift+Z or Ctrl+Y | Undo and redo the last change (outside a text field)                                                        |
| Tab, Shift+Tab                 | Move between the regions: header, pages list, view tabs, pane                                               |
| Escape                         | Close a dialog, a menu or the palette                                                                       |

The palette, the Move dialog, Settings, the Files folder and Import are dialogs: focus stays
inside until they close, and returns to where it was.

## The pages list

One tab stop; the arrows move inside it.

| Keys             | Does                                         |
| ---------------- | -------------------------------------------- |
| Up, Down         | The previous or next page                    |
| Right            | Open a folded page, or go to its first child |
| Left             | Fold a page, or go to its parent             |
| Home, End        | The first or last page                       |
| Enter, Space     | Open the page                                |
| Alt+Up, Alt+Down | Move the page up or down among its siblings  |

The **New page** button and the **⋯** actions button (New page inside, Move up, Move down, Move
to…, Move to Trash) sit above the list. In the Move dialog, type to find the new parent, Tab to the choices, Enter to choose.

## The Document view

The title is a text field; Enter moves into the text. The formatting toolbar is one tab stop
before the text, with Left and Right between its buttons and Enter to apply one. In the text:
Ctrl+B, Ctrl+I and Ctrl+U for bold, italic and underline; `[[` opens the list of pages to link,
Up and Down choose, Enter or Tab inserts, Escape closes. Each field under the types line is a
button; Enter opens it for editing, Enter keeps the value and Escape drops it.

## The Outline view

Each line is a text field.

| Keys                       | Does                                    |
| -------------------------- | --------------------------------------- |
| Enter                      | A new line after this one (a new page)  |
| Ctrl+Enter                 | Open this line's page                   |
| Tab, Shift+Tab             | Indent under the line above, or outdent |
| Alt+Up, Alt+Down           | Move the line among its siblings        |
| Ctrl+Down, Ctrl+Up         | Unfold or fold this line's children     |
| Up, Down                   | The line above or below                 |
| Backspace on an empty line | Remove the line (to the Trash)          |

## The Mind map

One tab stop; the arrows move between nodes. Left goes to the parent, Right to the first child
(or unfolds), Up and Down among siblings; Alt+Up and Alt+Down reorder. Tab adds a child and Enter
a sibling, as mind-map tools do; F2 renames; Delete sends a node to the Trash; Space folds and
unfolds; Ctrl+Enter opens the node's page. Shift+Tab always leaves the map. A relation drawn
between nodes is chosen by clicking it and removed with Delete; it is not on the keyboard's path
in 0.1.

## The Table view

A grid with one tab stop. Arrows move between cells, Home and End to the ends of a row; Enter or
F2 edits the cell, Enter keeps the value and moves down, Tab keeps it and moves right, Escape
drops it; Space ticks a box. The column headers sort.

## Screen readers

Regions, dialogs, trees, grids, tabs and menus are named and carry the roles assistive technology
expects (`tree` and `treeitem` with the level and the expanded state; `grid` with `gridcell`;
`tablist` and `tab`; `dialog` with its title; `status` for what changes on its own, such as the
files folder's line). The active row of the pages list and the active cell of the grid are
announced through `aria-activedescendant`. Icons that stand alone carry a label; decorative ones
are hidden. The mind map's nodes are a tree to a screen reader, with the same keys as above.

## Motion and contrast

Both themes meet the AA contrast rules for text, controls and the focus ring, which is visible on
every control. The app follows the system's reduced-motion setting for its animations.

## Reporting a problem

An accessibility problem is a bug: [reporting a problem](reporting-a-problem.md) says how to send
one, with the diagnostic report.
