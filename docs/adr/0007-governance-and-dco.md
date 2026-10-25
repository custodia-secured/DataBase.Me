# ADR-0007: Governance and the DCO

- **Status:** Accepted
- **Date:** 2026-10-25
- **Milestone:** M0

## Context

The build plan (§8, decision 9) fixes the governance model: a BDFL now and a steering group once
there are three or more regular maintainers, with contributions under a Developer Certificate of
Origin rather than a Contributor License Agreement. A CLA mainly lets a project relicense or sell
licences; with donation funding and no paid tiers we need neither, and a DCO is much friendlier to
contributors.

## Decision

- [GOVERNANCE.md](../../GOVERNANCE.md) sets out the BDFL model and the commitment to a steering
  group, including what "regular maintainer" means.
- Every commit must carry `Signed-off-by: Name <email>` certifying DCO 1.1, and the sign-off's
  email must match the commit author's email (case-insensitive). Merge commits are not checked;
  the commits they merge are.
- The check is our own script, `scripts/dco-check.mjs`, run by the `dco` job in CI on every pull
  request and push. It is tested against a real temporary repository with signed and unsigned
  commits (`scripts/dco-check.test.mjs`). Contributors can run it locally (`pnpm dco <range>`).
- Once the repository is on GitHub, branch protection on `main` must require the `DCO sign-off`
  check. That is a repository setting, not code, and is listed in the M0 report as a manual step.

## Consequences

- No legal paperwork for contributors; one flag (`git commit -s`) per commit.
- The project can never relicense without every contributor's consent. That is intended.
- An in-repo check works the same on forks and mirrors and has no third-party app to trust.

## Alternatives considered

- **A CLA.** Needed only to relicense or sell licences, neither of which is planned.
- **The DCO GitHub App.** Works well, but is an outside dependency; it can be added alongside the
  in-repo check if wanted.
