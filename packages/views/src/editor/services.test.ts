// SPDX-License-Identifier: AGPL-3.0-or-later
import { keys, type ObjectRow_Serialize } from "@databaseme/client";
import { QueryClient } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import { nodeServices } from "./services";

const ID = "01JB3R7Q8Y5K3M2N4P6Q8R0S2T";

function row(title: string): ObjectRow_Serialize {
  return {
    id: ID,
    title,
    summary: null,
    parent_id: null,
    sort_key: "a0",
    props_json: {},
    created_at: 0,
    updated_at: 0,
    captured_at: null,
    event_start: null,
    event_end: null,
    geo_lat: null,
    geo_lon: null,
    deleted_at: null,
    sensitivity: "normal",
  };
}

describe("observeTitle", () => {
  it("reports a title already read, then each change", () => {
    // As in the app: reads are never stale on a timer, so a cached title is not read again.
    const client = new QueryClient({ defaultOptions: { queries: { staleTime: Infinity } } });
    client.setQueryData(keys.object(ID), row("Alpha"));
    const onTitle = vi.fn();
    const stop = nodeServices(client, "self").observeTitle(ID, onTitle);
    expect(onTitle).toHaveBeenLastCalledWith("Alpha", false);

    client.setQueryData(keys.object(ID), row("Alpha prime"));
    expect(onTitle).toHaveBeenLastCalledWith("Alpha prime", false);
    stop();
    client.setQueryData(keys.object(ID), row("Gone"));
    expect(onTitle).toHaveBeenLastCalledWith("Alpha prime", false);
  });
});
