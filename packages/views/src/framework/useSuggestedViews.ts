// SPDX-License-Identifier: AGPL-3.0-or-later
import { useDataProfile, useFields, type Id } from "@databaseme/client";
import { suggestions, type Recommendation } from "@databaseme/recommender";
import { useMemo } from "react";
import { listingOf, listingQuery } from "./listing";
import { profileKeys, shapeOf } from "./suggested";

/**
 * Which views suit what is under a page, best first (M8, Task 9). `asked` keeps it from reading anything
 * until someone wants to know, since every change to the vault would otherwise read a profile again.
 */
export function useSuggestedViews(
  objectId: Id,
  asked: boolean,
): { suggested: readonly Recommendation[]; failed: boolean } {
  const listing = useMemo(() => listingOf(objectId), [objectId]);
  const fields = useFields(listing.source, asked);
  const keys = useMemo(() => profileKeys(fields.data ?? []), [fields.data]);
  const query = useMemo(() => listingQuery(listing), [listing]);
  const profile = useDataProfile(asked && fields.data !== undefined ? query : null, keys);
  const suggested = useMemo(
    () => (profile.data && fields.data ? suggestions(shapeOf(profile.data, fields.data)) : []),
    [profile.data, fields.data],
  );
  return { suggested, failed: asked && (fields.isError || profile.isError) };
}
