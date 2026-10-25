# Contributing to DataBase.Me

Thank you for helping. This page covers how to report problems, how to sign off your work, how to
set up a development environment, and what a change needs before it can be merged.

## Reporting a bug

Open a GitHub issue with the **Bug report** template. DataBase.Me has no telemetry and no crash
reporter, so the issue is how we find out. From version 0.1, use **Help → Copy diagnostic report**
in the app and paste the result into the issue: it contains versions, your OS and recent error
codes, and never your notes or files. Read it before you paste it.

Security problems are different: report them privately as [SECURITY.md](SECURITY.md) explains.

## Developer Certificate of Origin (DCO)

DataBase.Me uses the [Developer Certificate of Origin](https://developercertificate.org) instead of
a Contributor License Agreement. By adding a `Signed-off-by:` line to a commit, you certify the
following:

```
Developer Certificate of Origin
Version 1.1

Copyright (C) 2004, 2006 The Linux Foundation and its contributors.

Everyone is permitted to copy and distribute verbatim copies of this
license document, but changing it is not allowed.


Developer's Certificate of Origin 1.1

By making a contribution to this project, I certify that:

(a) The contribution was created in whole or in part by me and I
    have the right to submit it under the open source license
    indicated in the file; or

(b) The contribution is based upon previous work that, to the best
    of my knowledge, is covered under an appropriate open source
    license and I have the right under that license to submit that
    work with modifications, whether created in whole or in part
    by me, under the same open source license (unless I am
    permitted to submit under a different license), as indicated
    in the file; or

(c) The contribution was provided directly to me by some other
    person who certified (a), (b) or (c) and I have not modified
    it.

(d) I understand and agree that this project and the contribution
    are public and that a record of the contribution (including all
    personal information I submit with it, including my sign-off) is
    maintained indefinitely and may be redistributed consistent with
    this project or the open source license(s) involved.
```

Sign off with `git commit --signoff` (or `-s`). The line must use the same email address as the
commit author:

```
Signed-off-by: Your Name <you@example.org>
```

A CI check rejects pull requests with unsigned commits. To fix them:

```sh
git commit --amend --signoff --no-edit   # the last commit
git rebase --signoff main                # every commit on your branch
git push --force-with-lease
```

You can run the same check locally: `pnpm dco origin/main..HEAD`.

Your contribution is licensed under the licence of the files you change: AGPL-3.0-or-later for most
of the repository, Apache-2.0 for `packages/sdk/` and `docs/FORMAT.md`. Nobody, including the
project, gets the right to relicense your work.

## Development setup

See "Build from source" in the [README](README.md). Then, before you open a pull request, run what
CI runs:

```sh
pnpm lint                 # ESLint
pnpm format:check         # Prettier (pnpm format fixes it)
pnpm typecheck            # TypeScript, strict mode
pnpm test                 # Vitest and the scripts' tests
pnpm licenses:js          # JS dependency licences
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check          # Rust dependency licences and advisories (cargo install cargo-deny)
```

## What a change needs

- **Tests.** Unit tests for logic; property tests (proptest) for the op log, query DSL, export
  round trips and files-folder merging; end-to-end tests for views at desktop and phone width.
- **Coding standards.**
  - Rust: stable toolchain, rustfmt, clippy with `-D warnings`, no `unwrap()` or `expect()`
    outside tests, `thiserror`/`anyhow` for errors, `tracing` for logs, and all SQL inside
    `crates/store` as prepared statements.
  - TypeScript: strict mode, ESLint and Prettier, no `any` unless an `eslint-disable` comment
    explains why, small pure components, and IPC types generated from Rust.
  - Every public API has doc comments. New source files start with an SPDX licence line.
- **An ADR for every architectural decision.** Copy `docs/adr/0000-template.md`, take the next
  number, and link it from `docs/adr/README.md`.
- **Schema and format changes** need a migration, an update to `docs/FORMAT.md`, and updated
  export/import and files-folder tests.
- **No new network traffic** unless a feature the user turns on needs it.
- **New dependencies** must pass the licence policy in
  [ADR-0009](docs/adr/0009-dependency-licence-policy.md).
- **Small, reviewable pull requests**, and `main` stays green.

## Conduct

Everyone taking part follows the [Code of Conduct](CODE_OF_CONDUCT.md). How decisions are made is
described in [GOVERNANCE.md](GOVERNANCE.md).
