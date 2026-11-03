# ADR-0028: Onboarding and the network guarantee

- **Status:** Proposed
- **Date:** 2026-11-03
- **Milestone:** M7

## Context

The brief's acceptance for the MVP (§8) has two lines that meet in the first launch: onboarding
asks about the files-folder location and about opt-in update checks, and the app makes "ZERO
network requests on first launch with defaults (network test)". ADR-0010 set the baseline that
makes the second possible (the interface is bundled, the Content Security Policy names only the
bridge, no updater until opt-in checks exist) and said in so many words that whether the webview
itself makes requests is verified by this milestone's test, not assumed.

A test of that kind has to watch the process, not the page: a webview can reach out for its own
reasons (safe-browsing lists, font services, telemetry) with no line of ours asking it to. The
three webviews differ in how they can be watched. WebKitGTK on Linux is an ordinary process tree
that `strace` follows. WebView2 on Windows and WKWebView on macOS run their network code in helper
processes the operating system owns, and a shared build machine gives no way to trace them that
is both allowed and faithful.

Onboarding itself must keep the promise: nothing it asks, and nothing it does with the answers, may
touch the network. The update-check question is the sharp edge, because the honest answer to "what
happens if I turn this on" is a request.

## Decision

**Onboarding** runs when the settings file says it has not run and no vault is open (ADR-0027
holds the settings). Its steps are: welcome; where the vault goes (the default folder, or one
picked with the native dialog, `tauri-plugin-dialog`, admitted at gate 1a, or a typed path); the
files folder (on in its default place, off, or a folder); the mode (Simple highlighted); the
theme; update checks. Every answer is written to the settings file or to the vault's own files
folder configuration; none leaves the machine. "Set up again" in Settings reopens it with the
current answers.

**Update checks are a recorded choice in 0.1.** The step says plainly that this version has no
updater, that turning the check on in a version that has one would send the app's version and the
person's address to the download host, and that the choice is kept for the first version that
does. The default is off and the off choice is highlighted. No code in 0.1 reads the setting for
anything but showing it.

**The guarantee is tested where it can be, and checked by hand where it cannot.**

- _In CI, blocking, on Linux._ The desktop job builds the release app and launches it through the
  smoke test under `strace -f -e trace=network` on a virtual display, with a fresh configuration
  folder so that the defaults apply. `scripts/network-probe.mjs` reads the trace and lists every
  `connect`, `sendto` and `sendmsg` with an internet address. Loopback addresses and Unix sockets
  are the app talking to the machine it runs on and pass; any other address fails the job. The
  trace is kept as the `network-trace` artifact, so the record is the list, not only the verdict.
  The gate's first reading (M7 "Gate results", 1b) was zero internet socket calls.
- _In the end-to-end harness, on every test._ `fixtures.ts` records every request the page makes
  and fails the test if one went to an origin other than the e2e server's, which stands in for the
  bridge. This catches a stray `fetch` or a remote asset in the interface on the four browser
  projects, the day it is written.
- _By hand, once per release, on Windows and macOS._ `docs/user-guide/privacy.md` gives the
  procedure with the operating system's own tools (Resource Monitor and `Get-NetTCPConnection` or
  Packet Monitor on Windows; `nettop` or `tcpdump` on macOS), and the result is written into the
  release notes. The same page tells a person how to run every check on their own machine, so the
  promise does not rest on this repository's word.

**What the tests do not prove, said plainly.** The Linux trace proves what WebKitGTK does on a
build machine with no network features turned on; it does not prove what WebView2 does on a given
Windows installation, where the operating system's policies can add their own traffic to the
webview's helper processes. That is why the by-hand check is per release and per platform, and
why the user guide teaches it rather than asserting it.

## Consequences

- A feature that needs the network (the updater, the AI features of M11) must be opt-in, off by
  default, explained in the setting that turns it on, and tested on its own; the default launch
  must still pass the probe unchanged. Turning any of them on by default needs a new ADR.
- The CI job gains about a minute on Linux (a second launch, under `strace`). The probe runs after
  the plain launch so that a crash is reported as a crash and not as a trace with no sockets.
- Release notes carry a "Network check" line for Windows and macOS with the date and the tool
  used. A release without it is incomplete.
- The onboarding step about update checks will be reworded when the updater ships; the setting's
  key and meaning stay.

## Alternatives considered

- **Trusting the CSP and the bundle alone.** The CSP governs the page, not the webview's own
  processes. ADR-0010 said this had to be verified; this ADR is the verification.
- **A network namespace with no route, failing on any packet.** Cleaner in principle, but the app
  under test then cannot tell a refused connection from a missing network, and the record would be
  a count of failures rather than a list of addresses. The trace lists what was attempted, which
  is the useful artifact when something does appear.
- **A proxy (`HTTPS_PROXY`) that logs requests.** Only traffic that honours the proxy variables is
  seen; a webview helper that ignores them is exactly what the test is for.
- **Tracing Windows and macOS in CI.** Event Tracing for Windows and `nettop` can be scripted,
  but on a shared runner neither distinguishes the app's helper processes from the machine's own
  background traffic reliably, and a flaky gate on a privacy promise is worse than an honest
  by-hand check. Revisited when a self-hosted runner exists.
- **Asking the AI question in onboarding now.** The brief orders it; the build plan puts it in M11
  with the feature, so that the question is asked when the answer does something.
