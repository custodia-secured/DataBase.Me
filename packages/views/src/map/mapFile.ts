// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * A map file on this machine (M8, Task 10, ADR-0033), read in ranges over IPC. The shell reads it, so the
 * webview cannot name another file, and nothing here makes a request.
 */
import { commands, unwrap, useSettings } from "@databaseme/client";
import { useQuery } from "@tanstack/react-query";
import { addProtocol } from "maplibre-gl";
import { PMTiles, Protocol, TileType, type RangeResponse, type Source } from "pmtiles";
import type { MapFileShape } from "./geo";

function bytesOf(base64: string): ArrayBuffer {
  const text = atob(base64);
  const bytes = new Uint8Array(text.length);
  for (let at = 0; at < text.length; at += 1) bytes[at] = text.charCodeAt(at);
  return bytes.buffer;
}

class IpcSource implements Source {
  constructor(private readonly key: string) {}

  getKey(): string {
    return this.key;
  }

  async getBytes(offset: number, length: number): Promise<RangeResponse> {
    return { data: bytesOf(await unwrap(commands.mapFileRead(offset, length))) };
  }
}

let protocol: Protocol | null = null;
let opened = 0;

function theProtocol(): Protocol {
  if (!protocol) {
    protocol = new Protocol();
    addProtocol("pmtiles", protocol.tile);
  }
  return protocol;
}

async function openMapFile(): Promise<MapFileShape> {
  // A name of its own for each opening: a map keeps tiles by URL, and this file may not be the last one.
  opened += 1;
  const archive = new PMTiles(new IpcSource(`map-file-${String(opened)}`));
  const header = await archive.getHeader();
  // Only a vector file's layers have to be known, and they are all its metadata is needed for.
  const vector = header.tileType === TileType.Mvt;
  const metadata = vector
    ? ((await archive.getMetadata()) as { vector_layers?: { id: string }[] } | null)
    : null;
  theProtocol().add(archive);
  return {
    key: archive.source.getKey(),
    kind: vector ? "vector" : "raster",
    minZoom: header.minZoom,
    maxZoom: header.maxZoom,
    layers: (metadata?.vector_layers ?? []).map((layer) => layer.id),
  };
}

export interface MapFileRead {
  file: MapFileShape | null;
  /** Whether what is chosen has been read, or nothing is chosen. */
  settled: boolean;
  failed: boolean;
}

/** The map file the settings name. If it cannot be read, the map goes on without it and says so. */
export function useMapFile(): MapFileRead {
  const settings = useSettings();
  const path = settings.data?.map_file ?? null;
  const read = useQuery({
    queryKey: ["map-file", path],
    queryFn: openMapFile,
    enabled: path !== null,
    staleTime: Infinity,
    retry: false,
  });
  return {
    file: path !== null ? (read.data ?? null) : null,
    settled: settings.isSuccess && (path === null || !read.isPending),
    failed: path !== null && read.isError,
  };
}
