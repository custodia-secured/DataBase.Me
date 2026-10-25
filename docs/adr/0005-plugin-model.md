# ADR-0005: Plugin model

- **Status:** Accepted
- **Date:** 2026-10-25
- **Milestone:** M0 (implemented in M10; mobile runtime checked before M14)

## Context

Obsidian's plugins run with full access to the machine, and in 2026 one was abused to deliver a
remote-access trojan. VS Code's extension host has the same permissions as the editor. Logseq's
iframe sandbox was escaped (CVE-2026-47901) because its Content Security Policy was disabled
(build plan §2.8). A lifetime archive needs plugins that cannot take more than the user gave them.

## Decision

- **Logic plugins are WebAssembly modules** run by Extism on Wasmtime, behind a `PluginRuntime`
  trait in `crates/plugins`. A WebAssembly module has no ambient authority: it can only call the
  functions the host links in.
- **A manifest declares every capability**: which spaces and types it may read or write, which
  network hosts it may contact, which paths it may see, and its memory and time limits. The user
  approves them in a phone-style prompt and can revoke them.
- **The host always passes an explicit `allowed_hosts` list.** Extism treats an empty list as "no
  network" but a missing (null) list as "every host", so the host never passes null. `allowed_paths`
  maps only to scoped folders. Memory and timeout limits are always set.
- **Host functions are the only way out**: scoped `objects.query/get/create/update`, `relations.*`,
  scoped `blobs.read`, allow-listed `net.fetch`, and event subscriptions.
- **UI plugins run in sandboxed iframes** (`sandbox="allow-scripts"`, never `allow-same-origin`)
  with a strict CSP, and talk to the app only through a typed `postMessage` RPC that the host
  validates. They never see the app's DOM, and CSP is never disabled.
- Importers, views, commands and formula functions are plugin kinds with the same contracts the
  built-in ones use.
- Plugins are never updated without consent, and a kill switch disables one immediately.
- **iOS forbids JIT compilation.** Before M14 we must confirm an interpreter backend (Wasmtime's
  Pulley or wasmi) behind the same trait, or record what mobile plugins can and cannot do.

## Consequences

- Plugins are slower to write than in-process JavaScript, so the SDK (Apache-2.0) and samples must
  be good, and capabilities must be generous enough to be useful.
- The security tests in M10 must show that a plugin cannot reach the network, files or objects
  outside its grant, and that a UI plugin cannot reach the host DOM.

## Alternatives considered

- **JavaScript in the app process (Obsidian).** Easiest to write, impossible to sandbox.
- **A separate Node or extension-host process (VS Code).** Isolates crashes, not permissions.
- **Deno isolates.** Real permissions, but a large runtime to embed, and no iOS story.
- **The WebAssembly component model (WASI preview 2).** Promising, and Extism or a successor may
  adopt it; we revisit at M10.
