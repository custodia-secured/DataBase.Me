// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * Live consistency across views (ADR-0017). After every committed change the shell sends
 * `vault-changed`. Every open view then reads again what it shows: all active vault queries are
 * refetched, and TanStack Query's structural sharing keeps the results that did not change
 * identical, so only the parts that changed re-render. Inactive queries are just marked stale.
 *
 * Listeners can also react to a change directly, for example the editor, which must not reload
 * its own echo while the user types.
 */
import type { QueryClient } from "@tanstack/react-query";
import type { Committed_Serialize } from "./bindings";
import { events } from "./ipc";
import { VAULT } from "./queries";

type Listener = (committed: Committed_Serialize) => void;
const listeners = new Set<Listener>();

/** Calls `listener` after every change, until the returned function is called. */
export function onVaultChanged(listener: Listener): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** Refreshes `client`'s vault queries after every change. Returns a function that stops it. */
export async function startLiveRefresh(client: QueryClient): Promise<() => void> {
  const unlisten = await events.vaultChanged.listen((event) => {
    const committed = event.payload;
    for (const listener of listeners) listener(committed);
    void client.invalidateQueries({ queryKey: [VAULT] });
  });
  return unlisten;
}
