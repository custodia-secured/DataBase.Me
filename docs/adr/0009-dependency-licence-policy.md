# ADR-0009: Dependency licence policy

- **Status:** Accepted
- **Date:** 2026-10-25
- **Milestone:** M0

## Context

The brief requires every dependency licence to be verified in CI, allowing MIT, Apache, BSD, ISC,
MPL and Zlib and flagging anything else. Real dependency trees contain a few other permissive
licences, so the policy has to say exactly which identifiers count and what happens to the rest.

## Decision

**Allowed everywhere** (SPDX identifiers): `MIT`, `MIT-0`, `Apache-2.0`, `BSD-2-Clause`,
`BSD-3-Clause`, `0BSD`, `ISC`, `MPL-2.0`, `Zlib`. An `OR` expression passes when either side is
allowed; `AND` needs both; `X WITH <exception>` counts as `X`.

**Additional licences, each flagged and justified here:**

| Licence         | Where                 | Why it is acceptable                                                                                                                |
| --------------- | --------------------- | ----------------------------------------------------------------------------------------------------------------------------------- |
| `Unicode-3.0`   | Rust, shipped         | The Unicode data tables in `unicode-ident` and ICU crates, which nearly every Rust program depends on. Permissive and OSI-approved. |
| `BlueOak-1.0.0` | JS build tooling only | `lru-cache`, `minimatch`. Permissive and OSI-approved.                                                                              |
| `CC-BY-4.0`     | JS build tooling only | `caniuse-lite` browser-support data, read at build time and not shipped in the app.                                                 |
| `CC0-1.0`       | JS build tooling only | `mdn-data`, a public-domain dedication.                                                                                             |

**How it is enforced.**

- Rust: `cargo deny check licenses bans sources` in CI with [`deny.toml`](../../deny.toml).
  cargo-deny needs each `X WITH <exception>` pair listed explicitly; at M0 that is
  `Apache-2.0 WITH LLVM-exception` (`cfg-expr`, a Linux build dependency).
- Advisories are checked too, in a job that reports without blocking unrelated pull requests. An
  advisory may be ignored only with a written reason in `deny.toml`. At M0 one is:
  RUSTSEC-2024-0370 (`proc-macro-error` unmaintained, not a vulnerability), a build-time dependency
  of the GTK3 bindings Tauri 2 needs on Linux.
- JS: `scripts/check-js-licenses.mjs` reads `pnpm licenses list` and applies
  [`scripts/license-policy.json`](../../scripts/license-policy.json). Runtime dependencies must use
  an allowed licence; dev tooling may also use the dev-only licences above. Anything the app imports
  at runtime must therefore be declared in `dependencies`, never `devDependencies`.
- Our own crates are AGPL-3.0-or-later and unpublished, so cargo-deny skips them.

**Adding a licence.** A pull request that needs a new licence adds it to `deny.toml` or
`license-policy.json`, adds a row to the table above with the reason, and is reviewed like any
architectural decision. Strong copyleft licences (GPL, LGPL, AGPL) in shipped code, and licences
that are not OSI-approved, need their own ADR.

## Consequences

- CI fails the moment a dependency with an unlisted licence arrives, including transitively.
- The JS check depends on the licence field each package declares; a wrong field is caught only by
  review.

## Alternatives considered

- **Allow only the brief's six families, no exceptions.** Not possible: Tauri itself depends on
  `unicode-ident`.
- **A third-party licence service.** Adds an external dependency and sends our dependency list to
  someone else for no gain.
