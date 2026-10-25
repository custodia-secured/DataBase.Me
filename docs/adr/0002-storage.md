# ADR-0002: Storage: SQLite vault, blob store and op log

- **Status:** Accepted
- **Date:** 2026-10-25
- **Milestone:** M0 (implemented in M1, encryption in M9, vectors in M11)

## Context

A lifetime repository must keep data safe for decades, handle a million objects and a terabyte of
files, survive crashes, stay readable without the app, and later sync between devices. Plain
Markdown files are durable but weak at types, relations, transactions and scale; the build plan
(§2.4) records Logseq's 2026 move from files to SQLite as evidence. SQLite is a Library of
Congress recommended storage format with a long-term support promise.

## Decision

**The canonical store is one SQLite file**, `vault.sqlite`, opened with rusqlite (bundled SQLite,
so every platform including iOS and Android gets the same version) with FTS5 for full-text search.
The file is marked with SQLite's `application_id` and its schema version with `user_version`, and
it changes only through numbered migrations. The tables are those in the handoff brief §4;
`docs/FORMAT.md` documents them.

**Files live in a content-addressed blob store** next to the database:
`blobs/<first 2 hex>/<next 2 hex>/<full hex>`, named by the file's BLAKE3 hash. Blobs are
immutable, written to a temporary file and renamed into place, and stored once however many objects
use them (deduplication). The database records them in `blobs` and links them through `attachments`.

**History is an append-only op log**, the `ops` table. Every change is an op; each op's hash covers
its content and the previous op's hash, so the log is tamper-evident. The op log provides history,
undo and redo, provenance, and later sync (M13). The other tables are materialised state, updated in
the same transaction as the op that changes them.

**The files folder is a replica, not a second source of truth** (ADR-0006). It syncs through the op
log like a second device.

**Vectors** (sqlite-vec) are created only when the user turns on an embedding provider (M11).

**Encryption at rest** (M9, optional): SQLCipher-compatible database encryption through
SQLite3MultipleCiphers, a passphrase stretched with Argon2id into a vault key, and blobs encrypted
per file with XChaCha20-Poly1305.

**Export** writes JSON (schema and views) and CSV (one file per type) into `export/`. These are
export-only until a later ADR makes them two-way.

The vault layout on disk:

```
MyVault.dbme/
  vault.sqlite   blobs/   files/   export/   plugins/   themes/   logs/ai-audit.jsonl   FORMAT.md
```

## Consequences

- Transactions make every change all-or-nothing; M1 must prove a process killed mid-write leaves a
  valid vault.
- Large files stay out of the database, so it stays small and fast to back up.
- Unreferenced blobs need garbage collection, which must never delete anything the Trash or the op
  log can still bring back.
- Bundling SQLite3MultipleCiphers is not the same build as rusqlite's `bundled` feature. M9 must
  confirm it builds for all five targets, or record an alternative in an ADR.
- A reader without the app needs only SQLite and FORMAT.md.

## Alternatives considered

- **Markdown files as the canonical store.** Durable and simple, but no transactions, weak types and
  relations, and slow at scale. We keep the benefit through the two-way files folder instead.
- **DuckDB.** Excellent for analytics, but not an application file format with SQLite's stability
  promise. It may be added later for analytics only.
- **A CRDT document (Automerge, Loro) as the canonical store.** Ideal for sync but opaque to other
  tools and unproven at a million objects. CRDTs arrive behind a sync adapter in M13.
- **LMDB, sled or another key-value store.** Fast, but no SQL, no FTS, and a weaker long-term story.
