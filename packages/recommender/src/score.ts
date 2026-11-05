// SPDX-License-Identifier: AGPL-3.0-or-later
/** How often the first recommendation is the one a set of fixtures labels (M8, acceptance). */
import type { Fixture } from "./fixtures";
import { recommend } from "./recommend";

export interface Agreement {
  agreed: number;
  total: number;
  /** From 0 to 1. */
  share: number;
  /** What disagreed, for reading. */
  misses: { name: string; expected: string; got: string }[];
}

export function agreement(fixtures: readonly Fixture[]): Agreement {
  const misses: Agreement["misses"] = [];
  for (const fixture of fixtures) {
    const got = recommend(fixture.shape, fixture.goal)[0]?.view ?? "nothing";
    if (got !== fixture.expected) {
      misses.push({ name: fixture.name, expected: fixture.expected, got });
    }
  }
  const total = fixtures.length;
  const agreed = total - misses.length;
  return { agreed, total, share: total === 0 ? 0 : agreed / total, misses };
}
