// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * The shell (M2): the hierarchy sidebar, a header with the path, view switcher, split and undo,
 * and one or two panes showing the open object in a view. On a phone the sidebar is a drawer.
 */
import {
  commands,
  events,
  forgetVault,
  startLiveRefresh,
  unwrap,
  useAncestors,
  useObject,
  useMirrorStatus,
  usePage,
  useSettings,
  useVaultInfo,
  useViews,
  MANUAL,
  SETTINGS_KEY,
  setSettings,
  type Id,
  type VaultInfo,
} from "@databaseme/client";
import {
  applyTheme,
  Button,
  Columns2,
  Dialog,
  FolderInput,
  FolderSync,
  IconButton,
  PanelLeft,
  Redo2,
  Search,
  SettingsIcon,
  StatusMessage,
  Undo2,
  useMediaQuery,
  X,
  type ThemeChoice,
} from "@databaseme/ui";
import {
  actions,
  CommandPalette,
  FilesDialog,
  HoldBanner,
  ImportDialog,
  Onboarding,
  onActionError,
  reportError,
  SettingsDialog,
  ResultsView,
  Sidebar,
  useMode,
  viewById,
  visibleViews,
  type Mode,
  type PaletteCommand,
} from "@databaseme/views";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { isTauri } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { useEffect, useId, useRef, useState, type KeyboardEvent } from "react";
import { notifyReady } from "./ipc";
import { useShell } from "./store";

const WIDE = "(min-width: 768px)";
const SPLIT = "(min-width: 1100px)";
/** One empty list, so a closed Import dialog's props do not change between renders. */
const NO_PATHS: readonly string[] = [];
/** Where Help points, as text to copy: the app opens no browser and makes no request (M7). */
const LINKS = {
  docs: "https://custodia-secured.github.io/DataBase.Me/",
  issues: "https://github.com/custodia-secured/DataBase.Me/issues",
};

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

  // What to show first (M7): the vault already open; else onboarding, until it has run once;
  // else the last vault, or the default one (ADR-0017).
  const vault = useQuery({
    queryKey: ["bootstrap"],
    queryFn: async (): Promise<Bootstrap> => {
      const open = await unwrap(commands.vaultInfo());
      const settings = await unwrap(commands.settingsGet());
      client.setQueryData(SETTINGS_KEY, settings);
      if (open) return { vault: open, defaultVault: null };
      if (!settings.onboarded) {
        const info = await commands.appInfo();
        return { vault: null, defaultVault: info.default_vault };
      }
      let last: VaultInfo | null = null;
      if (settings.last_vault !== null) {
        last = await unwrap(commands.vaultOpen(settings.last_vault)).catch(() => null);
      }
      const opened = last ?? (await unwrap(commands.vaultOpenDefault()));
      if (opened.path !== settings.last_vault) {
        await setSettings(client, { last_vault: opened.path });
      }
      return { vault: opened, defaultVault: null };
    },
    enabled: reported,
    staleTime: Infinity,
    retry: false,
  });
  const setupAgain = useShell((state) => state.setupAgain);
  const setSetupAgain = useShell((state) => state.setSetupAgain);
  // Another vault means nothing of the last one stays selected (M7).
  const vaultId = vault.data?.vault?.id;
  useEffect(() => {
    useShell.getState().select(null);
  }, [vaultId]);

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
  if (vault.data.vault === null || setupAgain) {
    return (
      <Onboarding
        vaultPath={vault.data.vault?.path ?? null}
        defaultVault={vault.data.defaultVault}
        pickFolder={isTauri() ? pickFolder : undefined}
        onDone={() => {
          setSetupAgain(false);
          void client.invalidateQueries({ queryKey: ["bootstrap"] });
        }}
      />
    );
  }
  // Another vault is another shell: its panes, drawer and selection start afresh.
  return <Shell key={vault.data.vault.id} />;
}

/** What the first query settles: the vault to show, or none and onboarding's default path. */
interface Bootstrap {
  vault: VaultInfo | null;
  defaultVault: string | null;
}

/** The OS's folder picker, through the dialog plugin (M7 gate 1a); `null` when cancelled. */
async function pickFolder(): Promise<string | null> {
  const picked = await open({ directory: true, multiple: false });
  return typeof picked === "string" ? picked : null;
}

