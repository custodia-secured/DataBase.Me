// SPDX-License-Identifier: AGPL-3.0-or-later
/** The block editor of the Document view: TipTap on the object's blocks, kept in sync (sync.ts). */
import { commands, unwrap, useBlocks, type BlockRow_Serialize, type Id } from "@databaseme/client";
import {
  Bold,
  Heading2,
  Italic,
  List,
  ListOrdered,
  ListTodo,
  SquareCode,
  StatusMessage,
  TextQuote,
  type LucideIcon,
} from "@databaseme/ui";
import { Extension, type Editor } from "@tiptap/core";
import { EditorContent, useEditor, useEditorState } from "@tiptap/react";
import { useQueryClient } from "@tanstack/react-query";
import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import { actions, registerFlusher, reportError } from "../actions";
import { blocksToDoc, rowsToTree } from "./mapping";
import { editorExtensions, OPEN_EVENT } from "./nodes";
import { nodeServices } from "./services";
import { EditorSync } from "./sync";

export interface BlockEditorProps {
  objectId: Id;
  /** Opens another object, from a mention. */
  onOpen: (id: Id) => void;
  /** The editor's accessible name. */
  label?: string;
}

/** Undo and redo are the vault's (ADR-0016); Escape leaves the editor so Tab moves on. */
function vaultKeys(): Extension {
  return Extension.create({
    name: "vaultKeys",
    addKeyboardShortcuts() {
      const undo = () => {
        void actions.undo();
        return true;
      };
      const redo = () => {
        void actions.redo();
        return true;
      };
      return {
        "Mod-z": undo,
        "Shift-Mod-z": redo,
        "Mod-y": redo,
        Escape: () => this.editor.commands.blur(),
      };
    },
  });
}

export function BlockEditor(props: BlockEditorProps) {
  const blocks = useBlocks(props.objectId);
  if (blocks.isError)
    return <StatusMessage tone="error">The text could not be read.</StatusMessage>;
  if (!blocks.data) return <StatusMessage>Loading…</StatusMessage>;
  return <LoadedEditor key={props.objectId} {...props} rows={blocks.data} />;
}

function LoadedEditor({
  objectId,
  onOpen,
  rows,
  label = "Text",
}: BlockEditorProps & { rows: BlockRow_Serialize[] }) {
  const client = useQueryClient();
  const [initialRows] = useState(rows);
  const [services] = useState(() => nodeServices(client, objectId));
  const frame = useRef<HTMLDivElement>(null);
  const sync = useRef<EditorSync | null>(null);

  const editor = useEditor({
    extensions: [...editorExtensions(services), vaultKeys()],
    content: blocksToDoc(rowsToTree(initialRows)),
    injectCSS: false,
    shouldRerenderOnTransaction: false,
    editorProps: {
      attributes: {
        class: "dbme-editor",
        role: "textbox",
        "aria-multiline": "true",
        "aria-label": label,
      },
    },
    onUpdate: () => {
      sync.current?.changed();
    },
    onBlur: () => {
      void sync.current?.flush();
    },
  });

  useEffect(() => {
    const current = new EditorSync(
      {
        getJSON: () => editor.getJSON(),
        show: (doc) => {
          const { from, to } = editor.state.selection;
          editor.commands.setContent(doc, { emitUpdate: false });
          const size = editor.state.doc.content.size;
          if (editor.isFocused) {
            editor.commands.setTextSelection({
              from: Math.min(from, size),
              to: Math.min(to, size),
            });
          }
        },
      },
      {
        load: () => unwrap(commands.blocksList(objectId)),
        apply: async (changes) => {
          await unwrap(commands.blocksApply(objectId, changes));
        },
      },
      initialRows,
      reportError,
    );
    sync.current = current;
    const unregister = registerFlusher(() => current.flush());
    return () => {
      unregister();
      sync.current = null;
      void current.dispose();
    };
  }, [editor, objectId, initialRows]);

  useEffect(() => {
    if (rows !== initialRows) void sync.current?.reconcile();
  }, [rows, initialRows]);

  useEffect(() => {
    const element = frame.current;
    if (!element) return;
    const onMentionOpen = (event: Event) => {
      onOpen((event as CustomEvent<{ id: Id }>).detail.id);
    };
    element.addEventListener(OPEN_EVENT, onMentionOpen);
    return () => {
      element.removeEventListener(OPEN_EVENT, onMentionOpen);
    };
  }, [onOpen]);

  return (
    <div ref={frame} className="dbme-editor-frame">
      <Toolbar editor={editor} />
      <EditorContent editor={editor} className="dbme-editor-host" />
    </div>
  );
}

