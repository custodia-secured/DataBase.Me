// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * Changes the views make, and how they report failures. Every change goes through a command and
 * comes back to every view as `vault-changed` (ADR-0017), so nothing here updates a view directly.
 */
import {
  commands,
  unwrap,
  type Id,
  type NewObject_Deserialize,
  type Position,
  type Value,
} from "@databaseme/client";

type ErrorListener = (message: string) => void;
const errorListeners = new Set<ErrorListener>();

/** Calls `listener` with a message whenever a change fails. */
export function onActionError(listener: ErrorListener): () => void {
  errorListeners.add(listener);
  return () => errorListeners.delete(listener);
}

/** Reports a failed change to whoever shows errors. */
export function reportError(error: unknown): void {
  const message = error instanceof Error ? error.message : String(error);
  for (const listener of errorListeners) listener(message);
}

/** Runs a change, reporting its failure. Resolves to its result, or `undefined` if it failed. */
export async function attempt<T>(change: () => Promise<T>): Promise<T | undefined> {
  try {
    return await change();
  } catch (error: unknown) {
    reportError(error);
    return undefined;
  }
}

type Flusher = () => Promise<void>;
const flushers = new Set<Flusher>();

/** Registers an editor's flush, so undo and redo save typing first. */
export function registerFlusher(flush: Flusher): () => void {
  flushers.add(flush);
  return () => flushers.delete(flush);
}

/** Saves every editor's unsaved typing. */
export async function flushEditors(): Promise<void> {
  await Promise.all([...flushers].map((flush) => flush()));
}

export const actions = {
  async undo() {
    await flushEditors();
    return attempt(() => unwrap(commands.undo()));
  },
  async redo() {
    await flushEditors();
    return attempt(() => unwrap(commands.redo()));
  },
  create(object: NewObject_Deserialize) {
    return attempt(() => unwrap(commands.objectCreate(object)));
  },
  rename(id: Id, title: string) {
    return attempt(() => unwrap(commands.objectRename(id, title)));
  },
  setProp(id: Id, key: string, value: Value | null) {
    return attempt(() => unwrap(commands.objectSetProp(id, key, value)));
  },
  move(id: Id, parent: Id | null, position: Position) {
    return attempt(() => unwrap(commands.objectMove(id, parent, position)));
  },
  trash(id: Id) {
    return attempt(() => unwrap(commands.objectTrash(id)));
  },
};
