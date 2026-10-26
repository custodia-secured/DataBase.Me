// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * Reads of the open vault as TanStack Query hooks (ADR-0017). Every hook's key starts with
 * {@link VAULT}, so a change refreshes them all at once (see `live.ts`). Large lists are read in
 * pages; nothing reads a whole table.
 */
import { keepPreviousData, useQuery, type QueryClient } from "@tanstack/react-query";
import type {
  Backlink_Serialize,
  BlockRow_Serialize,
  FieldRow,
  Id,
  ObjectRow_Serialize,
  ObjectSummary_Serialize,
  Sort,
  Source,
  TypeRow_Serialize,
  VaultInfo,
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
  blocks: (objectId: Id) => [VAULT, "blocks", objectId] as const,
  backlinks: (id: Id) => [VAULT, "backlinks", id] as const,
  search: (text: string, limit: number) => [VAULT, "search", text, limit] as const,
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

export function useFields(source: Source) {
  return useQuery<FieldRow[]>({
    queryKey: keys.fields(source),
    queryFn: () => unwrap(commands.fieldsList(source)),
  });
}

export function useBlocks(objectId: Id | null) {
  return useQuery<BlockRow_Serialize[]>({
    queryKey: keys.blocks(objectId ?? ""),
    queryFn: () => unwrap(commands.blocksList(objectId ?? "")),
    enabled: objectId !== null,
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
