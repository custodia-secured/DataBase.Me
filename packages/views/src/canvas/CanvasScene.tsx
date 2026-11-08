// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * The canvas itself (M8, Task 12, ADR-0035): Excalidraw over the store's rows, kept the same by
 * `CanvasSync`. The app's Undo stands in for Excalidraw's own, which knows nothing of the store, and nothing
 * on a canvas leads out of the app: no link, no library, no export. Loaded only when the picture is asked
 * for, since it is the largest library in the app.
 */
import { useSummary, type CanvasItemRow_Serialize, type Id } from "@databaseme/client";
import { CaptureUpdateAction, Excalidraw, MainMenu, restoreElements } from "@excalidraw/excalidraw";
import "@excalidraw/excalidraw/index.css";
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { registerFlusher } from "../actions";
import { elementOf, isKept, objectOf, type CanvasChanges, type SceneElement } from "./scene";
import { CanvasSync, type SceneHandle } from "./sync";

type Props = Parameters<typeof Excalidraw>[0];
type Api = Parameters<NonNullable<Props["excalidrawAPI"]>>[0];
type AppState = ReturnType<Api["getAppState"]>;

/**
 * Nothing is being drawn, dragged, resized or typed, and the scene has finished starting: until then,
 * Excalidraw would put its starting elements back over anything shown.
 */
function isSettled(state: AppState): boolean {
  return (
    !state.isLoading &&
    state.newElement === null &&
    state.resizingElement === null &&
    state.multiElement === null &&
    state.editingTextElement === null &&
    state.selectionElement === null &&
    !state.selectedElementsAreBeingDragged &&
    !state.isResizing &&
    !state.isRotating
  );
}

function toScene(rows: readonly CanvasItemRow_Serialize[]) {
  return restoreElements(rows.map(elementOf) as never, null, {
    refreshDimensions: false,
    repairBindings: true,
  });
}

function handleOn(api: Api): SceneHandle {
  const elements = () =>
    api.getSceneElementsIncludingDeleted() as unknown as readonly SceneElement[];
  return {
    elements,
    settled: () => isSettled(api.getAppState()),
    show: (rows) => {
      // A picture is not kept, but it stays for as long as the scene does, as the warning says.
      const pictures = elements().filter((element) => !isKept(element) && !element.isDeleted);
      api.updateScene({
        elements: [...toScene(rows), ...(pictures as never[])],
        captureUpdate: CaptureUpdateAction.NEVER,
      });
    },
  };
}

/** What a card shows, drawn by the app over the scene. A click still reaches the scene underneath. */
function Card({ objectId }: { objectId: Id }) {
  const summary = useSummary(objectId);
  const object = summary.data?.object;
  // No object at all, or one with a date in the Trash.
  const gone = summary.isSuccess && object?.deleted_at !== null;
  const inside = summary.data?.child_count ?? 0;
  return (
    <div className="dbme-canvas__card" data-object={objectId}>
      <strong>{gone ? "In the Trash" : object ? object.title || "Untitled" : ""}</strong>
      {!gone && inside > 0 ? (
        <span>
          {inside} {inside === 1 ? "page" : "pages"} inside
        </span>
      ) : null}
    </div>
  );
}

/** Brings an element into view and chooses it, once, if it has arrived. */
function focusIfThere(pending: { current: { api: Api; id: string } | null }): void {
  if (!pending.current) return;
  const { api, id } = pending.current;
  const element = api.getSceneElements().find((candidate) => candidate.id === id);
  if (!element) return;
  pending.current = null;
  api.scrollToContent(element, { fitToContent: false });
  api.updateScene({
    appState: { selectedElementIds: { [id]: true } },
    captureUpdate: CaptureUpdateAction.NEVER,
  });
}

/**
 * Excalidraw focuses a link in Help's header, which is hidden here. That leaves the focus nowhere, and Escape
 * and Tab with nothing to act on, so it goes on the dialog's content, which takes one a frame or two to appear.
 */
function focusHelp(frames = 10): void {
  const content = document.querySelector<HTMLElement>(".excalidraw .HelpDialog .Modal__content");
  if (content) content.focus();
  else if (frames > 0) {
    requestAnimationFrame(() => {
      focusHelp(frames - 1);
    });
  }
}

/** Excalidraw's actions that would go round the app. Registering one by its name replaces it. */
const REPLACED = ["hyperlink", "undo", "redo"].map((name) => ({
  name,
  label: name,
  trackEvent: false,
  // Its key goes on up to the app: Ctrl+K to the palette, Ctrl+Z and Ctrl+Y to the app's Undo and Redo.
  keyTest: () => false,
  perform: () => false,
}));

const UI_OPTIONS: NonNullable<Props["UIOptions"]> = {
  canvasActions: {
    changeViewBackgroundColor: false,
    clearCanvas: true,
    export: false,
    loadScene: false,
    saveAsImage: false,
    saveToActiveFile: false,
    toggleTheme: false,
  },
  tools: { image: false },
};

