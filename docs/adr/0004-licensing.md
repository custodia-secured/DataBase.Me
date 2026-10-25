# ADR-0004: Licensing

- **Status:** Accepted
- **Date:** 2026-10-25
- **Milestone:** M0

## Context

DataBase.Me is funded by donations, with no paid tiers and no hosted service (build plan §8,
decisions 1 and 6). The risk to such a project is a closed fork, especially a hosted one, that
takes the work without giving back. At the same time, the file format and the plugin interface
should be as easy as possible for anyone to adopt, including commercial tools, because readers and
plugins in other hands make users' data safer.

## Decision

- **The application is licensed under the GNU Affero General Public License, version 3 or (at your
  option) any later version (`AGPL-3.0-or-later`).** The brief says "AGPL-3.0"; we read that as the
  "or later" form, which the FSF recommends, so the project can benefit from a future AGPL version
  without asking every contributor. Switching to `AGPL-3.0-only` later would be easy now and hard
  after outside contributions arrive, so this is flagged for the project lead to confirm.
- **The plugin SDK (`packages/sdk/`), the file-format specification (`docs/FORMAT.md`) and future
  client or reader libraries are licensed under the Apache License 2.0.**
- The repository root `LICENSE` holds the AGPL text and `packages/sdk/LICENSE` holds the Apache
  text. Each source file starts with an SPDX line (`// SPDX-License-Identifier: ...`) so mixed
  licensing stays unambiguous.
- AGPL section 13 means that anyone who lets people use a modified version over a network (the
  later server mode, the sync relay) must offer them its source. Our own server mode will show a
  link to the source.
- The project's position is that plugins, which reach the app only through the Apache-licensed SDK
  across a WebAssembly or iframe boundary, are separate works that may use any licence. This is a
  project policy, not legal advice, and should be reviewed by counsel before 1.0.
- What third-party licences we may depend on is ADR-0009.

## Consequences

- Hosted closed forks are not possible without publishing their changes.
- Some companies avoid AGPL code. They can still build plugins and readers under Apache-2.0.
- Contributions are inbound=outbound under the DCO (ADR-0007). With no CLA, relicensing the app
  would need every contributor's consent; that is intended.

## Alternatives considered

- **MIT or Apache-2.0 for everything.** Maximum adoption, but nothing stops a closed hosted fork.
- **MPL-2.0.** File-level copyleft only, and nothing for network use.
- **GPL-3.0.** Copyleft, but no network clause, so a hosted fork could stay closed.
- **AGPL-3.0-only.** Gives more control over future versions; kept as an option for the lead.
