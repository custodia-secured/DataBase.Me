## What and why

<!-- What does this change, and why? Link the issue or milestone. -->

## Checklist

- [ ] Every commit is signed off (`git commit --signoff`); see CONTRIBUTING.md.
- [ ] Tests cover the change, and `cargo test`, `pnpm test`, `pnpm lint` and `pnpm typecheck` pass.
- [ ] An architectural decision has an ADR in `docs/adr/`.
- [ ] A schema or file-format change has a migration, a `docs/FORMAT.md` update, and updated
      export/import and files-folder tests.
- [ ] No new network request unless a user-enabled feature needs it.
