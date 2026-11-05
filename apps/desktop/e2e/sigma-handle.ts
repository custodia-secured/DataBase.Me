// SPDX-License-Identifier: AGPL-3.0-or-later
/** What the Graph's tests read of Sigma: only the e2e build hands it over (GraphCanvas). */
export interface SigmaHandle {
  getGraph(): {
    order: number;
    size: number;
    getNodeAttribute(id: string, name: string): unknown;
    nodes(): string[];
  };
  getNodeDisplayData(id: string): { x: number; y: number } | undefined;
  framedGraphToViewport(at: { x: number; y: number }): { x: number; y: number };
  getCamera(): { getState(): { x: number; y: number; ratio: number } };
  getWebGLContext(): WebGL2RenderingContext;
  refresh(): void;
}

declare global {
  interface Window {
    __dbmeSigma?: SigmaHandle;
  }
}
