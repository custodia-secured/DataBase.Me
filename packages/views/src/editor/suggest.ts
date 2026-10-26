// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * The mention picker. Typing `[[` opens a listbox of objects whose titles match what follows; the
 * arrow keys choose, Enter or Tab inserts, Escape closes. The editor stays focused throughout and
 * points at the active option with `aria-activedescendant`, as a combobox does.
 */
import type { Editor } from "@tiptap/core";
import type { NodeType } from "@tiptap/pm/model";
import { PluginKey } from "@tiptap/pm/state";
import { Suggestion, type SuggestionProps } from "@tiptap/suggestion";

/** An object the picker offers. */
export interface MentionCandidate {
  id: string;
  title: string;
}

export type MentionSearch = (text: string) => Promise<MentionCandidate[]>;

const key = new PluginKey("mentionSuggestion");
let pickers = 0;

/** The `[[` suggestion plugin, inserting `type` nodes. */
export function mentionSuggestion(editor: Editor, type: NodeType, search: MentionSearch) {
  return Suggestion<MentionCandidate, MentionCandidate>({
    editor,
    pluginKey: key,
    char: "[[",
    allowSpaces: true,
    allowedPrefixes: null,
    items: ({ query }) => search(query),
    command: ({ editor: target, range, props }) => {
      target
        .chain()
        .focus()
        .insertContentAt(range, [
          { type: type.name, attrs: { id: props.id } },
          { type: "text", text: " " },
        ])
        .run();
    },
    render: () => {
      const listId = `dbme-mention-list-${String((pickers += 1))}`;
      let list: HTMLElement | null = null;
      let unmount: (() => void) | null = null;
      let current: SuggestionProps<MentionCandidate, MentionCandidate> | null = null;
      let active = 0;

      const target = () => current?.editor.view.dom;

      const draw = () => {
        if (!list || !current) return;
        const { items, loading } = current;
        list.replaceChildren();
        if (items.length === 0) {
          const empty = document.createElement("div");
          empty.className = "dbme-suggest__empty";
          empty.textContent = loading ? "Searching…" : "No matching objects";
          list.append(empty);
          target()?.removeAttribute("aria-activedescendant");
          return;
        }
        active = Math.min(active, items.length - 1);
        items.forEach((item, index) => {
          const option = document.createElement("div");
          option.id = `${listId}-${String(index)}`;
          option.className = "dbme-suggest__option";
          option.setAttribute("role", "option");
          option.setAttribute("aria-selected", String(index === active));
          option.textContent = item.title === "" ? "Untitled" : item.title;
          option.addEventListener("pointerdown", (event) => {
            event.preventDefault();
            current?.command(item);
          });
          list?.append(option);
        });
        target()?.setAttribute("aria-activedescendant", `${listId}-${String(active)}`);
      };

      const close = () => {
        unmount?.();
        unmount = null;
        list = null;
        const dom = target();
        dom?.removeAttribute("aria-activedescendant");
        dom?.removeAttribute("aria-controls");
      };

      return {
        onStart: (props) => {
          current = props;
          active = 0;
          list = document.createElement("div");
          list.id = listId;
          list.className = "dbme-suggest";
          list.setAttribute("role", "listbox");
          list.setAttribute("aria-label", "Objects to mention");
          draw();
          unmount = props.mount(list);
          const dom = target();
          dom?.setAttribute("aria-controls", listId);
        },
        onUpdate: (props) => {
          current = props;
          draw();
        },
        onKeyDown: ({ event }) => {
          const count = current?.items.length ?? 0;
          if (event.key === "Escape") {
            close();
            return true;
          }
          if (count === 0) return false;
          if (event.key === "ArrowDown" || event.key === "ArrowUp") {
            active = (active + (event.key === "ArrowDown" ? 1 : count - 1)) % count;
            draw();
            return true;
          }
          if (event.key === "Enter" || event.key === "Tab") {
            const item = current?.items[active];
            if (item) current?.command(item);
            return true;
          }
          return false;
        },
        onExit: () => {
          close();
          current = null;
        },
      };
    },
  });
}
