# Test fixtures

Sample data for integration, import and security tests. Nothing here is ever executed: fixtures
are read as data by tests only.

Contents, each added with the milestone that tests it; a row without a test is planned:

| Folder                | Contents                                                                                                                                                                        | Milestone | Test                                   |
| --------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------- | -------------------------------------- |
| `vaults/v2.dbme/`     | An empty vault at schema version 2 (made with the CLI at `main`'s M4 merge, 25 ops); opening it runs migration 3                                                                | M5        | `crates/store/tests/migrate.rs`        |
| `obsidian/`           | An Obsidian vault with wikilinks, aliases, block links, embeds, tags, a `links:` relation, lists, a picture and three cloud drives' conflicting copies; `links.json` names each | M5, M6    | `crates/mirror/tests/obsidian.rs`      |
| `obsidian-roundtrip/` | The golden: `obsidian/` read into a fresh vault and written back, ids as `ULID<n>`; rewritten with `DBME_WRITE_FIXTURES=1`                                                      | M5        | `crates/mirror/tests/obsidian.rs`      |
| `obsidian-gate/`      | The sample a person opens in Obsidian at gate 1h: every block kind, both frontmatter shapes, the link cases                                                                     | M5        | a person, per its README               |
| `malicious/`          | Hostile inputs for the files folder: oversized and too-deep frontmatter, `..` in ids, links and embeds, bad ids; the test adds a junction and a 240-character path              | M5, M6    | `crates/mirror/tests/security.rs`      |
| `csv/`                | Tables whose rows become objects: `people.csv` (every inferred kind, a quoted newline, a misfit cell per kind), `semicolon.csv`, `bom-ragged.csv` (a BOM, ragged rows)          | M6        | `crates/ingest/tests/import_tables.rs` |
| `xlsx/`               | `people.xlsx`: two sheets, a date by style, a formula with its cached value, an empty column, a ragged row; the test writes it from its own builder and checks it is unchanged  | M6        | `crates/ingest/tests/import_tables.rs` |
| `pdf/`, `images/`     | PDFs with text, photos with EXIF data                                                                                                                                           | M6        |
| `mail/`               | mbox and .eml files                                                                                                                                                             | M9        |
| `takeout/`            | A trimmed Google Takeout export                                                                                                                                                 | M12       |

Fixtures must be small, made up or freely licensed, and contain no real personal data.
