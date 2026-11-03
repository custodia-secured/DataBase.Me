// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * The shell (M2): the hierarchy sidebar, a header with the path, view switcher, split and undo,
 * and one or two panes showing the open object in a view. On a phone the sidebar is a drawer.
 */
import {
  commands,
  startLiveRefresh,
  unwrap,
  useAncestors,
  useObject,
  useMirrorStatus,
  usePage,
  useVaultInfo,
  useViews,
  MANUAL,
  type Id,
} from "@databaseme/client";
import {
  Button,
  Columns2,
  Dialog,
  FolderSync,
  IconButton,
  PanelLeft,
  Redo2,
  Search,
  StatusMessage,
  Undo2,
  useMediaQuery,
  X,
} from "@databaseme/ui";
import {
  actions,
  CommandPalette,
  FilesDialog,
  HoldBanner,
  onActionError,
  ResultsView,
  Sidebar,
  VIEWS,
  viewById,
  type PaletteCommand,
} from "@databaseme/views";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useId, useRef, useState, type KeyboardEvent } from "react";
import { notifyReady } from "./ipc";
import { useShell } from "./store";

const WIDE = "(min-width: 768px)";
const SPLIT = "(min-width: 1100px)";

export function App() {
  const client = useQueryClient();
  const [reported, setReported] = useState(false);

  useEffect(() => {
    notifyReady()
      .catch((error: unknown) => {
        console.error("Could not tell the shell the UI is ready", error);
      })
      .finally(() => {
        setReported(true);
      });
  }, []);

  useEffect(() => {
    let stop: (() => void) | undefined;
    let cancelled = false;
    void startLiveRefresh(client).then((unlisten) => {
      if (cancelled) unlisten();
      else stop = unlisten;
    });
    return () => {
      cancelled = true;
      stop?.();
    };
  }, [client]);

  // The shell keeps one default vault (ADR-0017); vault choice arrives with Settings.
  const vault = useQuery({
    queryKey: ["bootstrap"],
    queryFn: async () =>
      (await unwrap(commands.vaultInfo())) ?? (await unwrap(commands.vaultOpenDefault())),
    enabled: reported,
    staleTime: Infinity,
    retry: false,
  });

  if (vault.isError) {
    return (
      <main className="shell-message">
        <h1>DataBase.Me</h1>
        <StatusMessage tone="error">
          The vault could not be opened: {vault.error.message}
        </StatusMessage>
      </main>
    );
  }
  if (!vault.data) {
    return (
      <main className="shell-message">
        <h1>DataBase.Me</h1>
        <StatusMessage>Opening your vault…</StatusMessage>
        <p className="shell-meta">Version {__APP_VERSION__}</p>
      </main>
    );
  }
  return <Shell />;
}

