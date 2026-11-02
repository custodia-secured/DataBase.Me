// SPDX-License-Identifier: AGPL-3.0-or-later
/** A search snippet's matching words, which the store wraps in `[` and `]`, as `<mark>`s. */
import type { ReactNode } from "react";

export function Snippet({ text }: { text: string }) {
  const parts: ReactNode[] = [];
  let rest = text;
  let n = 0;
  while (rest !== "") {
    const open = rest.indexOf("[");
    const close = open === -1 ? -1 : rest.indexOf("]", open);
    if (open === -1 || close === -1) {
      parts.push(rest);
      break;
    }
    if (open > 0) parts.push(rest.slice(0, open));
    parts.push(<mark key={n++}>{rest.slice(open + 1, close)}</mark>);
    rest = rest.slice(close + 1);
  }
  return <span className="dbme-snippet">{parts}</span>;
}
