# Hostile inputs for the importers (M6)

Each file here must be refused or contained by an import: the plan and the apply return inside a
bound, the vault verifies afterwards, the file is attached whole as the object's original, and
nothing outside the import's own folders is read into the vault or written to. The test
`crates/ingest/tests/security.rs` imports each one into a fresh vault beside a folder that must
stay untouched, and makes what a repository should not hold (a CSV with a 1 MiB cell, a nested
zip of a megabyte of zeros). Every file here is the test's own builder's output and is checked
unchanged; delete one and the test writes it again.

M5's `fixtures/malicious/` is a separate folder on purpose: the files folder's test copies it
whole, and the same test here imports it whole through the engine.

| File                   | What it tries                                                                   | Expected                                                              |
| ---------------------- | ------------------------------------------------------------------------------- | --------------------------------------------------------------------- |
| `truncated.pdf`        | The two-page fixture cut at three fifths                                        | A `Document` with the file attached; its text, or a warning           |
| `recursive.pdf`        | A page whose parent's only kid is the page itself                               | A `Document`, the walk ends; no stack overflow                        |
| `page-tree-bomb.pdf`   | A 3 KB file declaring a million pages through three levels of a hundred refs    | A `Document`, bounded by the 10 s text budget                         |
| `png-bomb.png`         | A PNG header declaring 100,000 × 100,000 pixels and no image data               | A `Media` with the file attached, a warning, no thumbnail             |
| `formulas.csv`         | Cells beginning with `=`, `@`, `+` and `-` (formula injection)                  | Text, verbatim; the plan counts them in a warning; nothing is run     |
| `sheet-traversal.xlsx` | A sheet named `../x`                                                            | A type and a dataset named `../x`; no path is made from it            |
| (made by the test)     | A CSV with a cell of 1 MiB + 10 bytes                                           | The cell cut at 1 MiB and counted; the file attached whole            |
| (made by the test)     | A zip holding a zip holding a zip holding a megabyte of zeros                   | A `Document` with the file attached at its own size; nothing unpacked |

Nothing here is executed by anything; the files are data.