function Shell() {
  const wide = useMediaQuery(WIDE);
  const splitFits = useMediaQuery(SPLIT);
  const {
    selected,
    select,
    split,
    setSplit,
    drawer,
    setDrawer,
    palette,
    setPalette,
    files,
    setFiles,
    results,
    showResults,
    setView,
  } = useShell();
  const topLevel = usePage({ kind: "children", parent: null }, MANUAL, 0, 1);
  const savedViews = useViews();

  // Open the first page when nothing is open yet.
  const first = topLevel.data?.[0]?.object.id ?? null;
  useEffect(() => {
    if (selected === null && first !== null) select(first);
  }, [selected, first, select]);

  useGlobalKeys(() => {
    setPalette(true);
  });

  const openView = (id: Id) => {
    showResults({ kind: "view", id });
  };
  const sidebar = (
    <Sidebar
      selected={selected}
      onSelect={select}
      openView={results?.kind === "view" ? results.id : null}
      onOpenView={openView}
    />
  );
  const panes = split && splitFits ? 2 : 1;
  const paletteCommands: PaletteCommand[] = [
    ...VIEWS.map((view) => ({
      id: `view-${view.id}`,
      label: `Switch to ${view.label}`,
      run: () => {
        setView(0, view.id);
      },
    })),
    {
      id: "new-page",
      label: "New page",
      run: () => {
        void actions.create({ title: "", parent_id: null, position: "last" }).then((id) => {
          if (id !== undefined) select(id);
        });
      },
    },
    { id: "undo", label: "Undo", run: () => void actions.undo() },
    { id: "redo", label: "Redo", run: () => void actions.redo() },
    {
      id: "files",
      label: "Files folder",
      run: () => {
        setFiles(true);
      },
    },
    ...(splitFits
      ? [
          {
            id: "split",
            label: split ? "Close the split view" : "Split view",
            run: () => {
              setSplit(!split);
            },
          },
        ]
      : []),
    ...(savedViews.data ?? []).map((view) => ({
      id: `open-view-${view.id}`,
      label: `Open view: ${view.name}`,
      run: () => {
        openView(view.id);
      },
    })),
  ];

  return (
    <div className="app">
      <header className="app__bar">
        {wide ? null : (
          <IconButton
            label="Pages"
            icon={PanelLeft}
            aria-expanded={drawer}
            onClick={() => {
              setDrawer(true);
            }}
          />
        )}
        <h1 className="app__brand">DataBase.Me</h1>
        {selected ? <Breadcrumbs id={selected} onOpen={select} /> : null}
        <span className="app__spacer" />
        <IconButton
          label="Search"
          icon={Search}
          onClick={() => {
            setPalette(true);
          }}
        />
        {splitFits ? (
          <IconButton
            label="Split view"
            icon={Columns2}
            aria-pressed={split}
            onClick={() => {
              setSplit(!split);
            }}
          />
        ) : null}
        <FilesButton
          onClick={() => {
            setFiles(true);
          }}
        />
        <HistoryButtons />
      </header>
      <HoldBanner
        onDetails={() => {
          setFiles(true);
        }}
      />
      {wide ? <aside className="app__sidebar">{sidebar}</aside> : null}
      {wide ? null : (
        <Dialog
          open={drawer}
          onClose={() => {
            setDrawer(false);
          }}
          title="Pages"
          className="app__drawer"
        >
          <IconButton
            label="Close pages"
            icon={X}
            className="app__drawer-close"
            onClick={() => {
              setDrawer(false);
            }}
          />
          {sidebar}
        </Dialog>
      )}
      <main className={`app__main app__main--${String(panes)}`}>
        {results ? (
          <ResultsView
            source={results}
            onOpen={select}
            onClose={() => {
              showResults(null);
            }}
            onSaved={openView}
          />
        ) : selected === null ? (
          <Empty onCreated={select} ready={topLevel.isSuccess} />
        ) : (
          <>
            <Pane index={0} objectId={selected} onOpen={select} />
            {panes === 2 ? <Pane index={1} objectId={selected} onOpen={select} /> : null}
          </>
        )}
      </main>
      <CommandPalette
        open={palette}
        onClose={() => {
          setPalette(false);
        }}
        onOpen={select}
        onShowResults={(query) => {
          showResults({ kind: "search", query });
        }}
        commands={paletteCommands}
      />
      <FilesDialog
        open={files}
        onClose={() => {
          setFiles(false);
        }}
        onOpen={select}
      />
      <Errors />
    </div>
  );
}

/** Opens the files folder dialog; the badge counts the open entries of the Conflicts inbox. */
function FilesButton({ onClick }: { onClick: () => void }) {
  const status = useMirrorStatus();
  const open = status.data?.open_conflicts ?? 0;
  const label =
    open === 0
      ? "Files folder"
      : `Files folder, ${String(open)} ${open === 1 ? "conflict" : "conflicts"}`;
  return (
    <span className="dbme-files-badge">
      <IconButton label={label} icon={FolderSync} onClick={onClick} />
      {open > 0 ? (
        <span className="dbme-files-badge__count" aria-hidden="true">
          {open > 99 ? "99+" : open}
        </span>
      ) : null}
    </span>
  );
}

/**
 * Ctrl+K (⌘K) opens the palette anywhere. Ctrl+Z and Ctrl+Shift+Z (or Ctrl+Y) undo and redo the
 * vault, outside text fields.
 */
