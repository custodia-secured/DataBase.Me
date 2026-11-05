// SPDX-License-Identifier: AGPL-3.0-or-later
import type { CanvasItemRow_Serialize } from "@databaseme/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { itemOf, type CanvasChanges, type SceneElement } from "./scene";
import { CanvasSync, type SceneHandle } from "./sync";

function element(id: string, version = 1): SceneElement {
  return {
    id,
    type: "rectangle",
    x: 0,
    y: 0,
    width: 10,
    height: 10,
    index: "a1",
    version,
    versionNonce: version * 7,
  };
}

function row(from: SceneElement): CanvasItemRow_Serialize {
  const item = itemOf(from);
  return { ...item, object_id: null, id: from.id, view_id: "view" } as CanvasItemRow_Serialize;
}

/** A scene that holds what it is told, and a store that takes what it is given when it is told to. */
function setUp(start: readonly SceneElement[] = []) {
  const scene = {
    elements: [...start],
    settled: true,
    shown: 0,
  };
  const handle: SceneHandle = {
    elements: () => scene.elements,
    settled: () => scene.settled,
    show: (rows) => {
      scene.shown += 1;
      scene.elements = rows.map((one) => element(one.item_id, versionOf(one)));
    },
  };
  const commits: CanvasChanges[] = [];
  let answer: (took: boolean) => void = () => undefined;
  const commit = vi.fn(
    (changes: CanvasChanges) =>
      new Promise<boolean>((resolve) => {
        commits.push(changes);
        answer = resolve;
      }),
  );
  const sync = new CanvasSync(start.map(row), handle, commit, 100);
  return {
    scene,
    sync,
    commit,
    commits,
    answer: (took = true) => {
      answer(took);
    },
  };
}

const versionOf = (one: CanvasItemRow_Serialize) =>
  (one.style_json as unknown as SceneElement).version;

/** Lets the store's answer come through. */
const settle = () => vi.advanceTimersByTimeAsync(0);

beforeEach(() => {
  vi.useFakeTimers();
});
afterEach(() => {
  vi.useRealTimers();
});

describe("what the scene changes", () => {
  it("is written once it has been still, as one change", async () => {
    const { scene, sync, commit, commits } = setUp();
    scene.elements = [element("a")];
    sync.sceneChanged();
    await vi.advanceTimersByTimeAsync(50);
    scene.elements = [element("a", 2), element("b")];
    sync.sceneChanged();
    await vi.advanceTimersByTimeAsync(99);
    expect(commit).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(1);
    expect(commit).toHaveBeenCalledOnce();
    expect(commits[0]?.upsert.map((item) => item.item_id)).toEqual(["a", "b"]);
  });

  it("waits for the stroke to end, however long it takes", async () => {
    const { scene, sync, commit } = setUp();
    scene.settled = false;
    scene.elements = [element("a")];
    sync.sceneChanged();
    await vi.advanceTimersByTimeAsync(1000);
    expect(commit).not.toHaveBeenCalled();
    scene.settled = true;
    await vi.advanceTimersByTimeAsync(100);
    expect(commit).toHaveBeenCalledOnce();
  });

  it("writes nothing for what only looks like a change", async () => {
    const { sync, commit } = setUp([element("a")]);
    sync.sceneChanged();
    await vi.advanceTimersByTimeAsync(1000);
    expect(commit).not.toHaveBeenCalled();
  });

  it("is not written twice", async () => {
    const { scene, sync, commit, answer } = setUp();
    scene.elements = [element("a")];
    sync.sceneChanged();
    await vi.advanceTimersByTimeAsync(100);
    answer();
    await settle();
    sync.sceneChanged();
    await vi.advanceTimersByTimeAsync(1000);
    expect(commit).toHaveBeenCalledOnce();
  });

  it("goes one write at a time, in order", async () => {
    const { scene, sync, commit, answer } = setUp();
    scene.elements = [element("a")];
    sync.sceneChanged();
    await vi.advanceTimersByTimeAsync(100);
    expect(commit).toHaveBeenCalledOnce();
    scene.elements = [element("a"), element("b")];
    sync.sceneChanged();
    await vi.advanceTimersByTimeAsync(100);
    expect(commit).toHaveBeenCalledOnce();
    answer();
    await settle();
    expect(commit).toHaveBeenCalledTimes(2);
  });
});

describe("flush", () => {
  it("writes now, and resolves when the store has it", async () => {
    const { scene, sync, commit, answer } = setUp();
    scene.elements = [element("a")];
    sync.sceneChanged();
    let done = false;
    void sync.flush().then(() => {
      done = true;
    });
    await settle();
    expect(commit).toHaveBeenCalledOnce();
    expect(done).toBe(false);
    answer();
    await settle();
    expect(done).toBe(true);
    await vi.advanceTimersByTimeAsync(1000);
    expect(commit).toHaveBeenCalledOnce();
  });

  it("leaves the store alone when the scene has gone empty", async () => {
    const { scene, sync, commit } = setUp([element("a"), element("b")]);
    scene.elements = [];
    await sync.close();
    expect(commit).not.toHaveBeenCalled();
  });

  it("does nothing more once the scene has closed", async () => {
    const { scene, sync, commit, answer } = setUp();
    scene.elements = [element("a")];
    void sync.close();
    await settle();
    expect(commit).toHaveBeenCalledOnce();
    answer(false);
    await settle();
    await vi.advanceTimersByTimeAsync(1000);
    expect(scene.shown).toBe(0);
    expect(commit).toHaveBeenCalledOnce();
  });
});

