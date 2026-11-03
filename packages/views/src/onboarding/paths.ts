// SPDX-License-Identifier: AGPL-3.0-or-later
/** Paths onboarding builds (M7). */

/** The vault's folder inside `folder`, joined the way the default path is. */
export function vaultIn(folder: string, defaultVault: string | null): string {
  const separator = (defaultVault ?? "").includes("\\") ? "\\" : "/";
  const trimmed = folder.replace(/[\\/]+$/, "");
  return `${trimmed}${separator}Main.dbme`;
}
