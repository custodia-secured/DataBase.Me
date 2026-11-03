# Hostile inputs for the files folder (M5)

Each file here must be rejected or contained by the files folder: an inbox entry, a plain page, or
nothing at all, and never a read or a write outside the folder. The test
`crates/mirror/tests/security.rs` copies this folder into a temporary files folder, adds what a
repository cannot hold (a junction or symlink pointing outside the folder, a file whose path is 240
characters long), reads it into a fresh vault, and checks that nothing outside the folder was
touched and that every written path stays inside it.

| File                         | What it tries                                                  | Expected                                           |
| ---------------------------- | -------------------------------------------------------------- | -------------------------------------------------- |
| `oversized-frontmatter.md`   | A frontmatter block over 256 KiB (`MAX_FRONTMATTER_BYTES`)     | A `parse_error` in the inbox; no page              |
| `deep-yaml.md`               | YAML nested 70 maps deep (`MAX_YAML_DEPTH` is 64)              | A `parse_error` in the inbox; no page              |
| `traversal.md`               | `..` in the `id`, in an embed, in a link and in an image block | A page; the embed and link stay text; nothing read |
| `bad-ids.md`                 | An `id` that is no ULID, a relation to one, bad anchors        | A page with a new id; the bad parts are text       |
| (made by the test) junction  | A junction or symlink inside the folder pointing outside it    | Never followed; nothing under it becomes a page    |
| (made by the test) deep path | A file whose relative path is 240 characters long              | A page at that path, inside the folder             |

`CON.md` cannot be committed (Windows refuses the name) and a 240-character path exceeds `MAX_PATH`
in a checkout, so the reserved-name rule is tested in `dbme-core` (`paths.rs`) and the long path is
made by the test. Nothing here is executed by anything; the files are data. The two generated
files were made by a short Python loop; their content is a repeated comment line and a nested
`a:` map.