export interface CanvasSceneProps {
  rows: readonly CanvasItemRow_Serialize[];
  /**
   * When the store was last read. Rows that say the same as before stay the same array, but they are still
   * news: if Undo overtakes the read after a stroke, the rows go from none to none and the scene has the stroke.
   */
  readAt: number;
  dark: boolean;
  /** Brings this element into view and chooses it, once it has arrived. */
  focus: string | null;
  /** The chosen element, if exactly one is chosen. */
  onSelect: (itemId: string | null) => void;
  /** A card's link was followed. */
  onOpen: (id: Id) => void;
  /** A picture was put on the canvas, which does not keep them yet. */
  onPicture: () => void;
  onCommit: (changes: CanvasChanges) => Promise<boolean>;
}

export default function CanvasScene({
  rows,
  readAt,
  dark,
  focus,
  onSelect,
  onOpen,
  onPicture,
  onCommit,
}: CanvasSceneProps) {
  const [api, setApi] = useState<Api | null>(null);
  const sync = useRef<CanvasSync | null>(null);
  const chosen = useRef<string | null>(null);
  /** The element to bring into view and choose, until it is there to be chosen. */
  const pendingFocus = useRef<{ api: Api; id: string } | null>(null);
  const helpIsOpen = useRef(false);
  const callbacks = useRef({ onSelect, onOpen, onPicture, onCommit });
  useEffect(() => {
    callbacks.current = { onSelect, onOpen, onPicture, onCommit };
  });
  const firstRows = useRef(rows);
  // Excalidraw reads its starting scene once. From there on, the rows reach it through `CanvasSync`.
  const [initialData] = useState(() => ({ elements: toScene(rows), scrollToContent: true }));

  // A layout effect, because of how it ends: its cleanup runs before Excalidraw unmounts and empties its scene,
  // while an ordinary effect's runs after, when there is nothing left to save.
  useLayoutEffect(() => {
    if (!api) return;
    const started = new CanvasSync(firstRows.current, handleOn(api), (changes) =>
      callbacks.current.onCommit(changes),
    );
    sync.current = started;
    for (const action of REPLACED) api.registerAction(action as never);
    if (import.meta.env.MODE === "e2e") {
      // What a test on a phone draws with, and only the e2e build has it.
      (window as unknown as { __dbmeCanvas?: Api }).__dbmeCanvas = api;
    }
    // Undo and Redo save what is in hand first, as they do for typing, and so does leaving.
    const unregister = registerFlusher(() => started.flush());
    return () => {
      unregister();
      void started.close();
      sync.current = null;
    };
  }, [api]);

  useEffect(() => {
    if (api) sync.current?.rowsArrived(rows);
  }, [api, rows, readAt]);

  useEffect(() => {
    if (!api || focus === null) return;
    pendingFocus.current = { api, id: focus };
    focusIfThere(pendingFocus);
  }, [api, focus]);

  // Excalidraw renders again whenever one of these is a new function.
  const handlers = useMemo<Partial<Props>>(
    () => ({
      validateEmbeddable: (link) => link.startsWith("dbme:"),
      renderEmbeddable: (element) => {
        const shown = objectOf(element);
        return shown === null ? <div className="dbme-canvas__card" /> : <Card objectId={shown} />;
      },
      onLinkOpen: (element, event) => {
        event.preventDefault();
        const shown = objectOf(element);
        if (shown !== null) callbacks.current.onOpen(shown);
      },
      onPaste: (_data, event) => {
        if ((event?.clipboardData?.files.length ?? 0) === 0) return true;
        callbacks.current.onPicture();
        return false;
      },
      onChange: (elements, state) => {
        const ids = Object.keys(state.selectedElementIds);
        const selected = ids.length === 1 ? (ids[0] ?? null) : null;
        if (selected !== chosen.current) {
          chosen.current = selected;
          callbacks.current.onSelect(selected);
        }
        if (elements.some((element) => element.type === "image" && !element.isDeleted)) {
          callbacks.current.onPicture();
        }
        const help = state.openDialog?.name === "help";
        if (help && !helpIsOpen.current) focusHelp();
        helpIsOpen.current = help;
        sync.current?.sceneChanged();
        // Rows can wait for the scene to settle, so what is to be chosen may arrive only now.
        if (pendingFocus.current) setTimeout(focusIfThere, 0, pendingFocus);
      },
    }),
    [],
  );

  return (
    <div className="dbme-canvas__host">
      <Excalidraw
        excalidrawAPI={setApi}
        initialData={initialData}
        theme={dark ? "dark" : "light"}
        aiEnabled={false}
        UIOptions={UI_OPTIONS}
        {...handlers}
      >
        <MainMenu>
          <MainMenu.DefaultItems.ClearCanvas />
          <MainMenu.DefaultItems.Help />
        </MainMenu>
      </Excalidraw>
    </div>
  );
}
