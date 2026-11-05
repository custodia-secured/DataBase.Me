# Architecture decision records

Every architectural decision in DataBase.Me is written down here, so that anyone can see what was
decided, why, and what else was considered. To propose a decision, copy
[0000-template.md](0000-template.md), take the next number, and open a pull request. An accepted
ADR is never rewritten to say something else: a later ADR supersedes it, and both link to each
other.

| ADR                                                  | Title                                                        | Status   |
| ---------------------------------------------------- | ------------------------------------------------------------ | -------- |
| [0001](0001-technology-stack.md)                     | Technology stack and identifiers                             | Accepted |
| [0002](0002-storage.md)                              | Storage: SQLite vault, blob store and op log                 | Accepted |
| [0003](0003-identifiers-and-ordering.md)             | Identifiers, ordering keys, hashes and timestamps            | Accepted |
| [0004](0004-licensing.md)                            | Licensing                                                    | Accepted |
| [0005](0005-plugin-model.md)                         | Plugin model                                                 | Accepted |
| [0006](0006-two-way-files-folder.md)                 | Two-way Markdown files folder                                | Accepted |
| [0007](0007-governance-and-dco.md)                   | Governance and the DCO                                       | Accepted |
| [0008](0008-platform-targets-and-budgets.md)         | Platform targets, performance budgets and CI                 | Accepted |
| [0009](0009-dependency-licence-policy.md)            | Dependency licence policy                                    | Accepted |
| [0010](0010-desktop-shell-security-baseline.md)      | Desktop shell security baseline                              | Accepted |
| [0011](0011-funding-through-stripe.md)               | Funding through a Stripe payment link                        | Accepted |
| [0012](0012-op-log.md)                               | Op log: payloads, hashed bytes, groups, undo and redo        | Accepted |
| [0013](0013-sort-keys.md)                            | Sort keys: the fractional-index algorithm                    | Accepted |
| [0014](0014-typed-ipc.md)                            | Typed IPC: generated bindings and the change event           | Accepted |
| [0015](0015-sqlite-settings-and-migrations.md)       | SQLite settings, migration policy and the performance budget | Accepted |
| [0016](0016-blocks-and-the-editor.md)                | Blocks and the editor                                        | Accepted |
| [0017](0017-ui-architecture.md)                      | UI architecture: packages, state, live views and the CSP     | Accepted |
| [0018](0018-e2e-harness-and-ui-budgets.md)           | The end-to-end harness and the UI budgets                    | Accepted |
| [0019](0019-mind-map.md)                             | The mind map                                                 | Proposed |
| [0020](0020-search-and-query.md)                     | Search, the query DSL and saved views                        | Proposed |
| [0021](0021-files-folder-engine.md)                  | The files folder engine, the export, and what they commit    | Proposed |
| [0022](0022-markdown-parser.md)                      | The Markdown parser for the files folder                     | Proposed |
| [0023](0023-yaml-frontmatter.md)                     | The YAML crate for frontmatter                               | Proposed |
| [0024](0024-file-watcher.md)                         | The file watcher                                             | Proposed |
| [0025](0025-three-way-merge.md)                      | The three-way merge inside a block                           | Proposed |
| [0026](0026-ingestion-v0.md)                         | Ingestion v0: the importers, the run and provenance          | Proposed |
| [0027](0027-settings-modes-and-themes.md)            | Settings, modes and themes                                   | Proposed |
| [0028](0028-onboarding-and-the-network-guarantee.md) | Onboarding and the network guarantee                         | Proposed |
| [0029](0029-the-release.md)                          | The release                                                  | Proposed |
| [0030](0030-table-columns.md)                        | Adding columns to a Table                                    | Proposed |
| [0035](0035-the-canvas.md)                           | The canvas: Excalidraw, with no network                      | Proposed |
