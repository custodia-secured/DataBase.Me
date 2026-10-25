# ADR-0003: Identifiers, ordering keys, hashes and timestamps

- **Status:** Accepted
- **Date:** 2026-10-25
- **Milestone:** M0 (implemented in M1)

## Context

Objects are created offline, on several devices, by several people, and must keep one identity for
life: in the database, in the files folder, in exports and in sync. Siblings in outlines and mind
maps need a stable order that can change without renumbering everything. File hashes must stay
meaningful if the hash algorithm ever changes.

## Decision

- **IDs are ULIDs** for objects, types, fields, blocks, relations, views, ops, import runs and
  conflicts: 128 bits, a 48-bit millisecond timestamp followed by 80 random bits, written as 26
  characters of Crockford base32 in upper case (for example `01JB3R7Q8Y5K3M2N4P6Q8R0S2T`). They
  are generated offline without coordination, sort by creation time, and are safe in file names
  and Obsidian block anchors.
- **Sibling order uses fractional index sort keys**: strings that sort lexicographically, where a
  new key can always be made between any two existing keys. Equal keys (possible after concurrent
  inserts) are ordered by ID. The exact key alphabet and algorithm are fixed in M1 and documented
  in FORMAT.md.
- **Hashes carry their algorithm**: `<algorithm>:<lowercase hex>`, for example
  `blake3:af13...`. BLAKE3 is the algorithm today. A reader that meets an unknown algorithm must
  report it rather than guess.
- **Timestamps** are integer milliseconds since the Unix epoch, UTC, in the database, and ISO 8601
  in UTC (`2026-10-25T08:21:11.000Z`) in files and exports.
- **Actor and device IDs** are ULIDs created on first launch (M1). Shared spaces (M15) bind actors
  to public keys for signed ops.

## Consequences

- A ULID reveals when its object was created. That is acceptable because creation times are
  stored anyway, but it is worth knowing before sharing a vault.
- IDs are independent of titles, so renaming never breaks a link.
- Fractional keys can grow long after many inserts between the same pair; M1 adds a rebalancing
  op that rewrites keys for one parent's children.

## Alternatives considered

- **UUIDv7 (RFC 9562).** Also time-sortable and standard. ULID is shorter as text (26 characters
  against 36), case-insensitive and has no hyphens, which suits file names and block anchors.
- **UUIDv4.** Not sortable, so indexes fragment and IDs carry no useful order.
- **Integer auto-increment.** Collides between devices; unusable for sync.
- **Integer positions for order.** Every insert renumbers siblings, which conflicts in sync.
