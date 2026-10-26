// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * Keeps an editor and an object's stored blocks in step (ADR-0016).
 *
 * - Typing is saved in batches: 400 ms after the last change, at most 2 s after the first, on
 *   blur, and before undo or redo. Each batch is one `blocks_apply` call, so one undo step.
 * - The editor remembers the blocks it last saved or loaded (the snapshot). A batch is the diff from
 *   the snapshot to the document.
 * - When the stored blocks may have changed, it reads them afresh. Its own echo matches the
 *   snapshot and is ignored. Anything else was changed elsewhere (another view, undo): unsaved
 *   typing is saved first, then the editor shows what the vault holds.
 * - If a batch fails because the vault changed underneath, it is diffed again against what the
 *   vault holds and sent once more. The editor's document wins; what it overwrote stays in the
 *   vault's history, so undo brings it back. If that fails too, the editor reloads and reports it.
 */
import type { BlockChange_Deserialize } from "@databaseme/client";
import type { JSONContent } from "@tiptap/core";
import { diffBlocks, jsonEqual } from "./diff";
import { blocksToDoc, docToBlocks, rowsToTree, type BlockRowLike, type BlockTree } from "./mapping";

/** The editor, as the sync sees it. */
export interface EditorPort {
  getJSON: () => JSONContent;
  /** Replaces the document without counting as a change, keeping the selection where it can. */
  show: (doc: JSONContent) => void;
}

/** The vault, as the sync sees it. */
export interface VaultPort {
  load: () => Promise<readonly BlockRowLike[]>;
  apply: (changes: BlockChange_Deserialize[]) => Promise<void>;
}

export const IDLE_MS = 400;
export const MAX_WAIT_MS = 2000;

/** The blocks the editor would show for stored `rows`, as the editor would save them. */
export function canonical(rows: readonly BlockRowLike[]): BlockTree[] {
  const tree = rowsToTree(rows);
  return tree.length === 0 ? [] : docToBlocks(blocksToDoc(tree));
}

export class EditorSync {
  private snapshot: BlockTree[];
  private timer: ReturnType<typeof setTimeout> | null = null;
  private firstChange: number | null = null;
  private queue: Promise<void> = Promise.resolve();
  private disposed = false;

  constructor(
    private readonly editor: EditorPort,
    private readonly vault: VaultPort,
    rows: readonly BlockRowLike[],
    private readonly onError: (error: unknown) => void,
    private readonly now: () => number = Date.now,
  ) {
    this.snapshot = canonical(rows);
  }

  /** The editor's document changed. */
  changed(): void {
    if (this.disposed) return;
    const now = this.now();
    this.firstChange ??= now;
    if (this.timer !== null) clearTimeout(this.timer);
    const wait = Math.max(0, Math.min(IDLE_MS, this.firstChange + MAX_WAIT_MS - now));
    this.timer = setTimeout(() => void this.flush(), wait);
  }

  /** Saves unsaved changes now. Resolves when they are saved, or failed and were reported. */
  flush(): Promise<void> {
    return this.enqueue(() => this.save());
  }

  /** The stored blocks may have changed: brings the editor up to date. */
  reconcile(): Promise<void> {
    return this.enqueue(() => this.catchUp());
  }

  /** Saves what is left and stops. */
  dispose(): Promise<void> {
    const last = this.flush();
    this.disposed = true;
    return last;
  }

  private enqueue(task: () => Promise<void>): Promise<void> {
    const next = this.queue.then(task).catch(this.onError);
    this.queue = next;
    return next;
  }

  private clearTimer(): void {
    if (this.timer !== null) clearTimeout(this.timer);
    this.timer = null;
    this.firstChange = null;
  }

  private async save(): Promise<void> {
    this.clearTimer();
    const after = docToBlocks(this.editor.getJSON());
    const changes = diffBlocks(this.snapshot, after);
    if (changes.length === 0) return;
    try {
      await this.vault.apply(changes);
    } catch {
      try {
        const retry = diffBlocks(canonical(await this.vault.load()), after);
        if (retry.length > 0) await this.vault.apply(retry);
      } catch (error: unknown) {
        await this.reload();
        throw error;
      }
    }
    this.snapshot = after;
  }

  private async reload(): Promise<void> {
    const rows = await this.vault.load();
    this.snapshot = canonical(rows);
    if (!this.disposed) this.editor.show(blocksToDoc(rowsToTree(rows)));
  }

  private async catchUp(): Promise<void> {
    for (;;) {
      const rows = await this.vault.load();
      const stored = canonical(rows);
      if (jsonEqual(stored, this.snapshot)) return;
      if (this.disposed) return;
      const local = docToBlocks(this.editor.getJSON());
      if (jsonEqual(local, this.snapshot)) {
        this.snapshot = stored;
        this.editor.show(blocksToDoc(rowsToTree(rows)));
        return;
      }
      // Unsaved typing: save it on top of the other change, then look again.
      await this.save();
    }
  }
}
