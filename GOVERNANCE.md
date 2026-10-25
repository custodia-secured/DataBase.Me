# Governance

DataBase.Me is a small, donation-funded open-source project. This document says who decides what,
and commits the project to sharing that power as soon as there are enough people to share it with.

## Today: a benevolent dictator

While the project is small, the **project lead** (currently the founder) has the final say on
direction, scope, architecture and releases. "Benevolent dictator for life" (BDFL) is the usual
name for this; it only means somebody can break a tie when consensus fails.

Day to day, decisions are made in the open:

- Most decisions happen in issues and pull requests by **lazy consensus**: a proposal goes ahead
  if nobody with a stake objects within a reasonable time.
- **Architectural decisions** are written down as ADRs in `docs/adr/`, reviewed as pull requests,
  and merged only when accepted.
- When consensus fails, the project lead decides and records why.

## Maintainers

Maintainers review and merge pull requests and triage issues. Anyone who has made sustained,
high-quality contributions (code, review, documentation, design, testing or support) can be
nominated by an existing maintainer. The project lead confirms nominations until the steering group
exists; after that, the steering group does. Maintainers who are inactive for a year move to
emeritus status and can return at any time.

## Tomorrow: a steering group

**Commitment:** once there are **three or more regular maintainers**, the project forms a steering
group and the project lead's sole final say ends.

- A _regular maintainer_ is a maintainer who has reviewed or authored merged pull requests in at
  least six of the last twelve months.
- The steering group has three to five members, drawn from the regular maintainers, and the project
  lead is one of them.
- It decides by majority vote when lazy consensus fails, owns this document, the roadmap and
  releases, and confirms new maintainers.
- The transition, and every later change to this document, is made by pull request and recorded in
  the changelog.

## Licences and contributions

- The app is AGPL-3.0-or-later; the plugin SDK and the file-format specification are Apache-2.0
  ([ADR-0004](docs/adr/0004-licensing.md)).
- Contributions use the Developer Certificate of Origin sign-off, not a CLA
  ([ADR-0007](docs/adr/0007-governance-and-dco.md)). Contributors keep their copyright, and the
  project does not plan to relicense.

## Money

DataBase.Me is funded by donations only. There are no paid tiers, no hosted service and no
investors. Donations will go through a platform with a public ledger (GitHub Sponsors and/or Open
Collective; still to be chosen), and spending will be reported publicly.

## Contact

Before the first public release, the project will publish a private contact address here for Code
of Conduct reports and governance questions. Security reports go through the private channel in
[SECURITY.md](SECURITY.md).
