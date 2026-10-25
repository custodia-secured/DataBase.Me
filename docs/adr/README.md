# Architecture decision records

Every architectural decision in DataBase.Me is written down here, so that anyone can see what was
decided, why, and what else was considered. To propose a decision, copy
[0000-template.md](0000-template.md), take the next number, and open a pull request. An accepted
ADR is never rewritten to say something else: a later ADR supersedes it, and both link to each
other.

| ADR                                             | Title                                                        | Status   |
| ----------------------------------------------- | ------------------------------------------------------------ | -------- |
| [0001](0001-technology-stack.md)                | Technology stack and identifiers                             | Accepted |
| [0002](0002-storage.md)                         | Storage: SQLite vault, blob store and op log                 | Accepted |
| [0003](0003-identifiers-and-ordering.md)        | Identifiers, ordering keys, hashes and timestamps            | Accepted |
| [0004](0004-licensing.md)                       | Licensing                                                    | Accepted |
| [0005](0005-plugin-model.md)                    | Plugin model                                                 | Accepted |
| [0006](0006-two-way-files-folder.md)            | Two-way Markdown files folder                                | Accepted |
| [0007](0007-governance-and-dco.md)              | Governance and the DCO                                       | Accepted |
| [0008](0008-platform-targets-and-budgets.md)    | Platform targets, performance budgets and CI                 | Accepted |
| [0009](0009-dependency-licence-policy.md)       | Dependency licence policy                                    | Accepted |
| [0010](0010-desktop-shell-security-baseline.md) | Desktop shell security baseline                              | Accepted |
| [0012](0012-op-log.md)                          | Op log: payloads, hashed bytes, groups, undo and redo        | Accepted |
| [0013](0013-sort-keys.md)                       | Sort keys: the fractional-index algorithm                    | Accepted |
| [0014](0014-typed-ipc.md)                       | Typed IPC: generated bindings and the change event           | Accepted |
| [0015](0015-sqlite-settings-and-migrations.md)  | SQLite settings, migration policy and the performance budget | Accepted |
