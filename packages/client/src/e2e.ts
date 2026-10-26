// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * The end-to-end test transport (ADR-0018). Only the e2e build imports this module; a check on the
 * production build proves it is absent there.
 *
 * It replaces Tauri's IPC with `fetch` calls to the e2e server, which runs the same Rust command
 * functions as the desktop shell on a real vault. The server returns the changes each command
 * committed, and they are delivered as `vault-changed` events through Tauri's event mocks, exactly
 * where the shell's events arrive.
 */
import { emit } from "@tauri-apps/api/event";
import { mockIPC } from "@tauri-apps/api/mocks";

interface Reply {
  ok?: unknown;
  error?: unknown;
  events: unknown[];
}

/** Routes every command to the e2e server for the session named in the page's URL. */
export function installE2eTransport(): void {
  const session = new URLSearchParams(window.location.search).get("session");
  if (session === null) throw new Error("the e2e build needs ?session=<name> in its URL");
  mockIPC(
    async (cmd, args) => {
      const response = await fetch("/ipc", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ session, cmd, args: args ?? {} }),
      });
      const reply = (await response.json()) as Reply;
      for (const committed of reply.events) await emit("vault-changed", committed);
      if ("error" in reply) {
        // Tauri rejects with the IpcError itself; the bindings turn it into `{ status: "error" }`.
        throw reply.error;
      }
      return reply.ok;
    },
    { shouldMockEvents: true },
  );
}
