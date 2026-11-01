// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * The map from far away (ADR-0019). React Flow costs per node and edge it has mounted, on every
 * frame: at 2,011 nodes on screen it pans at 46 fps and drags at 13. Below reading zoom nothing can
 * be read anyway, so one canvas draws the whole map as plain boxes, and React Flow keeps only the
 * node with the keyboard. Its pan and zoom still drive the view, from underneath this canvas.
 */
import { useStoreApi } from "@xyflow/react";
import { useEffect, useLayoutEffect, useRef } from "react";
import { paintOverview, type OverviewColors, type OverviewScene } from "./paint";

function readColors(element: Element): OverviewColors {
  const style = getComputedStyle(element);
  const token = (name: string) => style.getPropertyValue(name).trim();
  return {
    branch: token("--dbme-color-border"),
    relation: token("--dbme-color-accent"),
    box: token("--dbme-color-surface"),
    outline: token("--dbme-color-fg-muted"),
    active: token("--dbme-color-focus"),
  };
}

export function OverviewCanvas({ scene }: { scene: OverviewScene }) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const latest = useRef(scene);
  const store = useStoreApi();
  const draw = useRef<() => void>(() => undefined);

  useLayoutEffect(() => {
    latest.current = scene;
    draw.current();
  }, [scene]);

  useEffect(() => {
    const element = canvas.current;
    const context = element?.getContext("2d");
    if (!element || !context) return;
    let width = 0;
    let height = 0;
    let shown = -1;
    let colors = readColors(element);
    const paint = () => {
      const ratio = window.devicePixelRatio;
      context.setTransform(ratio, 0, 0, ratio, 0, 0);
      const drawn = paintOverview(
        context,
        latest.current,
        store.getState().transform,
        width,
        height,
        colors,
      );
      // For the tests and the budget: what a canvas draws can't be read back from the page.
      if (drawn !== shown) {
        shown = drawn;
        element.dataset.drawn = String(drawn);
      }
    };
    draw.current = paint;
    const resize = () => {
      const box = element.getBoundingClientRect();
      width = box.width;
      height = box.height;
      element.width = Math.round(width * window.devicePixelRatio);
      element.height = Math.round(height * window.devicePixelRatio);
      colors = readColors(element);
      paint();
    };
    const observer = new ResizeObserver(resize);
    observer.observe(element);
    resize();
    // Pan and zoom don't re-render React: the view's transform comes straight from the store.
    const stop = store.subscribe((state, before) => {
      if (state.transform !== before.transform) paint();
    });
    const scheme = window.matchMedia("(prefers-color-scheme: dark)");
    const recolor = () => {
      colors = readColors(element);
      paint();
    };
    scheme.addEventListener("change", recolor);
    return () => {
      stop();
      observer.disconnect();
      scheme.removeEventListener("change", recolor);
      draw.current = () => undefined;
    };
  }, [store]);

  return <canvas ref={canvas} className="dbme-map__overview" aria-hidden="true" />;
}
