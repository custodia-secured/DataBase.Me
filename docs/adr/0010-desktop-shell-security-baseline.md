# ADR-0010: Desktop shell security baseline

- **Status:** Accepted
- **Date:** 2026-10-25
- **Milestone:** M0

## Context

The web UI runs in a webview with a bridge into Rust code that can read every byte of the vault.
Anything that runs script in that webview, whether a bug in a rendering library, a malicious
imported file shown in a view, or a future UI plugin, must not be able to reach that bridge or the
network. Tauri 2's defaults are reasonable, but the baseline should be explicit and stricter.

## Decision

- **Strict Content Security Policy in production** (`app.security.csp` in `tauri.conf.json`):
  `default-src 'self'`, scripts and styles only from the app bundle, images only from the bundle,
  `data:` and `blob:`, IPC as the only `connect-src`, and `object-src`, `base-uri`, `form-action`
  and `frame-ancestors` set to `'none'`. Tauri adds hashes for its own scripts. The development
  CSP (`devCsp`) relaxes only what Vite's hot reload needs.
- **No remote content.** The UI is bundled with the app; no CDN, no remote fonts, no remote images.
  This is also what makes "zero network requests by default" possible (tested in M7).
- **App commands are deny-by-default.** `build.rs` lists every app command in an app manifest, so
  each command gets an `allow-<command>` permission, and a window may call a command only if one of
  its capabilities grants it. At M0 the main window holds exactly `allow-app-ready` and no core
  plugin permissions.
- `freezePrototype` is on, so script injected into the page cannot tamper with built-in prototypes
  to subvert the IPC layer, and `withGlobalTauri` is off.
- No updater plugin until opt-in update checks exist (M7).
- UI plugin iframes (M10) get no capabilities at all; they reach the app only through the host's
  validated message channel (ADR-0005).

## Consequences

- Every new command needs an entry in `build.rs` and a grant in a capability file, a small cost
  that makes each new piece of attack surface visible in review.
- Some libraries that inject inline styles or use `eval` will not work under this CSP. That is a
  reason to reject them, not to loosen the policy. Relaxing the CSP needs an ADR.
- Whether the webview itself makes network requests (for example, WebView2's SmartScreen checks,
  which Tauri is documented to disable by default) is verified by the M7 network test, not assumed.

## Alternatives considered

- **Tauri defaults only.** Commands are allowed for every window by default, and there is no CSP
  until one is configured.
- **Disabling CSP in development.** Easier, but development and production would then behave
  differently in exactly the area that matters.
