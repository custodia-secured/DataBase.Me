// SPDX-License-Identifier: AGPL-3.0-or-later
/** Seeds for the e2e tests, in the e2e server's format (crates/e2e-server/src/seed.rs). */
import { crc32, deflateSync } from "node:zlib";

/** A solid-colour RGB PNG, base64, to use as a cover. */
export function solidPng(width: number, height: number, rgb: [number, number, number]): string {
  const chunk = (type: string, data: Buffer) => {
    const length = Buffer.alloc(4);
    length.writeUInt32BE(data.length);
    const body = Buffer.concat([Buffer.from(type, "ascii"), data]);
    const crc = Buffer.alloc(4);
    crc.writeUInt32BE(crc32(body));
    return Buffer.concat([length, body, crc]);
  };
  const header = Buffer.alloc(13);
  header.writeUInt32BE(width, 0);
  header.writeUInt32BE(height, 4);
  header[8] = 8; // bits per sample
  header[9] = 2; // RGB
  const row = Buffer.alloc(1 + width * 3);
  for (let x = 0; x < width; x++) row.set(rgb, 1 + x * 3);
  const pixels = Buffer.concat(Array.from({ length: height }, () => row));
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk("IHDR", header),
    chunk("IDAT", deflateSync(pixels)),
    chunk("IEND", Buffer.alloc(0)),
  ]).toString("base64");
}

/**
 * A small workspace: Home with text that mentions Alpha, three typed Milestones below it (two
 * with covers, one with a child), and an Inbox with two ideas.
 */
export function workspace(): unknown {
  return {
    kind: "tree",
    types: [
      {
        name: "Milestone",
        fields: [
          {
            key: "status",
            label: "Status",
            kind: "select",
            options: {
              options: [
                { id: "todo", label: "To do" },
                { id: "doing", label: "Doing" },
                { id: "done", label: "Done" },
              ],
            },
          },
          { key: "due", label: "Due", kind: "date" },
          { key: "estimate", label: "Estimate", kind: "number" },
          { key: "shipped", label: "Shipped", kind: "boolean" },
        ],
      },
    ],
    objects: [
      {
        title: "Home",
        blocks: [
          { kind: "heading", content: { level: 1, inline: [{ text: "Welcome" }] } },
          {
            kind: "paragraph",
            content: {
              inline: [{ text: "Start with " }, { mention: "title:Alpha" }, { text: "." }],
            },
          },
          { kind: "bullet", content: { inline: [{ text: "First point" }] } },
          { kind: "todo", content: { checked: false, inline: [{ text: "Write the plan" }] } },
        ],
        children: [
          {
            title: "Alpha",
            types: ["Milestone"],
            props: { status: "doing", due: "2026-11-02", estimate: 3, shipped: false },
            cover_png: solidPng(64, 48, [37, 99, 235]),
          },
          {
            title: "Beta",
            types: ["Milestone"],
            props: { status: "todo", estimate: 8, shipped: false },
            cover_png: solidPng(64, 48, [22, 163, 74]),
            children: [{ title: "Beta notes" }],
          },
          {
            title: "Gamma",
            types: ["Milestone"],
            props: { status: "done", estimate: 1, shipped: true },
          },
        ],
      },
      { title: "Inbox", children: [{ title: "Idea one" }, { title: "Idea two" }] },
    ],
  };
}

/** A page whose text is `count` paragraphs, for typing latency. */
export function longDocument(count: number): unknown {
  return {
    kind: "tree",
    objects: [
      {
        title: "Long read",
        blocks: Array.from({ length: count }, (_, n) => ({
          kind: "paragraph",
          content: {
            inline: [
              { text: `Paragraph ${String(n + 1)}: ` },
              { text: "some bold words", bold: true },
              { text: " and the rest of a sentence long enough to wrap on a phone screen." },
            ],
          },
        })),
      },
    ],
  };
}

/** `count` typed objects under one page, for the 100k-row table budget. */
export function bulk(count: number): unknown {
  return { kind: "bulk", count, type_name: "Load item", parent_title: "Big table" };
}
