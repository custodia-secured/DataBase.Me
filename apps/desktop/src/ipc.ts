// SPDX-License-Identifier: AGPL-3.0-or-later
import { isTauri } from "@tauri-apps/api/core";
import { commands, events, type Committed_Serialize, type IpcError } from "./bindings";

/**
 * The shell's typed commands and events, generated from `src-tauri/src/ipc.rs` (ADR-0014). Commands
 * that touch a vault resolve to `{ status, data | error }`; {@link unwrap} turns that into a value
 * or a thrown {@link CommandError}.
 */
export { commands, events };

/** A command failed. `kind` says what went wrong in a form the UI can act on. */
export class CommandError extends Error {
  readonly kind: IpcError["kind"];

  constructor(error: IpcError) {
    super(`${error.kind}: ${error.message}`);
    this.name = "CommandError";
    this.kind = error.kind;
  }
}

/** Returns a command's data, or throws its error as a {@link CommandError}. */
export async function unwrap<T>(
  result: Promise<{ status: "ok"; data: T } | { status: "error"; error: IpcError }>,
): Promise<T> {
  const settled = await result;
  if (settled.status === "error") throw new CommandError(settled.error);
  return settled.data;
}

/** Waits until `condition` holds, checking every 10 ms, or throws after `timeoutMs`. */
async function until(condition: () => boolean, what: string, timeoutMs = 5000): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  while (!condition()) {
    if (Date.now() > deadline) throw new Error(`timed out waiting for ${what}`);
    await new Promise((resolve) => setTimeout(resolve, 10));
  }
}

/**
 * The smoke test's launch checks (ADR-0008), run over real IPC: create a vault at `vaultPath`,
 * create an object, undo it, and receive a `vault-changed` event for each change. Throws on the
 * first failure.
 */
export async function runLaunchChecks(vaultPath: string): Promise<void> {
  const changes: Committed_Serialize[] = [];
  const unlisten = await events.vaultChanged.listen((event) => {
    changes.push(event.payload);
  });
  try {
    await unwrap(commands.vaultCreate(vaultPath));
    const id = await unwrap(commands.objectCreate({ title: "Launch check" }));
    const created = await unwrap(commands.objectChildren(null));
    if (created.length !== 1 || created[0]?.id !== id) {
      throw new Error(
        `expected only the new object at the top level, found ${String(created.length)}`,
      );
    }
    const undone = await unwrap(commands.undo());
    if (undone === null) throw new Error("undo found nothing to undo");
    const left = await unwrap(commands.objectChildren(null));
    if (left.length !== 0) throw new Error(`undo left ${String(left.length)} objects behind`);

    await until(() => changes.length >= 2, "two vault-changed events");
    const [create, undo] = changes;
    if (!create?.changed.includes(id)) {
      throw new Error("the create event does not name the object");
    }
    if (undo?.undo_of !== create.group_id) {
      throw new Error("the undo event does not name the create");
    }
  } finally {
    unlisten();
  }
}

let ready: Promise<void> | undefined;

/**
 * Tells the Rust shell that the UI has rendered. The shell logs the startup time. In smoke-test mode
 * the launch checks run first and the shell then exits, with status 0 only if they passed, so CI can
 * prove the app launched and its storage works over IPC.
 *
 * Only the first call does anything: React's StrictMode runs effects twice in development. Outside
 * the Tauri webview (unit tests, a plain browser on the Vite dev server) there is no shell to tell.
 */
export function notifyReady(): Promise<void> {
  ready ??= reportReady();
  return ready;
}

async function reportReady(): Promise<void> {
  if (!isTauri()) return;
  const info = await commands.appInfo();
  let error: string | null = null;
  if (info.smoke_vault !== null) {
    try {
      await runLaunchChecks(info.smoke_vault);
    } catch (failure: unknown) {
      error = failure instanceof Error ? failure.message : String(failure);
    }
  }
  await commands.appReady(error);
}
