// SPDX-License-Identifier: AGPL-3.0-or-later
/** The files folder's status line (ADR-0021). */
import type { MirrorStatus } from "@databaseme/client";

/** On or off, and what is waiting. */
export function statusLine(status: MirrorStatus): string {
  if (!status.enabled) return "Off. Your pages stay in the vault only.";
  const parts = [`${String(status.synced)} synced`];
  if (status.dirty > 0) parts.push(`${String(status.dirty)} waiting to be written`);
  if (status.pending > 0) parts.push(`${String(status.pending)} blocked by another program`);
  if (status.held > 0) parts.push(`${String(status.held)} held`);
  return `On: ${parts.join(", ")}.`;
}
