# ADR-0011: Funding through a Stripe payment link

- **Status:** Accepted
- **Date:** 2026-10-25
- **Milestone:** M0

## Context

The build plan (§8) funds DataBase.Me by voluntary payments only: no paid tiers, no hosted service,
no telemetry. It named GitHub Sponsors and Open Collective as likely platforms but left the choice
open, so at M0 `.github/FUNDING.yml` listed nothing.

On 2026-10-25 the maintainer chose Stripe, with the account of Belmaix Holdings LLC. The money goes
to a company, not a registered charity, so the wording matters: "donation" can suggest a charity
and tax relief.

## Decision

- One Stripe Payment Link, "Support DataBase.Me": the payer chooses the amount (suggested 10 USD,
  minimum 2 USD), pays once, and gives no postal address.
- It is linked from `.github/FUNDING.yml` (`custom`, which drives GitHub's Sponsor button) and from
  the README.
- The project calls these payments **support**, not donations, and says plainly that they are not
  tax-deductible and buy nothing: no features, no priority, no say over the roadmap.
- The principle is unchanged: no paid tiers, no hosted service, no investors.

## Consequences

- Anyone can pay without an account on a funding platform, and the maintainer receives the money
  directly.
- Stripe keeps no public ledger, so GOVERNANCE.md no longer promises one. The build plan's
  mitigation for the sustainability risk ("transparent finances") is therefore not met by this
  choice alone; whether to publish yearly totals is open.
- Stripe's fees apply to every payment, which is why the minimum is 2 USD.
- The payment link lives in the Belmaix Holdings LLC Stripe account; changing the amount or the
  wording happens there, and a new link means updating `FUNDING.yml` and the README.

## Alternatives considered

- **GitHub Sponsors.** Built into GitHub. Can be added to `FUNDING.yml` alongside Stripe later.
- **Open Collective.** A public ledger and a fiscal host, at the cost of the host's fee. Also
  addable later; it is the option that would restore public finances.
- **Calling it "Donate".** Familiar, but misleading for payments to a company.
