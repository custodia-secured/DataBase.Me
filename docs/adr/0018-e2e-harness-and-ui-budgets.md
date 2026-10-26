# ADR-0018: The end-to-end harness and the UI budgets

- **Status:** Accepted
- **Date:** 2026-10-25
- **Milestone:** M2

## Context

M2's acceptance needs every view tested end to end:

- at desktop size and at 360 px with touch;
- by keyboard alone;
- across views, for live consistency;
- against two budgets: a 100,000-row table at 60 fps, and typing under 16 ms (ADR-0008).

The obvious tool, tauri-driver, drives the real app over WebDriver. It has no macOS support,
because there is no WKWebView driver. It needs a native driver matched to each platform's
webview. It cannot emulate a phone's viewport or touch, and it gives no per-test vault isolation.
Pure JavaScript mocks of the commands would be fast, but they would test a fake: the vault's
rules, the op log and undo would never run.

## Decision

- **One command layer, two front doors.** Every vault command is a plain function in `dbme-api`,
  over a `Session`. The Tauri shell wraps each one in a one-line command (ADR-0014). The test-only
  `dbme-e2e-server` calls the same functions over HTTP on real vaults. A desktop-crate test checks
  that every command in `build.rs` has an arm in the server's dispatcher.
- **The server isolates tests.**
  - Each test names a session, `t-<uuid>`, and gets its own folder and vault. Tests therefore run
    in parallel.
  - `POST /seed` builds fixtures through the same store services: a small typed tree with blocks
    and image covers, a long document, or 100,000 objects in bulk.
  - A per-session lock makes each reply carry exactly the `vault-changed` events of its own
    command.
  - The server listens on 127.0.0.1 only and is never shipped.
- **The e2e build swaps only the transport.** `vite build --mode e2e` writes `dist-e2e`. Its entry
  point installs `packages/client/src/e2e.ts`, which uses Tauri's IPC and event mocks to send
  commands to the server with `fetch` and to deliver the returned events where the shell's would
  arrive. Everything above the transport is the production code. The e2e build also freezes
  `Object.prototype`, as `freezePrototype` does in the app.
- **CI proves the transport absent from production.** `scripts/check-e2e-transport.mjs` checks the
  production build for the transport and fails if it is there. It also checks the e2e build, to
  prove the marker still finds the transport.
- **The real CSP is enforced.** The server sends the CSP from `tauri.conf.json`, with `connect-src`
  pointed at itself. A test fails if the page reports any of the following:
  - a `securitypolicyviolation`;
  - an uncaught error;
  - a console error;
  - an HTTP status of 400 or more, except `/favicon.ico`.

  One exception is narrow and documented in the fixture. Playwright's WebKit reports a
  `style-src-attr` violation against any `<select>`, from the engine's own styling of the control,
  with no sample. That exact report is excused on WebKit and nowhere else.

- **Playwright with stand-in engines.**
  - Microsoft Edge, through Playwright's `msedge` channel, stands in for WebView2, which is built
    on Edge.
  - Playwright's WebKit stands in, approximately, for WKWebView and WebKitGTK.
  - There are four functional projects: each engine at 1280×800, and at 360×740 with `isMobile` and
    `hasTouch`.
  - Presses are taps on the phone projects. Drag-and-drop by touch uses CDP touch events on Edge.
    Playwright cannot send touch drags to WebKit, so "Move to…" is the touch path tested there.
  - axe checks WCAG 2.2 A and AA on every view, the mention list and the split pane.
- **The UI budgets are measured in the browser and enforced on Edge desktop.**
  - They are the `*-budget.spec.ts` files, in the projects `edge-budget` and `webkit-budget` at
    1280×800. Those projects run after the functional tests, one test at a time, so nothing else
    competes for the machine while they measure.
  - **Table scroll.** Seed 100,000 objects and open the Table. Jump to the middle and time until
    its rows are read. Then scroll by `requestAnimationFrame`: 2,000 px/s for 5 s (reading) and
    20,000 px/s for 3 s (flinging), recording every frame's interval and every frame that shows a
    placeholder row. The pass mark for "60 fps" is a median frame of at most 1.1 × 16.67 ms, and no
    more than 2% of frames over 25 ms (one and a half frames).
  - **Typing.** Seed a 1,000-paragraph page, put the caret at the end, and type 225 characters at
    40 ms intervals. For each key, measure from the `keydown` event's timestamp to the first task
    after the next animation frame, which is when the frame showing the key is done. Edge runs
    with its frame-rate cap and vsync off, so the measure is the key's own cost and not the wait
    for a display refresh. The pass mark is a 95th percentile under 16 ms, with one sample per key.
  - Results are written to `target/budgets/<project>-<budget>.json` and attached to the report.
    WebKit's numbers are recorded, not enforced: its Windows build is a test port, not the
    WKWebView the app ships on.
- **CI runs the suite on Windows**, as on the reference machine. A Windows runner has Edge
  installed; Playwright installs its own WebKit. The report and traces of a failed run, and the
  budget results of every run, are kept as artefacts.

## Consequences

- The end-to-end tests run the real Rust command code, store, op log and undo, under the real CSP,
  in parallel.
- What is not covered is the Tauri shell's own wiring: the IPC bridge, window setup and the
  capability file. The launch test on all three operating systems covers that (ADR-0008, ADR-0014).
- The engines are stand-ins. Real WKWebView on macOS and iOS, WebKitGTK on Linux, and phone
  keyboards and IME composition are not exercised until the mobile milestone (M14). The WebKit
  `<select>` report must be checked again there, in a real WKWebView.
- The frame-interval method gives no breakdown of where a slow frame went. When a budget fails, a
  performance trace is the next tool; the plan's trace step was left out because the budgets pass
  with a wide margin.
- On a display faster than 60 Hz, frames pace at the display's rate: the reference machine's
  panel runs at 240 Hz, and its table frames take 4.2 ms. The pass marks are frame times, so they
  hold at any refresh of 60 Hz or more.
- A budget does not run when a functional test fails, because its project depends on theirs. Use
  `--no-deps` to measure anyway.
- Shared CI runners are noisy. If the budgets flake there, they get a margin and an ungated
  companion, as the store's budget did (ADR-0015).

## Alternatives considered

- **tauri-driver (WebDriver).** It tests the real shell, but has no macOS driver, no phone
  emulation and no touch. It needs a native driver per platform, and it is slow to start per test.
  It may still serve a few smoke tests of the shell later.
- **JavaScript mocks of every command.** Fast, but the vault's rules would never run, and the mocks
  would drift from Rust.
- **Playwright's bundled Chromium instead of Edge.** Nearly the same engine, but Edge is the one
  WebView2 ships, at the same version cadence.
- **A Rust or WebDriver BiDi harness written for this project.** More to own, and still no phone
  or touch emulation.