function useGlobalKeys(openPalette: () => void) {
  useEffect(() => {
    const onKeyDown = (event: globalThis.KeyboardEvent) => {
      if (!(event.ctrlKey || event.metaKey) || event.altKey) return;
      const key = event.key.toLowerCase();
      if (key === "k" && !event.shiftKey) {
        event.preventDefault();
        openPalette();
        return;
      }
      const target = event.target as HTMLElement | null;
      if (target?.closest("input, textarea, select, [contenteditable='true']")) return;
      if (key === "z" && !event.shiftKey) {
        event.preventDefault();
        void actions.undo();
      } else if ((key === "z" && event.shiftKey) || key === "y") {
        event.preventDefault();
        void actions.redo();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => {
      window.removeEventListener("keydown", onKeyDown);
    };
  }, [openPalette]);
}

function HistoryButtons() {
  const info = useVaultInfo();
  return (
    <>
      <IconButton
        label="Undo"
        icon={Undo2}
        disabled={!info.data?.can_undo}
        onClick={() => void actions.undo()}
      />
      <IconButton
        label="Redo"
        icon={Redo2}
        disabled={!info.data?.can_redo}
        onClick={() => void actions.redo()}
      />
    </>
  );
}

function Breadcrumbs({ id, onOpen }: { id: Id; onOpen: (id: Id) => void }) {
  const ancestors = useAncestors(id);
  const object = useObject(id);
  const trail = ancestors.data ?? [];
  return (
    <nav className="app__crumbs" aria-label="Path">
      <ol>
        {trail.map((ancestor) => (
          <li key={ancestor.id}>
            <button
              type="button"
              className="app__crumb"
              onClick={() => {
                onOpen(ancestor.id);
              }}
            >
              {ancestor.title || "Untitled"}
            </button>
          </li>
        ))}
        <li aria-current="page" className="app__crumb app__crumb--current">
          {object.data && object.data.title !== "" ? object.data.title : "Untitled"}
        </li>
      </ol>
    </nav>
  );
}

function Pane({
  index,
  objectId,
  onOpen,
}: {
  index: 0 | 1;
  objectId: Id;
  onOpen: (id: Id) => void;
}) {
  const viewId = useShell((state) => state.views[index]);
  const setView = useShell((state) => state.setView);
  const object = useObject(objectId);
  const base = useId();
  const tabs = useRef<(HTMLButtonElement | null)[]>([]);
  const view = viewById(viewId);
  const View = view.component;

  const onKeyDown = (event: KeyboardEvent) => {
    const at = VIEWS.findIndex((candidate) => candidate.id === viewId);
    const step = event.key === "ArrowRight" ? 1 : event.key === "ArrowLeft" ? -1 : 0;
    let next = at + step;
    if (event.key === "Home") next = 0;
    else if (event.key === "End") next = VIEWS.length - 1;
    else if (step === 0) return;
    event.preventDefault();
    next = (next + VIEWS.length) % VIEWS.length;
    const target = VIEWS[next];
    if (!target) return;
    setView(index, target.id);
    tabs.current[next]?.focus();
  };

  return (
    <section className="pane" aria-label={index === 0 ? "Main pane" : "Second pane"}>
      <div className="pane__tabs" role="tablist" aria-label="Views" onKeyDown={onKeyDown}>
        {VIEWS.map((candidate, i) => {
          const Icon = candidate.icon;
          const chosen = candidate.id === viewId;
          return (
            <button
              key={candidate.id}
              ref={(button) => {
                tabs.current[i] = button;
              }}
              type="button"
              role="tab"
              id={`${base}-tab-${candidate.id}`}
              aria-selected={chosen}
              aria-label={candidate.label}
              title={candidate.label}
              aria-controls={`${base}-panel`}
              tabIndex={chosen ? 0 : -1}
              className="pane__tab"
              onClick={() => {
                setView(index, candidate.id);
              }}
            >
              <Icon aria-hidden="true" size={16} />
              <span>{candidate.label}</span>
            </button>
          );
        })}
      </div>
      {object.data?.deleted_at != null ? (
        <div className="pane__trashed">
          <StatusMessage>This page is in the Trash.</StatusMessage>
          <Button
            onClick={() =>
              void unwrap(commands.objectRestore(objectId)).catch((error: unknown) => {
                console.error(error);
              })
            }
          >
            Restore
          </Button>
        </div>
      ) : null}
      <div
        className={`pane__panel pane__panel--${view.id}`}
        role="tabpanel"
        id={`${base}-panel`}
        aria-labelledby={`${base}-tab-${view.id}`}
      >
        <View objectId={objectId} onOpen={onOpen} />
      </div>
    </section>
  );
}

function Empty({ onCreated, ready }: { onCreated: (id: Id) => void; ready: boolean }) {
  if (!ready) return <StatusMessage>Loading…</StatusMessage>;
  return (
    <div className="app__empty">
      <h2>Nothing here yet</h2>
      <p>Pages hold your notes, lists and tables. Make the first one.</p>
      <Button
        variant="primary"
        onClick={() =>
          void actions.create({ title: "", parent_id: null, position: "last" }).then((id) => {
            if (id !== undefined) onCreated(id);
          })
        }
      >
        New page
      </Button>
    </div>
  );
}

/** The latest failed change, until it is dismissed or 10 s pass. */
function Errors() {
  const [message, setMessage] = useState<string | null>(null);
  useEffect(() => onActionError(setMessage), []);
  useEffect(() => {
    if (message === null) return;
    const timer = setTimeout(() => {
      setMessage(null);
    }, 10_000);
    return () => {
      clearTimeout(timer);
    };
  }, [message]);
  return (
    <div className="app__errors">
      {message === null ? null : (
        <div className="app__error">
          <StatusMessage tone="error">{message}</StatusMessage>
          <IconButton
            label="Dismiss"
            icon={X}
            onClick={() => {
              setMessage(null);
            }}
          />
        </div>
      )}
    </div>
  );
}