describe("what the store holds", () => {
  it("replaces the scene when it differs, as an undo does", () => {
    const { scene, sync, commit } = setUp([element("a")]);
    sync.rowsArrived([]);
    expect(scene.elements).toEqual([]);
    expect(scene.shown).toBe(1);
    expect(commit).not.toHaveBeenCalled();
  });

  it("leaves a scene that already shows it alone", () => {
    const { scene, sync } = setUp([element("a")]);
    sync.rowsArrived([row(element("a"))]);
    expect(scene.shown).toBe(0);
  });

  it("does not write back what it has just shown", async () => {
    const { scene, sync, commit } = setUp([element("a")]);
    sync.rowsArrived([row(element("a", 5)), row(element("b"))]);
    expect(scene.shown).toBe(1);
    sync.sceneChanged();
    await vi.advanceTimersByTimeAsync(1000);
    expect(commit).not.toHaveBeenCalled();
  });

  it("waits for a stroke to end, then shows what came, with no change to write", async () => {
    const { scene, sync, commit } = setUp([element("a")]);
    scene.settled = false;
    sync.rowsArrived([row(element("a", 3))]);
    expect(scene.shown).toBe(0);
    await vi.advanceTimersByTimeAsync(1000);
    expect(scene.shown).toBe(0);
    scene.settled = true;
    await vi.advanceTimersByTimeAsync(100);
    expect(scene.shown).toBe(1);
    expect(commit).not.toHaveBeenCalled();
  });

  it("does not take a stroke away: it is written first, then the rows are shown", async () => {
    const { scene, sync, commit, commits, answer } = setUp([element("a")]);
    scene.elements = [element("a"), element("stroke")];
    sync.sceneChanged();
    sync.rowsArrived([row(element("a"))]);
    expect(scene.shown).toBe(0);
    await vi.advanceTimersByTimeAsync(100);
    expect(commit).toHaveBeenCalledOnce();
    expect(commits[0]?.upsert.map((item) => item.item_id)).toEqual(["stroke"]);
    // What the store holds after the write arrives while it is still answering.
    sync.rowsArrived([row(element("a")), row(element("stroke"))]);
    answer();
    await settle();
    await vi.advanceTimersByTimeAsync(100);
    expect(scene.shown).toBe(0);
    expect(scene.elements.map((one) => one.id)).toEqual(["a", "stroke"]);
  });

  it("drops rows older than a stroke, which would take it off the screen", async () => {
    const { scene, sync, commit, answer } = setUp();
    scene.settled = false;
    sync.rowsArrived([]);
    scene.elements = [element("stroke")];
    sync.sceneChanged();
    scene.settled = true;
    await vi.advanceTimersByTimeAsync(100);
    expect(commit).toHaveBeenCalledOnce();
    answer();
    await settle();
    await vi.advanceTimersByTimeAsync(1000);
    expect(scene.shown).toBe(0);
    expect(scene.elements.map((one) => one.id)).toEqual(["stroke"]);
    sync.rowsArrived([row(element("stroke"))]);
    expect(scene.shown).toBe(0);
  });

  it("takes rows that say the same as before, as they may not be what the scene shows", async () => {
    const { scene, sync, answer } = setUp();
    const rows: CanvasItemRow_Serialize[] = [];
    scene.elements = [element("stroke")];
    void sync.flush();
    await settle();
    answer();
    await settle();
    // The read that would have shown the stroke was overtaken by Undo's, so the rows go from none to none.
    sync.rowsArrived(rows);
    expect(scene.elements).toEqual([]);
  });

  it("shows what came during a write once that write is done", async () => {
    const { scene, sync, answer } = setUp();
    scene.elements = [element("a")];
    void sync.flush();
    await settle();
    sync.rowsArrived([row(element("a")), row(element("elsewhere"))]);
    expect(scene.shown).toBe(0);
    answer();
    await settle();
    await vi.advanceTimersByTimeAsync(100);
    expect(scene.shown).toBe(1);
    expect(scene.elements.map((one) => one.id)).toEqual(["a", "elsewhere"]);
  });
});

describe("what the store refuses", () => {
  it("goes from the scene, and is not tried again", async () => {
    const { scene, sync, commit, answer } = setUp([element("a")]);
    scene.elements = [element("a"), element("bad")];
    sync.sceneChanged();
    await vi.advanceTimersByTimeAsync(100);
    answer(false);
    await settle();
    expect(scene.elements.map((one) => one.id)).toEqual(["a"]);
    sync.sceneChanged();
    await vi.advanceTimersByTimeAsync(1000);
    expect(commit).toHaveBeenCalledOnce();
  });

  it("waits for a stroke in hand, and is tried once more with it", async () => {
    const { scene, sync, commit, answer } = setUp([element("a")]);
    scene.elements = [element("a"), element("bad")];
    sync.sceneChanged();
    await vi.advanceTimersByTimeAsync(100);
    scene.settled = false;
    answer(false);
    await settle();
    expect(scene.shown).toBe(0);
    scene.settled = true;
    await vi.advanceTimersByTimeAsync(100);
    expect(commit).toHaveBeenCalledTimes(2);
    answer(false);
    await settle();
    expect(scene.elements.map((one) => one.id)).toEqual(["a"]);
    await vi.advanceTimersByTimeAsync(1000);
    expect(commit).toHaveBeenCalledTimes(2);
  });
});
