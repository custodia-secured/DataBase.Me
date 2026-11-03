// SPDX-License-Identifier: AGPL-3.0-or-later
/** Words for where an object came from (M6): the importers' names and the provenance line. */
import type { ProvenanceRow_Serialize } from "@databaseme/client";

const IMPORTERS: Readonly<Record<string, string>> = {
  markdown: "Markdown",
  csv: "CSV",
  xlsx: "Excel",
  pdf: "PDF",
  image: "picture",
  file: "file",
};

/** The importer's name for people, from its id: a noun that fits "the CSV importer". */
export function importerLabel(id: string): string {
  return IMPORTERS[id] ?? id;
}

/** The last part of a path or URI, for a line. */
export function sourceName(uri: string): string {
  const [path] = uri.split("#");
  const trimmed = (path ?? uri).replace(/[\\/]+$/, "");
  const cut = Math.max(trimmed.lastIndexOf("/"), trimmed.lastIndexOf("\\"));
  const name = cut < 0 ? trimmed : trimmed.slice(cut + 1);
  const fragment = uri.includes("#") ? uri.slice(uri.indexOf("#") + 1) : "";
  if (fragment.startsWith("sheet=")) return `${name}, sheet ${fragment.slice(6)}`;
  if (fragment.startsWith("page=")) return `${name}, page ${fragment.slice(5)}`;
  return name;
}

/** "Imported from people.csv on 3 Nov 2026 by the CSV importer". */
export function originLine(row: ProvenanceRow_Serialize, locale?: string): string {
  const date = new Date(row.created_at).toLocaleDateString(locale, {
    year: "numeric",
    month: "short",
    day: "numeric",
  });
  const kind = row.source_kind;
  const importer = kind.startsWith("import:") ? kind.slice("import:".length) : null;
  const from = row.source_uri !== null ? ` from ${sourceName(row.source_uri)}` : "";
  if (importer !== null) {
    return `Imported${from} on ${date} by the ${importerLabel(importer)} importer`;
  }
  return `${kind}${from}, ${date}`;
}

/** "1.2 KB", "340 bytes". */
export function sizeOf(bytes: number): string {
  if (bytes < 1024) return `${String(bytes)} bytes`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}
