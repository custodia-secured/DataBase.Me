# ADR-0029: The release

- **Status:** Proposed
- **Date:** 2026-11-03
- **Milestone:** M7

## Context

The MVP's acceptance (brief §8) names signed installers and "docs + FORMAT.md v0.1 published".
ADR-0001 said the application identifier must be final before the first release, and the build
plan (§4.1) set the targets: Windows (MSI and NSIS), macOS (Apple Silicon and Intel) and Linux
(AppImage and deb). ADR-0008 put the three systems in CI from M0 and left installers to this
milestone. Signing certificates cost money and are the owner's to buy; the release must work with
and without them, and say which it was.

## Decision

**One tag, one workflow, one draft release.** `.github/workflows/release.yml` runs on a `v*` tag
(and by hand, for a rehearsal that uploads artifacts and makes no release). Four jobs build with
`tauri build` on the project's runners: `windows-x64` (MSI and NSIS), `macos-arm64` and
`macos-x64` (DMG), `linux-x64` on Ubuntu 22.04 (AppImage, deb and rpm; the oldest supported
WebKitGTK is the one the runner has). Each job computes SHA-256 sums of what it built and uploads
the installers and the sums as an artifact. On a tag, a last job gathers them, joins the sums into
one `SHA256SUMS.txt`, takes the release notes from the version's section of `CHANGELOG.md`, and
creates a **draft** GitHub Release with everything attached. Publishing the draft is a person's
act, after the by-hand checks (the network check on Windows and macOS, ADR-0028; the installers
opened once each).

**Signing when the secrets exist, said plainly when they do not.** Tauri signs when its signing
variables are set, and an empty variable counts as set (gate 1e: macOS failed in `codesign` with an
empty identity), so the workflow exports the variables only when the corresponding secrets exist:
Authenticode on Windows (`TAURI_WINDOWS_SIGNTOOL_CERTIFICATE`), Developer ID and notarization on
macOS (the certificate imported into a temporary keychain, the identity, Apple ID, app password and
team). Each job prints whether it signed. Linux artifacts are not signed in 0.1. Whether to buy the
certificates is the owner's decision (M7 Decision 6); 0.1 may ship unsigned, with the user guide
saying what each operating system will warn and how to verify the checksums instead.

**The version lives in two files and one test keeps them equal:** the workspace `Cargo.toml`
(`workspace.package.version`, which every crate and the Tauri shell inherit) and
`apps/desktop/package.json`. The tag is `v` and that version. The identifier
`org.databaseme.desktop` (`tauri.conf.json`, ADR-0001) becomes final with the 0.1 tag; changing
it after would orphan every installed copy's configuration folder.

**The documentation site is GitHub Pages rendering `docs/` as it is.** `docs/_config.yml` names
the site and the theme; `.github/workflows/pages.yml` builds `docs/` with GitHub's own Jekyll
action on a push to `main` and deploys it. No generator is added to the repository: the Markdown
that is read in the repository is the Markdown that is published, and relative links between
pages keep working in both places. The site's front page is `docs/index.md`; the user guide is
`docs/user-guide/`; FORMAT.md, ARCHITECTURE.md, the ADRs and the milestone notes are published
with it. Turning Pages on for the repository, with "GitHub Actions" as its source, is the
owner's step (M7 Decision 5); the address Help prints is the one Pages gives.

**FORMAT.md v0.1 ships with 0.1.** Its status line says so, every section carries Decided or
Draft with its ADR or milestone, and `dbme-store` writes this text into every vault it makes
(`FORMAT_FILE`, by `include_str!`), so the copy in a vault is the copy that was published.

## Consequences

- A release is reproducible from the tag: the same workflow, the same runners, the same inputs.
  The artifacts' sums are published with them, so an installer downloaded later can be checked
  against the release's `SHA256SUMS.txt`.
- The macOS build without a Developer ID produces an app Gatekeeper refuses by default; the user
  guide gives the right-click-Open step. Windows SmartScreen warns about an unsigned installer
  until the certificate's reputation builds. Both are the cost of Decision 6 until it is taken.
- The CHANGELOG is part of the release: a version with no section ships with "See CHANGELOG.md"
  as its notes, which is a defect to fix before publishing the draft.
- The Pages site rebuilds on every push to `main` that touches `docs/`; a broken relative link is
  broken in the repository too, where review catches it.
- An automatic updater, when it comes, needs its own ADR: a signing key for update manifests, the
  opt-in setting honoured, and the network test extended (ADR-0028).

## Alternatives considered

- **Publishing the release directly from the workflow.** One step fewer, and no moment for the
  by-hand checks the privacy promise asks for. A draft costs one click.
- **Signing Linux artifacts with a GPG key.** Possible, and nothing on Linux checks it by default;
  the checksums cover the integrity case. Revisited with a package repository.
- **A documentation generator (mdBook, Docusaurus, MkDocs).** Better navigation and search, at the
  cost of a toolchain, a theme and a build to keep working. The documents are few and are read in
  the repository as often as on the site; the built-in renderer is enough for 0.1.
- **Universal macOS binaries.** One DMG instead of two; the build is slower and the artifact
  larger for everyone. Two targets, each the size of one, until there is a reason.
- **An MSIX or a store listing.** Later, with a certificate and a publisher account.
