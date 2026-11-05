// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * Reads of the open vault as TanStack Query hooks (ADR-0017). Every hook's key starts with
 * {@link VAULT}, so a change refreshes them all at once (see `live.ts`). Large lists are read in
 * pages; nothing reads a whole table.
 */
import { keepPreviousData, useQuery, type QueryClient } from "@tanstack/react-query";
import type {
  AggregateQuery_Deserialize,
  AggregateResult,
  AttachmentRow_Serialize,
  Backlink_Serialize,
  BlockRow_Serialize,
  ColumnTarget_Serialize,
  DataProfile_Serialize,
  Conflict_Serialize,
  FacetBucket,
  FieldRow,
  Id,
  MirrorStatus,
  ObjectRow_Serialize,
  ObjectSummary_Serialize,
  ObjectTree_Serialize,
  ProvenanceRow_Serialize,
  Query_Deserialize,
  SearchHit_Serialize,
  Settings,
  SettingsPatch,
  Sort,
  Source,
  TypeRow_Serialize,
  VaultInfo,
  ViewRow_Serialize,
} from "./bindings";
import { commands, unwrap } from "./ipc";

/** The first element of every vault query key. */
export const VAULT = "vault";

/** Keys of the vault queries. */
export const keys = {
  info: () => [VAULT, "info"] as const,
  types: () => [VAULT, "types"] as const,
  object: (id: Id) => [VAULT, "object", id] as const,
  summary: (id: Id) => [VAULT, "summary", id] as const,
  ancestors: (id: Id) => [VAULT, "ancestors", id] as const,
  page: (source: Source, sort: Sort, offset: number, limit: number) =>
    [VAULT, "page", source, sort, offset, limit] as const,
  count: (source: Source) => [VAULT, "count", source] as const,
  fields: (source: Source) => [VAULT, "fields", source] as const,
  columnTarget: (source: Source) => [VAULT, "column-target", source] as const,
  blocks: (objectId: Id) => [VAULT, "blocks", objectId] as const,
  backlinks: (id: Id) => [VAULT, "backlinks", id] as const,
  search: (text: string, limit: number) => [VAULT, "search", text, limit] as const,
  tree: (root: Id, limit: number) => [VAULT, "tree", root, limit] as const,
  fullSearch: (text: string, limit: number) => [VAULT, "full-search", text, limit] as const,
  query: (query: Query_Deserialize) => [VAULT, "query", query] as const,
  queryCount: (query: Query_Deserialize) => [VAULT, "query-count", query] as const,
  aggregate: (aggregate: AggregateQuery_Deserialize) => [VAULT, "aggregate", aggregate] as const,
  dataProfile: (query: Query_Deserialize, fields: readonly string[]) =>
    [VAULT, "data-profile", query, fields] as const,
  facets: (query: Query_Deserialize, field: string) => [VAULT, "facets", query, field] as const,
  views: () => [VAULT, "views"] as const,
  view: (id: Id) => [VAULT, "view", id] as const,
  mirror: () => [VAULT, "mirror"] as const,
  conflicts: (openOnly: boolean) => [VAULT, "conflicts", openOnly] as const,
  provenance: (id: Id) => [VAULT, "provenance", id] as const,
  attachments: (id: Id) => [VAULT, "attachments", id] as const,
};

/** Blobs never change, so their key is outside {@link VAULT} and no refresh reloads them. */
export const blobKey = (hash: string) => ["blob", hash] as const;

/** The usual order: as people arranged it. */
export const MANUAL: Sort = { by: "manual", field: null, descending: false };

export function useVaultInfo() {
  return useQuery<VaultInfo | null>({
    queryKey: keys.info(),
    queryFn: () => unwrap(commands.vaultInfo()),
  });
}

export function useTypes() {
  return useQuery<TypeRow_Serialize[]>({
    queryKey: keys.types(),
    queryFn: () => unwrap(commands.typesList()),
    staleTime: Infinity,
  });
}

export function useObject(id: Id | null) {
  return useQuery<ObjectRow_Serialize>({
    queryKey: keys.object(id ?? ""),
    queryFn: () => unwrap(commands.objectGet(id ?? "")),
    enabled: id !== null,
  });
}

/** An object with its types, child count and cover. */
export function useSummary(id: Id | null) {
  return useQuery<ObjectSummary_Serialize | null>({
    queryKey: keys.summary(id ?? ""),
    queryFn: async () => (await unwrap(commands.objectSummaries([id ?? ""])))[0] ?? null,
    enabled: id !== null,
  });
}

export function useAncestors(id: Id | null) {
  return useQuery<ObjectRow_Serialize[]>({
    queryKey: keys.ancestors(id ?? ""),
    queryFn: () => unwrap(commands.objectAncestors(id ?? "")),
    enabled: id !== null,
  });
}

