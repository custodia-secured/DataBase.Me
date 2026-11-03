// SPDX-License-Identifier: AGPL-3.0-or-later
/** Words for an import plan (M6): counts, the paths typed, what a source becomes. */
import type { PlanEntry_Serialize } from "@databaseme/client";

/** "3 objects", "1 object". */
export function countOf(count: number, noun: string): string {
  return `${String(count)} ${noun}${count === 1 ? "" : "s"}`;
}

/** The lines of the paths box that are not blank, in order, each once. */
export function pathsOf(text: string): string[] {
  const seen = new Set<string>();
  const paths: string[] = [];
  for (const line of text.split(/\r?\n/)) {
    const path = line.trim();
    if (path !== "" && !seen.has(path)) {
      seen.add(path);
      paths.push(path);
    }
  }
  return paths;
}

/** What a plan entry becomes: "2 rows of type people, under a Dataset" or "a Document". */
export function becomes(entry: PlanEntry_Serialize): string {
  const folder = entry.extract.folder;
  if (folder !== null) {
    const files = folder.markdown_files + folder.other_files;
    return `${countOf(folder.markdown_files, "page")}${
      folder.other_files > 0 ? ` and ${countOf(folder.other_files, "file")}` : ""
    } from ${countOf(files, "file")}`;
  }
  const first = entry.extract.objects.at(0);
  if (first === undefined) return "nothing";
  const children = first.children;
  const child = children.at(0);
  if (child !== undefined) {
    return `${countOf(children.length, "row")} of type ${child.type_name}, under a ${first.type_name}`;
  }
  return entry.objects === 1 ? `a ${first.type_name}` : countOf(entry.objects, first.type_name);
}
