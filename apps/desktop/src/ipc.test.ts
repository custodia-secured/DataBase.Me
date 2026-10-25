// SPDX-License-Identifier: AGPL-3.0-or-later
import { emit } from "@tauri-apps/api/event";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { afterEach, describe, expect, it } from "vitest";
import type { Committed_Serialize, IpcError, ObjectRow_Serialize } from "./bindings";
import { CommandError, runLaunchChecks } from "./ipc";

afterEach(clearMocks);

/** A stand-in for the shell: one vault holding top-level objects, with a one-step undo. */
function mockShell(options: { failCreate?: IpcError; silent?: boolean } = {}) {
  const objects: ObjectRow_Serialize[] = [];
  let last: Committed_Serialize | null = null;
  let seq = 0;
  const changed = async (committed: Committed_Serialize) => {
    last = committed;
    if (!options.silent) await emit("vault-changed", committed);
  };
  const object = (id: string, title: string): ObjectRow_Serialize => ({
    id,
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
  });
  mockIPC(
    async (command, args) => {
      switch (command) {
        case "vault_create":
          return { id: "VAULT", path: "Smoke.dbme", can_undo: false, can_redo: false };
        case "object_create": {
          // eslint-disable-next-line @typescript-eslint/only-throw-error -- the shell rejects with a plain IpcError object, as here
          if (options.failCreate) throw options.failCreate;
          const { title } = (args as { object: { title: string } }).object;
          objects.push(object("OBJECT", title));
          seq += 1;
          await changed({
            group_id: "GROUP",
            undo_of: null,
            first_seq: seq,
            last_seq: seq,
            changed: ["OBJECT"],
          });
          return "OBJECT";
        }
        case "object_children":
          return objects;
        case "undo": {
          if (!last) return null;
          objects.pop();
          seq += 1;
          const committed = {
            ...last,
            group_id: "UNDO",
            undo_of: last.group_id,
            first_seq: seq,
            last_seq: seq,
          };
          await changed(committed);
          return committed;
        }
        default:
          throw new Error(`unexpected command ${command}`);
      }
    },
    { shouldMockEvents: true },
  );
}

describe("launch checks", () => {
  it("pass against a shell that creates, undoes and reports both changes", async () => {
    mockShell();
    await expect(runLaunchChecks("Smoke.dbme")).resolves.toBeUndefined();
  });

  it("throw the shell's error kind when a command fails", async () => {
    mockShell({ failCreate: { kind: "rule", message: "titles cannot be empty" } });
    const failure = runLaunchChecks("Smoke.dbme");
    await expect(failure).rejects.toBeInstanceOf(CommandError);
    await expect(failure).rejects.toMatchObject({ kind: "rule" });
  });

  it("fail when no vault-changed events arrive", async () => {
    mockShell({ silent: true });
    await expect(runLaunchChecks("Smoke.dbme")).rejects.toThrow(/two vault-changed events/);
  }, 10_000);
});