export function usePage(source: Source, sort: Sort, offset: number, limit: number, enabled = true) {
  return useQuery<ObjectSummary_Serialize[]>({
    queryKey: keys.page(source, sort, offset, limit),
    queryFn: () => unwrap(commands.objectsPage(source, sort, offset, limit)),
    placeholderData: keepPreviousData,
    enabled,
  });
}

export function useCount(source: Source) {
  return useQuery<number>({
    queryKey: keys.count(source),
    queryFn: () => unwrap(commands.objectsCount(source)),
  });
}

export function useFields(source: Source, enabled = true) {
  return useQuery<FieldRow[]>({
    queryKey: keys.fields(source),
    queryFn: () => unwrap(commands.fieldsList(source)),
    enabled,
  });
}

/** The type a new column of a Table showing `source` goes on (ADR-0030). */
export function useColumnTarget(source: Source, enabled: boolean) {
  return useQuery<ColumnTarget_Serialize>({
    queryKey: keys.columnTarget(source),
    queryFn: () => unwrap(commands.columnTarget(source)),
    enabled,
  });
}

export function useBlocks(objectId: Id | null) {
  return useQuery<BlockRow_Serialize[]>({
    queryKey: keys.blocks(objectId ?? ""),
    queryFn: () => unwrap(commands.blocksList(objectId ?? "")),
    enabled: objectId !== null,
  });
}

/** The most objects one mind map shows (ADR-0019). */
export const TREE_LIMIT = 5000;

/**
 * An object and what lies below it, in one read: the mind map would otherwise take a round trip
 * per branch. Structural sharing keeps the nodes that didn't change identical from read to read.
 */
export function useTree(root: Id | null, limit = TREE_LIMIT) {
  return useQuery<ObjectTree_Serialize>({
    queryKey: keys.tree(root ?? "", limit),
    queryFn: () => unwrap(commands.objectTree(root ?? "", limit)),
    enabled: root !== null,
  });
}

export function useBacklinks(id: Id | null) {
  return useQuery<Backlink_Serialize[]>({
    queryKey: keys.backlinks(id ?? ""),
    queryFn: () => unwrap(commands.backlinks(id ?? "")),
    enabled: id !== null,
  });
}

export function useSearch(text: string, limit = 20, enabled = true) {
  return useQuery<ObjectSummary_Serialize[]>({
    queryKey: keys.search(text, limit),
    queryFn: () => unwrap(commands.objectSearch(text, limit)),
    placeholderData: keepPreviousData,
    enabled,
  });
}

/** Objects matching `text` anywhere, best first, with snippets (ADR-0020). */
export function useFullSearch(text: string, limit = 12, enabled = true) {
  return useQuery<SearchHit_Serialize[]>({
    queryKey: keys.fullSearch(text, limit),
    queryFn: () => unwrap(commands.search(text, limit)),
    placeholderData: keepPreviousData,
    enabled,
  });
}

/** The results of a query (ADR-0020), or nothing while there is no query. */
export function useQueryResults(query: Query_Deserialize | null) {
  return useQuery<ObjectSummary_Serialize[]>({
    queryKey: keys.query(query ?? {}),
    queryFn: () => unwrap(commands.objectsQuery(query ?? {})),
    placeholderData: keepPreviousData,
    enabled: query !== null,
  });
}

export function useQueryCount(query: Query_Deserialize | null) {
  return useQuery<number>({
    queryKey: keys.queryCount(query ?? {}),
    queryFn: () => unwrap(commands.objectsQueryCount(query ?? {})),
    placeholderData: keepPreviousData,
    enabled: query !== null,
  });
}

/** Figures per group over a query's results, worked out in the store (build plan §2.7). */
export function useAggregate(aggregate: AggregateQuery_Deserialize | null) {
  return useQuery<AggregateResult>({
    queryKey: keys.aggregate(aggregate ?? { query: {}, measures: [] }),
    queryFn: () => unwrap(commands.aggregate(aggregate ?? { query: {}, measures: [] })),
    placeholderData: keepPreviousData,
    enabled: aggregate !== null,
  });
}

/** The shape of a query's results, which the view recommender reads. Nothing is read while `query` is null. */
export function useDataProfile(query: Query_Deserialize | null, fields: readonly string[]) {
  return useQuery<DataProfile_Serialize>({
    queryKey: keys.dataProfile(query ?? {}, fields),
    queryFn: () => unwrap(commands.dataProfile(query ?? {}, [...fields])),
    enabled: query !== null,
  });
}

/** The type facet of a query's results. */
export const TYPE_FACET = "$type";

