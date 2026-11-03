# An Obsidian vault for the files folder (M5)

A small vault in the shape Obsidian writes and the files folder reads (FORMAT.md §6): path-form,
bare, aliased and block wikilinks, an embed of a block and of a picture, tags in the frontmatter and
in the text, a `links:` relation, nested bullets, a numbered list, a code fence, a divider and a
table, and three cloud drives' conflicting copies of one note (Dropbox, Syncthing, Nextcloud).
Nothing here is real data; every name, id and date is made up.

`links.json` names every link and what the files folder makes of it. The test
`crates/mirror/tests/obsidian.rs` reads this folder into a fresh vault and checks each entry, then
writes the vault back in the folder's canonical form and compares the result with
`../obsidian-roundtrip/`, the golden of the normalising round trip, with every id replaced by a
`ULID<n>` placeholder. Set `DBME_WRITE_FIXTURES=1` on that test to rewrite the golden after a
deliberate format change, and read the diff before committing it.

Open it in Obsidian with "Open folder as vault"; do not commit what Obsidian adds under
`.obsidian/`.

Two things the manifest records as they are, not as one might wish: the picture Obsidian named
(`attachments/pixel.png`) is not an app attachment, since those are named by their hash (M5
Decision 9) and importing foreign files is M6 work, so its embed stays text and the file stays
where it is, which Obsidian shows as before; and the tags become `Tag` pages of their own
(`alpha.md`, `team.md`, `home (2).md`, `home-sub.md` in the golden), since a Tag is an object like
any other.

Left out on purpose: a note that links to a missing note (the round trip must end with zero
dangling links), and a pair of names differing only in case, which no repository checked out on
Windows or macOS can hold; the engine's case rule is tested on the in-memory file system instead.

In the golden, a binary file stands as its size (`<70 bytes>`), since only text is diffed.
