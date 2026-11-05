// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * What a listing has in each period on screen (M8): the store counts them in one read (build plan
 * §2.7), and only those with something in them are read, each to a bound. The Calendar's days and the
 * Timeline's buckets are both this.
 */
import {
  commands,
  keys,
  unwrap,
  useAggregate,
  type Condition_Deserialize,
  type ObjectSummary_Serialize,
  type Sort,
  type TimeBin,
} from "@databaseme/client";
import { useQueries } from "@tanstack/react-query";
import { useMemo } from "react";
import { addDays, countsByKey, rangeConditions, type Day } from "../calendar/dates";
import { listingQuery, narrowed, type Listing } from "./listing";

/** One period: its key as the store groups it, and its days, up to but not including `end`. */
export interface Bucket {
  key: string;
  start: Day;
  end: Day;
}

/** Each of `days` as a bucket. */
export function dayBuckets(days: readonly Day[]): Bucket[] {
  return days.map((day) => ({ key: day, start: day, end: addDays(day, 1) }));
}

/** The conditions for what is in a bucket. */
export function inBucket(key: string, bucket: Bucket): Condition_Deserialize[] {
  return rangeConditions(key, bucket.start, bucket.end);
}

export interface BucketReads {
  /** The buckets with something in them, in order. */
  filled: readonly Bucket[];
  /** Whether the counts have been read. */
  ready: boolean;
  countOf: (bucketKey: string) => number;
  /** The first `bound` of a bucket; undefined until they have been read. */
  itemsOf: (bucketKey: string) => readonly ObjectSummary_Serialize[] | undefined;
}

/** Reads `buckets`. With a `bound` of 0, only their counts. */
export function useBuckets(
  listing: Listing,
  key: string,
  bin: TimeBin,
  buckets: readonly Bucket[],
  bound: number,
  sort: Sort,
): BucketReads {
  const start = buckets[0]?.start ?? "";
  const end = buckets.at(-1)?.end ?? start;
  const counted = useAggregate({
    query: listingQuery(narrowed(listing, ...rangeConditions(key, start, end))),
    group_by: [{ field: key, bin }],
    measures: [{ op: "count" }],
  });
  const counts = useMemo(() => countsByKey(counted.data), [counted.data]);
  const filled = buckets.filter((bucket) => (counts.get(bucket.key) ?? 0) > 0);
  const toRead = bound > 0 ? filled : [];
  const reads = useQueries({
    queries: toRead.map((bucket) => {
      const query = listingQuery(narrowed(listing, ...inBucket(key, bucket)), sort, 0, bound);
      return {
        queryKey: keys.query(query),
        queryFn: () => unwrap(commands.objectsQuery(query)),
      };
    }),
  });
  return {
    filled,
    ready: counted.isSuccess,
    countOf: (bucketKey) => counts.get(bucketKey) ?? 0,
    itemsOf: (bucketKey) => reads[toRead.findIndex((bucket) => bucket.key === bucketKey)]?.data,
  };
}