interface Tool {
  label: string;
  icon: LucideIcon;
  active: (editor: Editor) => boolean;
  run: (editor: Editor) => void;
}

const TOOLS: Tool[] = [
  {
    label: "Bold",
    icon: Bold,
    active: (e) => e.isActive("bold"),
    run: (e) => e.chain().focus().toggleBold().run(),
  },
  {
    label: "Italic",
    icon: Italic,
    active: (e) => e.isActive("italic"),
    run: (e) => e.chain().focus().toggleItalic().run(),
  },
  {
    label: "Heading",
    icon: Heading2,
    active: (e) => e.isActive("heading"),
    // Pressed means "a heading" at any level, so pressing it again always gives a paragraph.
    run: (e) =>
      e.isActive("heading")
        ? e.chain().focus().setParagraph().run()
        : e.chain().focus().setHeading({ level: 2 }).run(),
  },
  {
    label: "Bulleted list",
    icon: List,
    active: (e) => e.isActive("bulletList"),
    run: (e) => e.chain().focus().toggleBulletList().run(),
  },
  {
    label: "Numbered list",
    icon: ListOrdered,
    active: (e) => e.isActive("orderedList"),
    run: (e) => e.chain().focus().toggleOrderedList().run(),
  },
  {
    label: "To-do list",
    icon: ListTodo,
    active: (e) => e.isActive("taskList"),
    run: (e) => e.chain().focus().toggleTaskList().run(),
  },
  {
    label: "Quote",
    icon: TextQuote,
    active: (e) => e.isActive("quote"),
    run: (e) => e.chain().focus().toggleNode("quote", "paragraph").run(),
  },
  {
    label: "Code block",
    icon: SquareCode,
    active: (e) => e.isActive("codeBlock"),
    run: (e) => e.chain().focus().toggleCodeBlock().run(),
  },
];

/** Formatting buttons, for pointers and touch. One tab stop; the arrow keys move between them. */
function Toolbar({ editor }: { editor: Editor }) {
  const active = useEditorState({
    editor,
    selector: ({ editor: current }) => TOOLS.map((tool) => tool.active(current)),
  });
  const [focus, setFocus] = useState(0);
  const buttons = useRef<(HTMLButtonElement | null)[]>([]);

  const onKeyDown = (event: KeyboardEvent) => {
    const step = event.key === "ArrowRight" ? 1 : event.key === "ArrowLeft" ? -1 : 0;
    const to = event.key === "Home" ? 0 : event.key === "End" ? TOOLS.length - 1 : focus + step;
    if (step === 0 && event.key !== "Home" && event.key !== "End") return;
    event.preventDefault();
    const next = (to + TOOLS.length) % TOOLS.length;
    setFocus(next);
    buttons.current[next]?.focus();
  };

  return (
    <div className="dbme-toolbar" role="toolbar" aria-label="Formatting" onKeyDown={onKeyDown}>
      {TOOLS.map((tool, index) => {
        const Icon = tool.icon;
        return (
          <button
            key={tool.label}
            ref={(button) => {
              buttons.current[index] = button;
            }}
            type="button"
            className="dbme-toolbar__button"
            aria-label={tool.label}
            title={tool.label}
            aria-pressed={active[index] ?? false}
            tabIndex={index === focus ? 0 : -1}
            // Keep the editor's selection: a press must not move focus out of the text. On the
            // mouse event, not the pointer one: WebKit drops a tap's click when pointerdown is
            // cancelled, and the mouse event a tap sends comes too late to stop it.
            onMouseDown={(event) => {
              event.preventDefault();
            }}
            onClick={() => {
              setFocus(index);
              tool.run(editor);
            }}
          >
            <Icon aria-hidden="true" focusable="false" size={16} />
          </button>
        );
      })}
    </div>
  );
}
