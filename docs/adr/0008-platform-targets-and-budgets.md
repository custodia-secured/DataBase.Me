# ADR-0008: Platform targets, performance budgets and CI

- **Status:** Accepted
- **Date:** 2026-10-25
- **Milestone:** M0

## Context

The build plan (§8, decision 10) fixes the hardware floor and the budgets every milestone measures
against. The app must also become a full mobile app at M14 without a rewrite, so mobile constraints
apply from the start.

## Decision

**Platforms.**

| Tier                                                     | Target                                                                                                                                                                                                                       |
| -------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Desktop minimum (the reference machine for every budget) | 64-bit 4-core CPU (x86-64 or ARM64, about 2018 or newer), 8 GB RAM, SSD. Windows 10 22H2 or 11; the three most recent major macOS versions on Intel and Apple Silicon; Linux with WebKitGTK 4.1 (Ubuntu 22.04+, Debian 12+). |
| Desktop recommended                                      | 16 GB RAM.                                                                                                                                                                                                                   |
| Local AI (optional)                                      | 16 GB RAM for 3–8B 4-bit models; 32 GB+ unified memory or 8 GB+ VRAM recommended; smaller machines can use a model server on the user's own network.                                                                         |
| Mobile (M14)                                             | Current and two previous major iOS versions; Android 10+; 4 GB RAM.                                                                                                                                                          |

**Budgets**, enforced by benchmarks in CI (criterion for Rust from M1, Playwright traces for the UI
from M2):

| What                               | Budget                                                   |
| ---------------------------------- | -------------------------------------------------------- |
| Cold start to usable, 100k objects | < 2 s                                                    |
| Typing latency                     | < 16 ms                                                  |
| Keyword search                     | < 50 ms at 100k objects; < 300 ms at 1M                  |
| Idle memory                        | < 300 MB                                                 |
| Table scroll                       | 100k rows at 60 fps                                      |
| Mind map                           | 2,000 nodes interactive                                  |
| Graph view                         | 100k nodes (WebGL)                                       |
| Files folder round trip            | < 2 s each way                                           |
| Lifetime scale                     | 1M objects and 1 TB of blobs work (slower, never broken) |

**CI matrix.** Every push and pull request builds, tests and launches the app on `ubuntu-22.04`
(the oldest supported Linux, so WebKitGTK 4.1 is really tested), `windows-latest` and
`macos-latest` (Apple Silicon). Intel macOS and Windows 10 are covered by the release builds in M7.

**Launch test.** CI proves the app launches, not just that it compiles. The release binary starts
with `DBME_SMOKE_TEST=1`; the web UI calls the `app_ready` command after its first render; the app
prints `DBME_SMOKE_OK startup_ms=<n>` and exits with status 0. `scripts/smoke-launch.mjs` fails the
job on a crash, a non-zero exit or a 120-second timeout. On Linux it runs under a virtual display
(`xvfb-run`). The startup time it prints is the first data point for the cold-start budget.

**Mobile from day one.**

- From M1, the core crates cross-compile in CI for `aarch64-apple-ios` and
  `aarch64-linux-android`.
- From M2, every view is tested at 360 px width with touch input.
- Desktop-only pieces (sidecars, file watching) sit behind traits with mobile fallbacks or a
  "process on my desktop" queue.

## Consequences

- GitHub's hosted runners are not the reference machine. Budgets are checked there with a safety
  margin, and numbers are tracked over time; a regression fails CI even when the absolute budget
  still passes on a fast runner.
- The launch test needs no WebDriver and runs in seconds, so it stays in every CI run.

## Alternatives considered

- **Build-only CI.** Cheaper, but it would not catch a webview that fails to start on one OS.
- **WebDriver (tauri-driver) for the launch test.** More capable, and it arrives with the
  end-to-end tests in M2, but its macOS support has historically been missing (there is no
  WKWebView driver), so the smoke mode stays as the launch check that covers all three OSes.
