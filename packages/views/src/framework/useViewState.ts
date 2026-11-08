// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * A view's settings (M8, ADR-0031): what it lists, in what order, its own options and whether it
 * shows as a list. They last as long as the view is open on the object, until the person saves them
 * (Decision 8): nothing is saved by looking at something.
 */
import { MANUAL, type Id, type Sort, type ViewRow_Serialize } from "@databaseme/client";
import { useState } from "react";
import { listingFromQuery, listingOf, type Listing } from "./listing";

/** What a saved view keeps beside its query. */
export interface SavedConfig<O> {
  options?: Partial<O>;
  as_list?: boolean;
  /** A chart's Vega-Lite spec, kept for what reads a vault from outside; never drawn from. */
  spec?: unknown;
}

export interface ViewState<O> {
  listing: Listing;
  setListing: (listing: Listing) => void;
  sort: Sort;
  setSort: (sort: Sort) => void;
  options: O;
  setOptions: (options: O) => void;
  /** Whether the view shows as its accessible alternative. */
  asList: boolean;
  setAsList: (asList: boolean) => void;
  /** The saved view these settings came from, if any. */
  savedId: Id | null;
  setSavedId: (id: Id | null) => void;
  /** Takes on a saved view's settings. */
  load: (view: ViewRow_Serialize) => void;
  /** Back to what the view shows before anything is chosen. */
  reset: () => void;
}

export function useViewState<O>(
  objectId: Id,
  defaults: O,
  defaultSort: Sort = MANUAL,
  /** Whether the view starts as its alternative, as the Graph does on a phone. */
  startAsList = false,
): ViewState<O> {
  const [listing, setListing] = useState<Listing>(() => listingOf(objectId));
  const [sort, setSort] = useState<Sort>(defaultSort);
  const [options, setOptions] = useState<O>(defaults);
  const [asList, setAsList] = useState(startAsList);
  const [savedId, setSavedId] = useState<Id | null>(null);
  const reset = () => {
    setListing(listingOf(objectId));
    setSort(defaultSort);
    setOptions(defaults);
    setAsList(startAsList);
    setSavedId(null);
  };
  // Another object starts over: settings belong to the object they were chosen on.
  const [shownFor, setShownFor] = useState(objectId);
  if (shownFor !== objectId) {
    setShownFor(objectId);
    reset();
  }
  return {
    listing,
    setListing,
    sort,
    setSort,
    options,
    setOptions,
    asList,
    setAsList,
    savedId,
    setSavedId,
    reset,
    load(view) {
      const saved = listingFromQuery(view.query_json, objectId);
      const config = (view.config_json ?? {}) as SavedConfig<O>;
      setListing(saved.listing);
      setSort(saved.sort);
      setOptions({ ...defaults, ...config.options });
      setAsList(config.as_list === true);
      setSavedId(view.id);
    },
  };
}