function Shell() {
  const client = useQueryClient();
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
    importing,
    setImporting,
    setSetupAgain,
    settingsOpen,
    setSettingsOpen,
    results,
    showResults,
    setView,
  } = useShell();
  const topLevel = usePage({ kind: "children", parent: null }, MANUAL, 0, 1);
  const savedViews = useViews();
  const about = useQuery({
    queryKey: ["app-info"],
    queryFn: () => commands.appInfo(),
    staleTime: Infinity,
  });

  // Files dropped on the window open the Import dialog with their paths (M6 Decision 9).
  useEffect(() => {
    let stop: (() => void) | undefined;
    let cancelled = false;
    void events.importDropped
      .listen((event) => {
        setImporting(event.payload);
      })
      .then((unlisten) => {
        if (cancelled) unlisten();
        else stop = unlisten;
      });
    return () => {
      cancelled = true;
      stop?.();
    };
  }, [setImporting]);

  // The theme follows the settings (M7): the hint applied before the first paint is confirmed
  // or corrected here, and every change goes through the same place.
  const settings = useSettings();
  const theme = settings.data?.theme;
  useEffect(() => {
    if (theme !== undefined) applyTheme(theme);
  }, [theme]);
  const chooseTheme = (next: ThemeChoice) => {
    setSettings(client, { theme: next }).catch(reportError);
  };

  // Open the first page when nothing is open yet.
  // Placeholder data is the previous vault's while another loads (M7): never open from it.
  const first = topLevel.isPlaceholderData ? null : (topLevel.data?.[0]?.object.id ?? null);
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
  const mode = useMode();
  const chooseMode = (next: Mode) => {
    setSettings(client, { mode: next }).catch(reportError);
  };
  const paletteCommands: PaletteCommand[] = [
    ...visibleViews(mode).map((view) => ({
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
    {
      id: "today",
      label: "Today",
      run: () => {
        void actions.today().then((id) => {
          if (id !== undefined) select(id);
        });
      },
    },
    {
      id: "mode-simple",
      label: "Mode: Simple",
      run: () => {
        chooseMode("simple");
      },
    },
    {
      id: "mode-organizer",
      label: "Mode: Organizer",
      run: () => {
        chooseMode("organizer");
      },
    },
    {
      id: "settings",
      label: "Settings",
      run: () => {
        setSettingsOpen(true);
      },
    },
    {
      id: "setup-again",
      label: "Set up again",
      run: () => {
        setSetupAgain(true);
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
    {
      id: "import",
      label: "Import files",
      run: () => {
        setImporting([]);
      },
    },
    {
      id: "theme-system",
      label: "Theme: follow the system",
      run: () => {
        chooseTheme("system");
      },
    },
    {
      id: "theme-light",
      label: "Theme: light",
      run: () => {
        chooseTheme("light");
      },
    },
    {
      id: "theme-dark",
      label: "Theme: dark",
      run: () => {
        chooseTheme("dark");
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
        <IconButton
          label="Import"
          icon={FolderInput}
          onClick={() => {
            setImporting([]);
          }}
        />
        <FilesButton
          onClick={() => {
            setFiles(true);
          }}
        />
        <IconButton
          label="Settings"
          icon={SettingsIcon}
          onClick={() => {
            setSettingsOpen(true);
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
      <ImportDialog
        open={importing !== null}
        paths={importing ?? NO_PATHS}
        parent={selected}
        onClose={() => {
          setImporting(null);
        }}
        onOpen={select}
      />
      <SettingsDialog
        open={settingsOpen}
        onClose={() => {
          setSettingsOpen(false);
        }}
        about={
          about.data
            ? { version: about.data.version, formatVersion: about.data.format_version }
            : null
        }
        links={LINKS}
        onOpenFiles={() => {
          setFiles(true);
        }}
        onSetupAgain={() => {
          setSetupAgain(true);
        }}
        onVaultChanged={() => {
          select(null);
          forgetVault(client);
          void client.invalidateQueries({ queryKey: ["bootstrap"] });
        }}
        pickFolder={isTauri() ? pickFolder : undefined}
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
  const chosenId = useShell((state) => state.views[index]);
  const setView = useShell((state) => state.setView);
  const object = useObject(objectId);
  const base = useId();
  const tabs = useRef<(HTMLButtonElement | null)[]>([]);
  // The mode decides which views are offered (M7); a chosen view the mode hides shows as the
  // first one, and the choice itself is kept for when the mode shows it again.
  const views = visibleViews(useMode());
  const view =
    views.find((candidate) => candidate.id === chosenId) ?? views[0] ?? viewById("document");
  const viewId = view.id;
  const View = view.component;

  const onKeyDown = (event: KeyboardEvent) => {
    const at = views.findIndex((candidate) => candidate.id === viewId);
    const step = event.key === "ArrowRight" ? 1 : event.key === "ArrowLeft" ? -1 : 0;
    let next = at + step;
    if (event.key === "Home") next = 0;
    else if (event.key === "End") next = views.length - 1;
    else if (step === 0) return;
    event.preventDefault();
    next = (next + views.length) % views.length;
    const target = views[next];
    if (!target) return;
    setView(index, target.id);
    tabs.current[next]?.focus();
  };

  return (
    <section className="pane" aria-label={index === 0 ? "Main pane" : "Second pane"}>
      <div className="pane__tabs" role="tablist" aria-label="Views" onKeyDown={onKeyDown}>
        {views.map((candidate, i) => {
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
