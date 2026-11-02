// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * The command palette (ADR-0020): Ctrl+K anywhere. Typing searches the whole vault, with a
 * snippet beside each title; a line that starts with `>` lists commands instead. An ARIA combobox
 * over a listbox: ↑ ↓ move, Enter chooses, Ctrl+Enter shows every result, Escape closes.
 */
import { useFullSearch, type Id, type Query_Deserialize } from "@databaseme/client";
import { Dialog } from "@databaseme/ui";
import { useEffect, useId, useRef, useState, type KeyboardEvent } from "react";
import { Snippet } from "./snippet";

export interface PaletteCommand {
  id: string;
  label: string;
  run: () => void;
}

interface Option {
  id: string;
  label: string;
  detail: string | null;
  choose: () => void;
}

export interface CommandPaletteProps {
  open: boolean;
  onClose: () => void;
  /** Opens an object. */
  onOpen: (id: Id) => void;
  /** Shows every result of a query on the results surface. */
  onShowResults: (query: Query_Deserialize) => void;
  commands: readonly PaletteCommand[];
}

export function CommandPalette({
  open,
  onClose,
  onOpen,
  onShowResults,
  commands,
}: CommandPaletteProps) {
  const [text, setText] = useState("");
  const [active, setActive] = useState(0);
  const listId = useId();
  const list = useRef<HTMLUListElement>(null);
  const commandMode = text.startsWith(">");
  const needle = commandMode ? text.slice(1).trim() : text;
  const hits = useFullSearch(needle, 12, open && !commandMode);

  const close = () => {
    setText("");
    setActive(0);
    onClose();
  };
  const showAll = () => {
    if (needle.trim() === "") return;
    close();
    onShowResults({ search: needle });
  };

  const options: Option[] = commandMode
    ? commands
        .filter((command) => command.label.toLowerCase().includes(needle.toLowerCase()))
        .map((command) => ({
          id: `command-${command.id}`,
          label: command.label,
          detail: null,
          choose: () => {
            close();
            command.run();
          },
        }))
    : [
        ...(hits.data ?? []).map((hit): Option => {
          const { object } = hit.summary;
          return {
            id: `object-${object.id}`,
            label: object.title || "Untitled",
            detail: hit.snippet === "" ? null : hit.snippet,
            choose: () => {
              close();
              onOpen(object.id);
            },
          };
        }),
        ...(needle.trim() === ""
          ? []
          : [
              {
                id: "all",
                label: `Show all results for “${needle.trim()}”`,
                detail: null,
                choose: showAll,
              },
            ]),
      ];
  const current = options[Math.min(active, options.length - 1)];
  const optionId = (option: Option) => `${listId}-${option.id}`;

  useEffect(() => {
    if (!current) return;
    list.current
      ?.querySelector(`[id="${optionId(current)}"]`)
      ?.scrollIntoView({ block: "nearest" });
    // The id of the option is enough to find it again.
    // eslint-disable-next-line react-hooks/exhaustive-deps -- the option's id is what finds it again
  }, [current?.id]);

  const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    switch (event.key) {
      case "ArrowDown":
        setActive((at) => Math.min(at + 1, options.length - 1));
        break;
      case "ArrowUp":
        setActive((at) => Math.max(at - 1, 0));
        break;
      case "Home":
        setActive(0);
        break;
      case "End":
        setActive(Math.max(options.length - 1, 0));
        break;
      case "Enter":
        if (event.ctrlKey || event.metaKey) showAll();
        else current?.choose();
        break;
      default:
        return;
    }
    event.preventDefault();
  };

  return (
    <Dialog open={open} onClose={close} title="Search" className="dbme-dialog--palette">
      <div className="dbme-palette">
        <input
          // The palette exists to take the keyboard the moment it opens.
          autoFocus
          className="dbme-input dbme-palette__input"
          type="text"
          role="combobox"
          aria-label="Search or type > for a command"
          aria-expanded={options.length > 0}
          aria-controls={listId}
          aria-activedescendant={current ? optionId(current) : undefined}
          aria-autocomplete="list"
          autoComplete="off"
          placeholder="Search pages, or type > for a command"
          value={text}
          onChange={(event) => {
            setText(event.target.value);
            setActive(0);
          }}
          onKeyDown={onKeyDown}
        />
        <ul
          ref={list}
          id={listId}
          role="listbox"
          aria-label={commandMode ? "Commands" : "Results"}
          className="dbme-palette__list"
        >
          {options.map((option) => (
            <li
              key={option.id}
              id={optionId(option)}
              role="option"
              aria-selected={option === current}
              className={`dbme-palette__option${option === current ? " is-active" : ""}`}
              onMouseDown={(event) => {
                // Keep the keyboard in the field.
                event.preventDefault();
              }}
              onClick={option.choose}
            >
              <span className="dbme-palette__label">{option.label}</span>
              {option.detail !== null && option.detail !== option.label ? (
                <Snippet text={option.detail} />
              ) : null}
            </li>
          ))}
        </ul>
        {options.length === 0 && needle.trim() !== "" ? (
          <p className="dbme-palette__hint" role="status">
            {commandMode ? "No such command." : "Nothing matches."}
          </p>
        ) : (
          <p className="dbme-palette__hint">
            Type &gt; for commands. Enter opens; Ctrl+Enter shows every result.
          </p>
        )}
      </div>
    </Dialog>
  );
}
