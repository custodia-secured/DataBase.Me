// SPDX-License-Identifier: AGPL-3.0-or-later
/** The editor's node services on top of the vault queries, so mentions and images stay live. */
import { QueryObserver, type QueryClient, type QueryObserverResult } from "@tanstack/react-query";
import {
  blobUrl,
  commands,
  keys,
  unwrap,
  type Id,
  type ObjectRow_Serialize,
} from "@databaseme/client";
import type { NodeServices } from "./nodes";

export function nodeServices(client: QueryClient, self: Id): NodeServices {
  return {
    observeTitle(id, onTitle) {
      const observer = new QueryObserver<ObjectRow_Serialize>(client, {
        queryKey: keys.object(id),
        queryFn: () => unwrap(commands.objectGet(id)),
        retry: false,
      });
      const report = (result: QueryObserverResult<ObjectRow_Serialize>) => {
        if (result.isError) onTitle(null, true);
        else if (result.data) onTitle(result.data.title, result.data.deleted_at !== null);
      };
      const stop = observer.subscribe(report);
      // A subscriber hears only of changes. A title already read, by an earlier editor or another
      // view, is not read again, so it is reported here.
      report(observer.getCurrentResult());
      return stop;
    },
    blobUrl: (hash) => blobUrl(hash, "image/*"),
    async search(text) {
      const found = await unwrap(commands.objectSearch(text, 8));
      return found
        .filter((summary) => summary.object.id !== self)
        .map((summary) => ({ id: summary.object.id, title: summary.object.title }));
    },
  };
}
