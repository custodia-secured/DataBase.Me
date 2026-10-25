# Test fixtures

Sample data for integration, import and security tests. Nothing here is ever executed: fixtures
are read as data by tests only.

Planned contents, added with the milestone that tests them:

| Folder            | Contents                                                                                                                                         | Milestone |
| ----------------- | ------------------------------------------------------------------------------------------------------------------------------------------------ | --------- |
| `vaults/`         | Small DataBase.Me vaults at known format versions, for migration and round-trip tests                                                            | M1, M5    |
| `obsidian/`       | An Obsidian vault with wikilinks, embeds, block references, tags and properties                                                                  | M5, M6    |
| `csv/`, `xlsx/`   | Spreadsheets whose rows become objects                                                                                                           | M6        |
| `pdf/`, `images/` | PDFs with text, photos with EXIF data                                                                                                            | M6        |
| `mail/`           | mbox and .eml files                                                                                                                              | M9        |
| `takeout/`        | A trimmed Google Takeout export                                                                                                                  | M12       |
| `malicious/`      | Hostile inputs: zip bombs, malformed PDFs, path-traversal archive entries, oversized frontmatter. Tests must show each is rejected or contained. | M5, M6    |

Fixtures must be small, made up or freely licensed, and contain no real personal data.
