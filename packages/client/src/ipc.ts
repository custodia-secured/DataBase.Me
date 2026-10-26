// SPDX-License-Identifier: AGPL-3.0-or-later
import { commands, events, type IpcError } from "./bindings";

/**
 * The shell's typed commands and events, generated from Rust (ADR-0014). Commands that touch a
 * vault resolve to `{ status, data | error }`; {@link unwrap} turns that into a value or a thrown
 * {@link CommandError}.
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
