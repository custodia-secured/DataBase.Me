// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * The diagnostic report (M7; brief §2.2, build plan §2.11): plain text a person pastes into an
 * issue. Versions, the OS, the webview, the mode and theme, whether the files folder is on,
 * counts of objects and blobs, and the last failures' kinds and times. Never a path, a title or
 * any other content: the vault's location, the files folder's folder and error messages stay
 * out on purpose.
 */
import {
  commands,
  recentErrors,
  SETTINGS_KEY,
  unwrap,
  type RecentError,
  type Settings_Serialize,
} from "@databaseme/client";
import type { QueryClient } from "@tanstack/react-query";

export interface ReportInput {
  /** The app's version and the format version it writes; `null` when not known. */
  about: { version: string; formatVersion: number } | null;
  os: string;
  arch: string;
  webview: string;
  counts: { objects: number; blobs: number; schema_version: number } | null;
  settings: Pick<Settings_Serialize, "mode" | "theme" | "update_checks"> | null;
  /** Whether the files folder is on; `null` when there is no vault. */
  filesFolder: boolean | null;
  errors: readonly RecentError[];
  now: Date;
}

/** The report as text. Pure, so a test can read every line. */
export function renderReport(input: ReportInput): string {
  const lines = [
    "DataBase.Me diagnostic report",
    `made: ${input.now.toISOString()}`,
    input.about
      ? `app: ${input.about.version}, vault format ${String(input.about.formatVersion)}`
      : "app: version unknown",
    `system: ${input.os} ${input.arch}`,
    `webview: ${input.webview}`,
    input.settings
      ? `settings: mode ${input.settings.mode}, theme ${input.settings.theme}, update checks ${
          input.settings.update_checks ? "on" : "off"
        }`
      : "settings: not read",
    input.counts
      ? `vault: ${String(input.counts.objects)} objects, ${String(input.counts.blobs)} blobs, schema ${String(input.counts.schema_version)}`
      : "vault: none open",
    `files folder: ${input.filesFolder === null ? "no vault" : input.filesFolder ? "on" : "off"}`,
    input.errors.length === 0
      ? "recent errors: none"
      : `recent errors (newest first): ${input.errors
          .map((error) => `${new Date(error.at).toISOString()} ${error.kind}`)
          .join("; ")}`,
  ];
  return `${lines.join("\n")}\n`;
}

/** Gathers the report's facts from the shell and the caches, and renders it. */
export async function diagnosticReport(
  client: QueryClient,
  about: ReportInput["about"],
): Promise<string> {
  const machine = await unwrap(commands.appDiagnostics());
  const settings =
    client.getQueryData<Settings_Serialize>(SETTINGS_KEY) ?? (await unwrap(commands.settingsGet()));
  const mirror = await unwrap(commands.mirrorStatus()).catch(() => null);
  return renderReport({
    about,
    os: machine.os,
    arch: machine.arch,
    webview: machine.webview,
    counts: machine.counts,
    settings,
    filesFolder: mirror === null ? null : mirror.enabled,
    errors: recentErrors(),
    now: new Date(),
  });
}
