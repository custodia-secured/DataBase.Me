// SPDX-License-Identifier: AGPL-3.0-or-later
import { QueryClient } from "@tanstack/react-query";
import { emit } from "@tauri-apps/api/event";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { Committed_Serialize } from "./bindings";
import { installE2eTransport } from "./e2e";
import { CommandError, commands, unwrap } from "./ipc";
import { onVaultChanged, startLiveRefresh } from "./live";
import { encodeBase64, keys, VAULT } from "./queries";
import { encodeUlid, newId } from "./ulid";

afterEach(() => {
  clearMocks();
  vi.unstubAllGlobals();
});

const committed: Committed_Serialize = {
  group_id: "GROUP",
  undo_of: null,
  first_seq: 1,
  last_seq: 1,
  changed: ["OBJECT"],
};

describe("unwrap", () => {
  it("returns the data of a command that succeeded", async () => {
    await expect(unwrap(Promise.resolve({ status: "ok", data: 7 }))).resolves.toBe(7);
  });

  it("throws a CommandError carrying the shell's error kind", async () => {
    const failure = unwrap(
      Promise.resolve({
        status: "error",
        error: { kind: "not_found", message: "no such object" },
      } as const),
    );
    await expect(failure).rejects.toBeInstanceOf(CommandError);
    await expect(failure).rejects.toMatchObject({ kind: "not_found" });
  });
});

describe("ULIDs", () => {
  it("are 26 Crockford base32 characters", () => {
    expect(newId()).toMatch(/^[0-9A-HJKMNP-TV-Z]{26}$/);
    expect(encodeUlid(0n)).toBe("0".repeat(26));
    expect(encodeUlid((1n << 128n) - 1n)).toBe("7" + "Z".repeat(25));
  });

  it("start with the time, so later IDs sort later", () => {
    const early = newId(1_000);
    const late = newId(2_000_000_000_000);
    expect(early < late).toBe(true);
    expect(late.slice(0, 10)).toBe(encodeUlid(2_000_000_000_000n << 80n).slice(0, 10));
  });

  it("count up within one millisecond and never repeat", () => {
    const now = 3_000_000_000_000;
    const ids = Array.from({ length: 1000 }, () => newId(now));
    expect(new Set(ids).size).toBe(ids.length);
    expect([...ids].sort()).toEqual(ids);
  });

  it("never go backwards when the clock does", () => {
    const ahead = newId(4_000_000_000_000);
    const behind = newId(1);
    expect(behind > ahead).toBe(true);
  });
});

describe("live refresh", () => {
  it("refreshes every vault query and tells listeners after a change", async () => {
    mockIPC(() => null, { shouldMockEvents: true });
    const client = new QueryClient();
    const invalidate = vi.spyOn(client, "invalidateQueries");
    const seen: Committed_Serialize[] = [];
    const stopListening = onVaultChanged((change) => seen.push(change));
    const stop = await startLiveRefresh(client);
    await emit("vault-changed", committed);
    expect(seen).toEqual([committed]);
    expect(invalidate).toHaveBeenCalledWith({ queryKey: [VAULT] });
    stop();
    stopListening();
    await emit("vault-changed", committed);
    expect(seen).toHaveLength(1);
  });

  it("keys every vault read under the vault prefix", () => {
    expect(keys.object("A")[0]).toBe(VAULT);
    expect(keys.search("x", 5)[0]).toBe(VAULT);
  });
});

describe("base64", () => {
  it("encodes bytes as atob reads them back", () => {
    const bytes = new Uint8Array(70_000).map((_, i) => i % 251);
    const back = Uint8Array.from(atob(encodeBase64(bytes)), (c) => c.charCodeAt(0));
    expect(back).toEqual(bytes);
  });
});

describe("e2e transport", () => {
  it("posts each command to the server and delivers the changes it reports", async () => {
    window.history.replaceState(null, "", "/?session=unit-1");
    const fetch = vi.fn(
      (_url: string, init: RequestInit) =>
        new Response(
          JSON.stringify(
            (JSON.parse(init.body as string) as { cmd: string }).cmd === "object_rename"
              ? { error: { kind: "rule", message: "titles cannot be empty" }, events: [] }
              : { ok: "OBJECT", events: [committed] },
          ),
        ),
    );
    vi.stubGlobal("fetch", fetch);
    installE2eTransport();
    const client = new QueryClient();
    const seen: Committed_Serialize[] = [];
    const stopListening = onVaultChanged((change) => seen.push(change));
    const stop = await startLiveRefresh(client);

    await expect(unwrap(commands.objectCreate({ title: "Hello" }))).resolves.toBe("OBJECT");
    expect(seen).toEqual([committed]);
    const [url, init] = fetch.mock.calls[0] ?? [];
    expect(url).toBe("/ipc");
    expect(JSON.parse(init?.body as string)).toEqual({
      session: "unit-1",
      cmd: "object_create",
      args: { object: { title: "Hello" } },
    });

    const failure = unwrap(commands.objectRename("OBJECT", ""));
    await expect(failure).rejects.toMatchObject({ kind: "rule" });
    stop();
    stopListening();
  });

  it("refuses to start without a session", () => {
    window.history.replaceState(null, "", "/");
    expect(installE2eTransport).toThrow(/session/);
  });
});
