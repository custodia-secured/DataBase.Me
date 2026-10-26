// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * What every e2e test gets (ADR-0018): a session of its own on the e2e server, so tests run in
 * parallel on separate vaults; a way to seed it; and the `App` driver. After each test the page
 * must have had no Content-Security-Policy violation and no uncaught error.
 */
import AxeBuilder from "@axe-core/playwright";
import { test as base, expect, type Locator, type Page } from "@playwright/test";
import { randomUUID } from "node:crypto";

declare global {
  interface Window {
    /** CSP violations seen by the page, recorded by an init script. */
    __dbmeViolations?: Violation[];
  }
}

interface Violation {
  directive: string;
  blocked: string;
  source: string;
  /** The element the violation names, if any. */
  target: string;
  sample: string;
}

/**
 * Playwright's WebKit (27.2) reports its own `<select>`: the engine styles the control's inner
 * parts with a style attribute, and its CSP check counts that against the page. Any `<select>`
 * does it, with no style of ours on it, and no other control does. The report names the select
 * and carries no sample; anything else, from a select or not, still fails the test (ADR-0018).
 */
function isWebKitSelectQuirk(violation: Violation, browserName: string): boolean {
  return (
    browserName === "webkit" &&
    violation.directive === "style-src-attr" &&
    violation.target === "select" &&
    violation.sample === ""
  );
}

/** What a `tree` seed returns: the IDs of its objects by title, and of its types by name. */
export interface TreeIds {
  ids: Record<string, string>;
  types: Record<string, string>;
}

/** Drives the app the way a person would: by touch on a phone, with the mouse on a desktop. */
export class App {
  constructor(
    readonly page: Page,
    readonly session: string,
    readonly phone: boolean,
  ) {}

  /** Loads the app and waits until it shows a page or offers to make the first one. */
  async open(): Promise<void> {
    await this.page.goto(`/?session=${this.session}`);
    await expect(
      this.page
        .getByRole("tablist", { name: "Views" })
        .first()
        .or(this.page.getByRole("heading", { name: "Nothing here yet" })),
    ).toBeVisible();
  }

  /**
   * Calls a command on this test's vault, to check what the UI saved. The server sends a
   * command's changes only with its own reply, so this never takes the UI's events.
   */
  async ipc<T>(cmd: string, args: Record<string, unknown> = {}): Promise<T> {
    const response = await this.page.request.post("/ipc", {
      data: { session: this.session, cmd, args },
    });
    const reply = (await response.json()) as { ok?: T; error?: unknown };
    if (reply.error !== undefined) throw new Error(`${cmd}: ${JSON.stringify(reply.error)}`);
    return reply.ok as T;
  }

  /** Clicks, or taps on a touch screen. */
  async press(target: Locator): Promise<void> {
    if (this.phone) await target.tap();
    else await target.click();
  }

  /** The sidebar. On a phone it lives in a drawer, which this opens by touch or keyboard. */
  async sidebar(by: "pointer" | "keyboard" = "pointer"): Promise<Locator> {
    if (this.phone) {
      const drawer = this.page.getByRole("dialog", { name: "Pages" });
      if (!(await drawer.isVisible())) {
        const button = this.page.getByRole("button", { name: "Pages", exact: true });
        if (by === "keyboard") {
          await button.focus();
          await this.page.keyboard.press("Enter");
        } else {
          await this.press(button);
        }
      }
      await expect(drawer).toBeVisible();
      return drawer.getByRole("navigation", { name: "Pages" });
    }
    return this.page.getByRole("navigation", { name: "Pages" });
  }

  /** The Pages tree, opening the drawer on a phone. */
  async tree(): Promise<Locator> {
    return (await this.sidebar()).getByRole("tree", { name: "Pages" });
  }

  /** A row of the Pages tree. */
  async treeItem(title: string): Promise<Locator> {
    return (await this.tree()).getByRole("treeitem", { name: title, exact: true });
  }

  /** Opens a page from the tree, expanding the pages above it: `openPage("Home", "Beta")`. */
  async openPage(...path: string[]): Promise<void> {
    for (const [index, title] of path.entries()) {
      const item = await this.treeItem(title);
      if (index === path.length - 1) {
        await this.press(item);
      } else if ((await item.getAttribute("aria-expanded")) !== "true") {
        await this.press(item.locator(".dbme-tree__toggle"));
        await expect(item).toHaveAttribute("aria-expanded", "true");
      }
    }
    await expect(this.currentCrumb()).toHaveText(path.at(-1) ?? "");
  }

  /** Every name in the breadcrumbs, top first; a phone shows only the last of them. */
  path(): Locator {
    return this.page.getByRole("navigation", { name: "Path" }).locator("li");
  }

  /** The open page's name in the breadcrumbs. */
  currentCrumb(): Locator {
    return this.page.getByRole("navigation", { name: "Path" }).locator("[aria-current=page]");
  }

  /** One of the panes: the first, or the second in a split. */
  pane(index: 0 | 1 = 0): Locator {
    return this.page.getByRole("region", { name: index === 0 ? "Main pane" : "Second pane" });
  }

