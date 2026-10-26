// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * Keyboard movement in a grid of cells (WAI-ARIA grid pattern), shared by the Table and the
 * Gallery. Row -1 is a header row when the grid has one.
 */

export interface Cell {
  row: number;
  col: number;
}

export interface GridSize {
  rows: number;
  cols: number;
  /** Rows a Page Up or Page Down moves. */
  page: number;
  /** Whether row -1 is a header row the focus can reach. */
  header: boolean;
}

/** Where a key moves the focus from `at`, or null if the key does not move it. */
export function moveCell(
  key: string,
  ctrl: boolean,
  at: Cell,
  { rows, cols, page, header }: GridSize,
): Cell | null {
  const top = header ? -1 : 0;
  const lastRow = Math.max(top, rows - 1);
  const clampRow = (row: number) => Math.min(lastRow, Math.max(top, row));
  const clampCol = (col: number) => Math.min(cols - 1, Math.max(0, col));
  switch (key) {
    case "ArrowUp":
      return { row: clampRow(at.row - 1), col: at.col };
    case "ArrowDown":
      return { row: clampRow(at.row + 1), col: at.col };
    case "ArrowLeft":
      return { row: at.row, col: clampCol(at.col - 1) };
    case "ArrowRight":
      return { row: at.row, col: clampCol(at.col + 1) };
    case "Home":
      return ctrl ? { row: Math.max(0, top), col: 0 } : { row: at.row, col: 0 };
    case "End":
      return ctrl ? { row: lastRow, col: cols - 1 } : { row: at.row, col: cols - 1 };
    case "PageUp":
      return { row: clampRow(at.row - page), col: at.col };
    case "PageDown":
      return { row: clampRow(at.row + page), col: at.col };
    default:
      return null;
  }
}

/** Whether a key press types a character, which starts editing a text cell. */
export function isTyping(event: {
  key: string;
  ctrlKey: boolean;
  metaKey: boolean;
  altKey: boolean;
}): boolean {
  return event.key.length === 1 && !event.ctrlKey && !event.metaKey && !event.altKey;
}
