export type EpisodeKeyBindings = {
  startKey: string;
  stopKey: string;
  keepKey: string;
  discardKey: string;
};

export type UiMode = "collect" | "setup";

export interface UiRuntimeConfig {
  mode: UiMode;
  controlWebsocketUrl: string;
  previewWebsocketUrl: string;
  episodeKeyBindings: EpisodeKeyBindings;
}

type LocationLike = Pick<Location, "protocol" | "host">;

type RawUiRuntimeConfig = {
  mode?: unknown;
  controlWebsocketUrl?: unknown;
  previewWebsocketUrl?: unknown;
  episodeKeyBindings?: Partial<Record<keyof EpisodeKeyBindings, unknown>>;
};

function normalizeMode(value: unknown): UiMode {
  // Default to `collect` when the field is absent so older gateways
  // (pre-`--mode` flag) keep rendering the recording view as before.
  if (value === undefined || value === null) {
    return "collect";
  }
  if (value === "collect" || value === "setup") {
    return value;
  }
  throw new Error(`runtime config "mode" must be "collect" or "setup"`);
}

function normalizeKey(
  label: keyof EpisodeKeyBindings,
  value: unknown,
): string {
  if (typeof value !== "string" || value.trim().length !== 1) {
    throw new Error(`runtime config "${label}" must be a single character`);
  }

  const normalized = value.trim().toLowerCase();
  if (normalized === "d") {
    throw new Error(`runtime config "${label}" conflicts with reserved shortcut "d"`);
  }
  return normalized;
}

function isAbsoluteUrl(value: string): boolean {
  return /^[a-zA-Z][a-zA-Z\d+\-.]*:/.test(value);
}

function normalizeWebSocketProtocol(protocol: string): string {
  if (protocol === "https:") {
    return "wss:";
  }
  if (protocol === "http:") {
    return "ws:";
  }
  return protocol;
}

function browserWebSocketOrigin(location: LocationLike): string {
  return `${location.protocol === "https:" ? "wss:" : "ws:"}//${location.host}`;
}

export function resolveWebSocketUrl(
  websocketUrl: string,
  location: LocationLike = window.location,
): string {
  const trimmed = websocketUrl.trim();
  if (trimmed === "") {
    throw new Error('runtime config websocket url must be a non-empty string');
  }

  if (isAbsoluteUrl(trimmed)) {
    const resolved = new URL(trimmed);
    resolved.protocol = normalizeWebSocketProtocol(resolved.protocol);
    if (resolved.protocol !== "ws:" && resolved.protocol !== "wss:") {
      throw new Error('runtime config websocket url must use ws:// or wss://');
    }
    return resolved.toString();
  }

  return new URL(trimmed, browserWebSocketOrigin(location)).toString();
}

function requireString(label: string, value: unknown): string {
  if (typeof value !== "string" || value.trim() === "") {
    throw new Error(`runtime config "${label}" must be a non-empty string`);
  }
  return value;
}

export function normalizeRuntimeConfig(
  config: RawUiRuntimeConfig,
  location: LocationLike = window.location,
): UiRuntimeConfig {
  const controlRaw = requireString("controlWebsocketUrl", config.controlWebsocketUrl);
  const previewRaw = requireString("previewWebsocketUrl", config.previewWebsocketUrl);

  const episodeKeyBindings = {
    startKey: normalizeKey("startKey", config.episodeKeyBindings?.startKey),
    stopKey: normalizeKey("stopKey", config.episodeKeyBindings?.stopKey),
    keepKey: normalizeKey("keepKey", config.episodeKeyBindings?.keepKey),
    discardKey: normalizeKey("discardKey", config.episodeKeyBindings?.discardKey),
  };

  const seen = new Set<string>();
  for (const key of Object.values(episodeKeyBindings)) {
    if (seen.has(key)) {
      throw new Error(`runtime config contains duplicate key binding "${key}"`);
    }
    seen.add(key);
  }

  return {
    mode: normalizeMode(config.mode),
    controlWebsocketUrl: resolveWebSocketUrl(controlRaw, location),
    previewWebsocketUrl: resolveWebSocketUrl(previewRaw, location),
    episodeKeyBindings,
  };
}

export async function loadRuntimeConfig(
  fetchImpl: typeof fetch = fetch,
  location: LocationLike = window.location,
): Promise<UiRuntimeConfig> {
  const response = await fetchImpl("/api/runtime-config", {
    cache: "no-store",
    headers: {
      Accept: "application/json",
    },
  });

  if (!response.ok) {
    throw new Error(`failed to load runtime config: HTTP ${response.status}`);
  }

  const payload = (await response.json()) as RawUiRuntimeConfig;
  return normalizeRuntimeConfig(payload, location);
}