  /** Switches a pane to a view: "Document", "Outline", "Table" or "Gallery". */
  async view(name: string, pane: 0 | 1 = 0): Promise<void> {
    const tab = this.pane(pane).getByRole("tab", { name });
    await this.press(tab);
    await expect(tab).toHaveAttribute("aria-selected", "true");
  }

  /** The Document view's title field in a pane. */
  title(pane: 0 | 1 = 0): Locator {
    return this.pane(pane).getByRole("textbox", { name: "Title", exact: true });
  }

  /** The Document view's text editor in a pane. */
  editor(pane: 0 | 1 = 0): Locator {
    return this.pane(pane).locator(".dbme-editor");
  }

  /**
   * Focuses the Document view's text and waits for the caret: ProseMirror puts its selection in
   * the page a moment after focus, and keys pressed before then have no caret to move.
   */
  async focusEditor(pane: 0 | 1 = 0): Promise<void> {
    const editor = this.editor(pane);
    await editor.focus();
    await expect
      .poll(() =>
        editor.evaluate((element) => {
          const anchor = document.getSelection()?.anchorNode;
          return anchor != null && element.contains(anchor);
        }),
      )
      .toBe(true);
  }

  /**
   * The name of the item a one-tab-stop widget (tree, grid) names as active with
   * `aria-activedescendant`: its `aria-label`, or else its text without the parts hidden from
   * assistive technology. Null when it names none. Use with `expect.poll`.
   */
  async activeText(widget: Locator): Promise<string | null> {
    const id = await widget.getAttribute("aria-activedescendant");
    if (id === null) return null;
    return this.page.locator(`[id="${id}"]`).evaluate((item) => {
      const label = item.getAttribute("aria-label");
      if (label !== null) return label;
      const copy = item.cloneNode(true) as Element;
      for (const hidden of copy.querySelectorAll("[aria-hidden=true]")) hidden.remove();
      return copy.textContent.trim();
    });
  }
}

interface Fixtures {
  session: string;
  seed: <T = TreeIds>(seed: unknown) => Promise<T>;
  app: App;
}

export const test = base.extend<Fixtures>({
  // eslint-disable-next-line no-empty-pattern -- Playwright reads what a fixture needs from this pattern; this one needs nothing
  session: async ({}, use) => {
    await use(`t-${randomUUID()}`);
  },

  seed: async ({ request, session }, use) => {
    await use(async <T>(seed: unknown): Promise<T> => {
      const response = await request.post("/seed", { data: { session, seed }, timeout: 300_000 });
      expect(response.ok(), await response.text()).toBe(true);
      return (await response.json()) as T;
    });
  },

  page: async ({ page, browserName }, use) => {
    await page.addInitScript(() => {
      window.__dbmeViolations = [];
      document.addEventListener("securitypolicyviolation", (event) => {
        window.__dbmeViolations?.push({
          directive: event.violatedDirective,
          blocked: event.blockedURI || "inline",
          source: `${event.sourceFile}:${String(event.lineNumber)}`,
          target: event.target instanceof Element ? event.target.localName : "",
          sample: event.sample,
        });
      });
    });
    const problems: string[] = [];
    page.on("pageerror", (error) => {
      problems.push(`uncaught: ${error.message}`);
    });
    page.on("response", (response) => {
      // The server answers failed commands with 200 and an error; a 4xx or 5xx is a fault. The
      // app has no favicon, which browsers ask for anyway.
      const url = new URL(response.url());
      if (response.status() >= 400 && url.pathname !== "/favicon.ico") {
        problems.push(`HTTP ${String(response.status())} for ${url.pathname}`);
      }
    });
    page.on("console", (message) => {
      // Failed loads are reported above, with their URL, and CSP refusals below, with the element.
      const text = message.text();
      if (
        message.type() === "error" &&
        !text.startsWith("Failed to load resource") &&
        !text.startsWith("Refused to ")
      ) {
        problems.push(`console.error: ${text}`);
      }
    });
    await use(page);
    const violations = await page
      .evaluate(() => window.__dbmeViolations ?? [])
      .catch(() => [] as Violation[]);
    expect(
      violations.filter((violation) => !isWebKitSelectQuirk(violation, browserName)),
      "Content-Security-Policy violations",
    ).toEqual([]);
    expect(problems, "errors in the page").toEqual([]);
  },

  app: async ({ page, session, isMobile }, use) => {
    await use(new App(page, session, isMobile));
  },
});

export { expect };

/** Runs axe (WCAG 2.2 A and AA) on the page, or on part of it, and expects no violations. */
export async function expectAccessible(page: Page, include?: string): Promise<void> {
  let builder = new AxeBuilder({ page }).withTags([
    "wcag2a",
    "wcag2aa",
    "wcag21a",
    "wcag21aa",
    "wcag22aa",
  ]);
  if (include !== undefined) builder = builder.include(include);
  const results = await builder.analyze();
  const found = results.violations.map(
    (violation) =>
      `${violation.id}: ${violation.help} — ${violation.nodes.map((node) => node.target.join(" ")).join(", ")}`,
  );
  expect(found, "axe violations").toEqual([]);
}