export function useFacets(query: Query_Deserialize | null, field: string) {
  return useQuery<FacetBucket[]>({
    queryKey: keys.facets(query ?? {}, field),
    queryFn: () => unwrap(commands.facets(query ?? {}, field)),
    placeholderData: keepPreviousData,
    enabled: query !== null,
  });
}

export function useViews() {
  return useQuery<ViewRow_Serialize[]>({
    queryKey: keys.views(),
    queryFn: () => unwrap(commands.viewsList()),
  });
}

export function useView(id: Id | null) {
  return useQuery<ViewRow_Serialize>({
    queryKey: keys.view(id ?? ""),
    queryFn: () => unwrap(commands.viewGet(id ?? "")),
    enabled: id !== null,
  });
}

/** Where the files folder stands (ADR-0021). The shell's `mirror-changed` event updates it too. */
export function useMirrorStatus() {
  return useQuery<MirrorStatus>({
    queryKey: keys.mirror(),
    queryFn: () => unwrap(commands.mirrorStatus()),
  });
}

/** The Conflicts inbox: every entry, or only the open ones. */
export function useConflicts(openOnly = true) {
  return useQuery<Conflict_Serialize[]>({
    queryKey: keys.conflicts(openOnly),
    queryFn: () => unwrap(commands.conflictsList(openOnly)),
  });
}

/** Where an object came from (M6): its provenance rows, oldest first. */
export function useProvenance(id: Id | null) {
  return useQuery<ProvenanceRow_Serialize[]>({
    queryKey: keys.provenance(id ?? ""),
    queryFn: () => unwrap(commands.provenanceList(id ?? "")),
    enabled: id !== null,
  });
}

/** The files an object carries: its attachments, by role. */
export function useAttachments(id: Id | null) {
  return useQuery<AttachmentRow_Serialize[]>({
    queryKey: keys.attachments(id ?? ""),
    queryFn: () => unwrap(commands.attachmentsList(id ?? "")),
    enabled: id !== null,
  });
}

/** The person's settings (M7): outside {@link VAULT}, since no vault change touches them. */
export const SETTINGS_KEY = ["settings"] as const;

export function useSettings() {
  return useQuery<Settings>({
    queryKey: SETTINGS_KEY,
    queryFn: () => unwrap(commands.settingsGet()),
    staleTime: Infinity,
  });
}

/** Changes the settings `patch` names and puts the result in the cache at once. */
export async function setSettings(client: QueryClient, patch: SettingsPatch): Promise<Settings> {
  const settings = await unwrap(commands.settingsSet(patch));
  client.setQueryData(SETTINGS_KEY, settings);
  return settings;
}

/** Chooses the file maps draw over the land they ship with, or none; refused if it is no map file. */
export async function setMapFile(client: QueryClient, path: string | null): Promise<Settings> {
  const settings = await unwrap(commands.mapFileSet(path));
  client.setQueryData(SETTINGS_KEY, settings);
  return settings;
}

/**
 * Object URLs for blob bytes. A blob is named by its hash and never changes, so one URL serves
 * every view; the oldest are revoked once more than {@link BLOB_URLS} are held.
 */
const BLOB_URLS = 256;
const blobUrls = new Map<string, string>();

function decodeBase64(data: string): Uint8Array<ArrayBuffer> {
  const binary = atob(data);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
  return bytes;
}

/** A `blob:` URL for the bytes of blob `hash`, typed `mime`. */
export async function blobUrl(hash: string, mime: string): Promise<string> {
  const known = blobUrls.get(hash);
  if (known !== undefined) return known;
  const data = await unwrap(commands.blobRead(hash));
  const url = URL.createObjectURL(new Blob([decodeBase64(data)], { type: mime }));
  blobUrls.set(hash, url);
  if (blobUrls.size > BLOB_URLS) {
    const [oldestHash, oldestUrl] = blobUrls.entries().next().value ?? [];
    if (oldestHash !== undefined && oldestUrl !== undefined) {
      blobUrls.delete(oldestHash);
      URL.revokeObjectURL(oldestUrl);
    }
  }
  return url;
}

export function useBlobUrl(hash: string | null, mime = "image/*") {
  return useQuery<string>({
    queryKey: blobKey(hash ?? ""),
    queryFn: () => blobUrl(hash ?? "", mime),
    enabled: hash !== null,
    staleTime: Infinity,
  });
}

/** Encodes bytes as base64, for {@link commands.attachmentAdd}. */
export function encodeBase64(bytes: Uint8Array): string {
  let binary = "";
  const chunk = 0x8000;
  for (let i = 0; i < bytes.length; i += chunk) {
    binary += String.fromCharCode(...bytes.subarray(i, i + chunk));
  }
  return btoa(binary);
}

/** Forgets every vault read, for when another vault opens. */
export function forgetVault(client: QueryClient): void {
  client.removeQueries({ queryKey: [VAULT] });
}
