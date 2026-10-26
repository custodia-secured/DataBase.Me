// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * The editor's own nodes: quote, image, mention and the placeholder for unknown blocks. Their node
 * views are plain DOM and take their data from {@link NodeServices}, so they stay live when a title
 * changes in another view.
 */
import { Node, textblockTypeInputRule, type AnyExtension, type Extensions } from "@tiptap/core";
import { TaskItem, TaskList } from "@tiptap/extension-list";
import { Placeholder } from "@tiptap/extensions";
import { NodeSelection } from "@tiptap/pm/state";
import { StarterKit } from "@tiptap/starter-kit";
import { BlockIds } from "./ids";
import { mentionSuggestion, type MentionSearch } from "./suggest";

/** The event a mention sends, from the editor's DOM, to open its object. */
export const OPEN_EVENT = "dbme-open";

function sendOpen(from: HTMLElement, id: string): void {
  from.dispatchEvent(new CustomEvent(OPEN_EVENT, { bubbles: true, detail: { id } }));
}

/** What the node views need from the app. */
export interface NodeServices {
  /** Calls `onTitle` with the title of object `id` now and whenever it changes. */
  observeTitle: (
    id: string,
    onTitle: (title: string | null, trashed: boolean) => void,
  ) => () => void;
  /** A URL for blob `hash`, to show an image. */
  blobUrl: (hash: string) => Promise<string>;
  /** Finds objects to mention. */
  search: MentionSearch;
}

/** A quotation: one block of text, not a wrapper of paragraphs. */
export const Quote = Node.create({
  name: "quote",
  group: "block",
  content: "inline*",
  defining: true,

  parseHTML() {
    return [{ tag: "blockquote" }];
  },

  renderHTML({ HTMLAttributes }) {
    return ["blockquote", HTMLAttributes, 0];
  },

  addInputRules() {
    return [textblockTypeInputRule({ find: /^\s*>\s$/, type: this.type })];
  },

  addKeyboardShortcuts() {
    return { "Mod-Shift-b": () => this.editor.commands.toggleNode(this.name, "paragraph") };
  },
});

/** An image from the blob store. It has no parse rule: images are added through the app. */
export const ImageBlock = Node.create<{ services: NodeServices | null }>({
  name: "image",
  group: "block",
  atom: true,
  draggable: true,

  addOptions() {
    return { services: null };
  },

  addAttributes() {
    return {
      blob: { default: null, rendered: false },
      alt: { default: null, rendered: false },
    };
  },

  renderHTML({ node }) {
    return [
      "figure",
      { class: "dbme-image" },
      ["img", { alt: (node.attrs.alt as string | null) ?? "" }],
    ];
  },

  addNodeView() {
    const services = this.options.services;
    return ({ node }) => {
      const dom = document.createElement("figure");
      dom.className = "dbme-image";
      const img = document.createElement("img");
      img.alt = (node.attrs.alt as string | null) ?? "";
      dom.append(img);
      const blob = node.attrs.blob as string | null;
      if (services && blob) {
        void services.blobUrl(blob).then((url) => {
          img.src = url;
        });
      }
      return { dom };
    };
  },
});

/** A block of a kind this version does not know, kept whole with the blocks below it. */
export const UnknownBlock = Node.create({
  name: "unknownBlock",
  group: "block",
  atom: true,
  selectable: true,
  draggable: true,

  addAttributes() {
    return {
      kind: { default: "unknown", rendered: false },
      content: { default: {}, rendered: false },
      children: { default: [], rendered: false },
    };
  },

  renderHTML({ node }) {
    return [
      "div",
      { class: "dbme-unknown-block" },
      `This ${String(node.attrs.kind)} block needs a newer version of DataBase.Me.`,
    ];
  },
});

/** A mention of another object, shown by its current title. `[[` starts one. */
export const Mention = Node.create<{ services: NodeServices | null }>({
  name: "mention",
  group: "inline",
  inline: true,
  atom: true,
  selectable: true,

  addOptions() {
    return { services: null };
  },

  addAttributes() {
    return { id: { default: null, rendered: false } };
  },

  renderHTML() {
    return ["span", { class: "dbme-mention" }];
  },

  renderText({ node }) {
    return `[[${String(node.attrs.id)}]]`;
  },

  addNodeView() {
    const services = this.options.services;
    return ({ node }) => {
      const dom = document.createElement("span");
      dom.className = "dbme-mention";
      dom.contentEditable = "false";
      const id = node.attrs.id as string;
      dom.dataset.mention = id;
      dom.textContent = "…";
      const stop = services?.observeTitle(id, (title, trashed) => {
        dom.textContent = title === null ? "Missing object" : title === "" ? "Untitled" : title;
        dom.classList.toggle("dbme-mention--missing", title === null || trashed);
      });
      dom.addEventListener("click", (event) => {
        event.preventDefault();
        sendOpen(dom, id);
      });
      return { dom, destroy: () => stop?.() };
    };
  },

  addKeyboardShortcuts() {
    // Enter on a selected mention opens it, so mentions are reachable from the keyboard.
    return {
      Enter: () => {
        const { selection } = this.editor.state;
        if (!(selection instanceof NodeSelection) || selection.node.type.name !== this.name)
          return false;
        sendOpen(this.editor.view.dom, selection.node.attrs.id as string);
        return true;
      },
    };
  },

  addProseMirrorPlugins() {
    const services = this.options.services;
    return services ? [mentionSuggestion(this.editor, this.type, services.search)] : [];
  },
});

/** Every extension of the block editor. Without `services` the schema is the same (for tests). */
export function editorExtensions(services: NodeServices | null = null): Extensions {
  const extensions: AnyExtension[] = [
    StarterKit.configure({
      // Undo is the vault's (ADR-0016); a blockquote is a single text block here (Quote).
      undoRedo: false,
      blockquote: false,
      trailingNode: false,
      heading: { levels: [1, 2, 3, 4, 5, 6] },
      link: {
        openOnClick: false,
        autolink: true,
        HTMLAttributes: { rel: "noopener noreferrer nofollow", target: null },
      },
    }),
    TaskList,
    TaskItem.configure({
      nested: true,
      a11y: {
        checkboxLabel: (node, checked) => `${checked ? "Done" : "To do"}: ${node.textContent}`,
      },
    }),
    Quote,
    ImageBlock.configure({ services }),
    UnknownBlock,
    Mention.configure({ services }),
    Placeholder.configure({ placeholder: "Write here. Type [[ to link to another page." }),
    BlockIds,
  ];
  return extensions;
}
