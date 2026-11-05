// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * Keeps a scene and the store's rows the same (M8, Task 12, ADR-0035), with no drawing library in it. The
 * store is the truth, and Undo, Redo and other panes change it, so what it holds replaces the scene. Except
 * that what is being drawn is the person's, so a stroke finishes and is written first, as one change, which
 * is one undo. Everything here is about the order those happen in.
 */
import type { CanvasItemRow_Serialize } from "@databaseme/client";
import {
  afterChanges,
  changesBetween,
  sameScene,
  signaturesOf,
  type CanvasChanges,
  type SceneElement,
} from "./scene";

type Row = CanvasItemRow_Serialize;

/** What this needs of a scene. */
export interface SceneHandle {
  /** Everything in the scene, what was deleted from it too. */
  elements: () => readonly SceneElement[];
  /** Nothing is being drawn, dragged, resized or typed. */
  settled: () => boolean;
  /** Shows these rows in place of what the scene has. */
  show: (rows: readonly Row[]) => void;
}

/** Writes changes as one, and says whether the store took them. */
export type Commit = (changes: CanvasChanges) => Promise<boolean>;

/** How long a scene has to be still before what changed is written. */
const SETTLE_MS = 250;

export class CanvasSync {
  private stored: ReadonlyMap<string, string>;
  private rows: readonly Row[];
  private timer: ReturnType<typeof setTimeout> | null = null;
  private queue: Promise<void> = Promise.resolve();
  private writes = 0;
  /** Rows came while the scene was busy, and it has not taken them yet. */
  private waiting = false;
  private closed = false;

  constructor(
    rows: readonly Row[],
    private readonly scene: SceneHandle,
    private readonly commit: Commit,
    private readonly settleMs: number = SETTLE_MS,
  ) {
    this.rows = rows;
    this.stored = signaturesOf(rows);
  }

  /** The scene says something happened, which may be no more than the pointer moving. */
  sceneChanged(): void {
    const { upsert, remove } = changesBetween(this.stored, this.scene.elements());
    if (upsert.length > 0 || remove.length > 0) this.settleLater();
  }

  /** The store says what it holds. */
  rowsArrived(rows: readonly Row[]): void {
    this.rows = rows;
    if (this.timer !== null || this.writes > 0 || !this.scene.settled()) {
      this.waiting = true;
      if (this.timer === null && this.writes === 0) this.settleLater();
      return;
    }
    this.adopt();
  }

  /** Writes what has changed now, and resolves when all of it is in the store. */
  flush(): Promise<void> {
    this.clearTimer();
    const wrote = this.write();
    if (!wrote && this.waiting) this.settleLater();
    return this.queue;
  }

  /** The scene is going: what has changed is written, and nothing happens after that. */
  close(): Promise<void> {
    const done = this.flush();
    this.clearTimer();
    this.closed = true;
    return done;
  }

  private clearTimer(): void {
    if (this.timer !== null) clearTimeout(this.timer);
    this.timer = null;
  }

  private settleLater(): void {
    if (this.closed) return;
    this.clearTimer();
    this.timer = setTimeout(() => {
      this.timer = null;
      if (!this.scene.settled()) this.settleLater();
      else if (!this.write() && this.waiting && this.writes === 0) this.adopt();
    }, this.settleMs);
  }

  private write(): boolean {
    const changes = changesBetween(this.stored, this.scene.elements());
    if (changes.upsert.length === 0 && changes.remove.length === 0) return false;
    this.stored = afterChanges(this.stored, changes);
    // Any rows still waiting are older than this, and would take it off the screen. A write makes the store
    // be read again, so what it holds afterwards is on its way.
    this.waiting = false;
    this.writes += 1;
    // One at a time, in order: a later one may depend on an earlier one, as the first makes the view.
    this.queue = this.queue
      .then(() => this.commit(changes))
      .then((took) => {
        this.finished(took);
      });
    return true;
  }

  private finished(took: boolean): void {
    this.writes -= 1;
    if (this.closed) return;
    if (!took) {
      // The store has said why, so what it does hold is what shows, and this is not tried again.
      this.stored = signaturesOf(this.rows);
      if (this.scene.settled()) this.adopt();
      else this.waiting = true;
    }
    if (this.waiting) this.settleLater();
  }

  private adopt(): void {
    this.waiting = false;
    this.stored = signaturesOf(this.rows);
    if (!sameScene(this.scene.elements(), this.rows)) this.scene.show(this.rows);
  }
}
