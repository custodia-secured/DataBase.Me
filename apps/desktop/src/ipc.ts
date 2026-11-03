// SPDX-License-Identifier: AGPL-3.0-or-later
import { isTauri } from "@tauri-apps/api/core";
import { commands, events, unwrap, type Committed_Serialize } from "@databaseme/client";

/** Waits until `condition` holds, checking every 10 ms, or throws after `timeoutMs`. */
async function until(condition: () => boolean, what: string, timeoutMs = 5000): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  while (!condition()) {
    if (Date.now() > deadline) throw new Error(`timed out waiting for ${what}`);
    await new Promise((resolve) => setTimeout(resolve, 10));
  }
}

/**
 * The mind map's commands, each of which the capability file has to grant (ADR-0010): read a tree
 * with a relation in it, then send the relation to the Trash.
 */
async function checkMapCommands(): Promise<void> {
  const root = await unwrap(commands.objectCreate({ title: "Map root" }));
  const leaf = await unwrap(commands.objectCreate({ title: "Map leaf", parent_id: root }));
  const relation = await unwrap(commands.relationCreate(root, leaf));
  const tree = await unwrap(commands.objectTree(root, 10));
  if (tree.nodes.length !== 2 || tree.relations[0]?.id !== relation) {
    throw new Error(
      `the map's read found ${String(tree.nodes.length)} nodes and ${String(tree.relations.length)} relations`,
    );
  }
  await unwrap(commands.relationTrash(relation));
  const after = await unwrap(commands.objectTree(root, 10));
  if (after.relations.length !== 0) throw new Error("the trashed relation is still on the map");
  // Search (ADR-0020): the index has caught up with the objects made just now.
  const hits = await unwrap(commands.search("map lea", 5));
  if (hits[0]?.summary.object.id !== leaf) {
    throw new Error(`search for the new leaf found ${String(hits.length)} other objects`);
  }
}

/**
 * The smoke test's launch checks (ADR-0008), run over real IPC: create a vault at `vaultPath`,
 * create an object, undo it, receive a `vault-changed` event for each change, and use the mind
 * map's commands. Throws on the first failure.
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

    await checkMapCommands();
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

/** How long the clipboard probe waits for the webview before it gives up. */
const CLIPBOARD_PROBE_MS = 2000;

/**
 * What the webview's clipboard does under the app's CSP, for the record (M7 gate 1c): writes a
 * line, which is all the diagnostic report needs. The outcome is a note, never a failure: the
 * report falls back to a text box the person copies from when the write is refused. The write
 * is given two seconds: WebView2 answered a clipboard call with a prompt nobody is there to
 * answer, and the launch waited on it (found at gate 1c). Nothing is read back: reading the
 * clipboard is a permission question on every webview, and the app never needs it.
 */
async function clipboardProbe(): Promise<string> {
  const text = `dbme clipboard probe ${String(Date.now())}`;
  const timeout = new Promise<"timeout">((resolve) => {
    setTimeout(() => {
      resolve("timeout");
    }, CLIPBOARD_PROBE_MS);
  });
  try {
    const outcome = await Promise.race([navigator.clipboard.writeText(text), timeout]);
    return outcome === "timeout"
      ? `clipboard: no answer to the write within ${String(CLIPBOARD_PROBE_MS)} ms`
      : "clipboard: write ok";
  } catch (failure: unknown) {
    return `clipboard: write failed (${failure instanceof Error ? failure.message : String(failure)})`;
  }
}

async function reportReady(): Promise<void> {
  if (!isTauri()) return;
  const info = await commands.appInfo();
  let error: string | null = null;
  const notes: string[] = [];
  if (info.smoke_vault !== null) {
    try {
      await runLaunchChecks(info.smoke_vault);
    } catch (failure: unknown) {
      error = failure instanceof Error ? failure.message : String(failure);
    }
    notes.push(await clipboardProbe());
  }
  await commands.appReady(error, notes);
}
