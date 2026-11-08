// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * What Excalidraw needs to work with no network (M8, gate 1d):
 *
 * - Its font loader always adds a fallback URL on a public CDN (`esm.sh`) after the app's own path.
 *   The CSP refuses it, but the attempt is a report per font, and the promise is that the app does
 *   not try. The transform points the fallback at the app's own copy and fails the build if the
 *   library stops having it, so an upgrade cannot quietly bring it back.
 * - The fonts are copied into the build with their licences (third-party/excalidraw-fonts-NOTICE.txt,
 *   ADR-0035). Xiaolai (CJK) is left out: it is 13 MB of the 13.5 MB, and CJK text falls back to the
 *   system's fonts. So is Liberation, which only a server-side export of Excalidraw's asks for.
 *
 * The optional Mermaid importer is not here: `pnpm-workspace.yaml` overrides it with an empty package,
 * so neither it nor what it brings is installed.
 */
import { copyFileSync, cpSync, existsSync } from "node:fs";
import { createRequire } from "node:module";
import path from "node:path";
import type { Plugin } from "vite";

export const EXCALIDRAW_ASSETS = "excalidraw-assets";
const FONTS = ["Assistant", "Cascadia", "ComicShanns", "Excalifont", "Lilita", "Nunito", "Virgil"];
// The literal holds a nested template (the package and its version), so it ends at its known tail.
const CDN = /`https:\/\/esm\.sh\/[\s\S]*?\/dist\/prod\/`/;

export function excalidrawOffline(): Plugin {
  let outDir = "";
  let replaced = false;
  return {
    name: "dbme-excalidraw-offline",
    enforce: "pre",
    configResolved(config) {
      outDir = path.resolve(config.root, config.build.outDir);
    },
    transform(code, id) {
      if (!id.includes("@excalidraw") || !code.includes("ASSETS_FALLBACK_URL")) return null;
      if (!CDN.test(code)) return null;
      replaced = true;
      return {
        code: code.replace(CDN, `\`\${location.origin}/${EXCALIDRAW_ASSETS}/\``),
        map: null,
      };
    },
    buildEnd(error) {
      if (!error && !replaced) {
        const loaded = [...this.getModuleIds()].some((id) => id.includes("@excalidraw/excalidraw"));
        if (loaded) this.error("Excalidraw's font fallback was not found: read this plugin again");
      }
    },
    closeBundle() {
      // Resolved from the package that depends on it, since pnpm does not hoist it here.
      const views = createRequire(path.resolve(import.meta.dirname, "../../packages/views/x.js"));
      const entry = views.resolve("@excalidraw/excalidraw");
      const from = path.resolve(path.dirname(entry), "fonts");
      if (!existsSync(from)) this.error(`no Excalidraw fonts at ${from}`);
      for (const font of FONTS) {
        cpSync(path.join(from, font), path.join(outDir, EXCALIDRAW_ASSETS, "fonts", font), {
          recursive: true,
        });
      }
      copyFileSync(
        path.resolve(import.meta.dirname, "third-party/excalidraw-fonts-NOTICE.txt"),
        path.join(outDir, EXCALIDRAW_ASSETS, "NOTICE.txt"),
      );
    },
  };
}
