// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * ULIDs made in the UI (ADR-0003, ADR-0016): 48 bits of milliseconds then 80 random bits, as 26
 * characters of Crockford base32. The editor names a block the moment it exists, before any round
 * trip to the vault. Within one millisecond the random part counts up, so IDs made here always
 * sort in the order they were made.
 */

const ALPHABET = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";
const RANDOM_BITS = 80n;
const RANDOM_MASK = (1n << RANDOM_BITS) - 1n;

let lastMs = -1;
let lastRandom = 0n;

function randomBits(): bigint {
  const bytes = new Uint8Array(10);
  crypto.getRandomValues(bytes);
  let value = 0n;
  for (const byte of bytes) value = (value << 8n) | BigInt(byte);
  return value;
}

/** Encodes a 128-bit value as 26 base32 characters. */
export function encodeUlid(value: bigint): string {
  let text = "";
  let rest = value;
  for (let i = 0; i < 26; i++) {
    text = (ALPHABET[Number(rest & 31n)] ?? "0") + text;
    rest >>= 5n;
  }
  return text;
}

/** A new ULID for time `now` (milliseconds; defaults to the clock). */
export function newId(now: number = Date.now()): string {
  const ms = Math.max(now, lastMs);
  let random: bigint;
  if (ms === lastMs) {
    random = (lastRandom + 1n) & RANDOM_MASK;
    // Overflow within one millisecond: borrow the next one.
    if (random === 0n) return newId(ms + 1);
  } else {
    random = randomBits();
  }
  lastMs = ms;
  lastRandom = random;
  return encodeUlid((BigInt(ms) << RANDOM_BITS) | random);
}
