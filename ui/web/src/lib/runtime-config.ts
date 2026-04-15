export type EpisodeKeyBindings = {
  startKey: string;
  stopKey: string;
  keepKey: string;
  discardKey: string;
};

export interface UiRuntimeConfig {
  websocketUrl: string;
  episodeKeyBindings: EpisodeKeyBindings;
}

type LocationLike = Pick<Location, "protocol" | "host">;

type RawUiRuntimeConfig = {
  websocketUrl?: unknown;
  episodeKeyBindings?: Partial<Record<keyof EpisodeKeyBindings, unknown>>;
};

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
    throw new Error('runtime config "websocketUrl" must be a non-empty string');
  }

  if (isAbsoluteUrl(trimmed)) {
    const resolved = new URL(trimmed);
    resolved.protocol = normalizeWebSocketProtocol(resolved.protocol);
    if (resolved.protocol !== "ws:" && resolved.protocol !== "wss:") {
      throw new Error('runtime config "websocketUrl" must use ws:// or wss://');
    }
    return resolved.toString();
  }

  return new URL(trimmed, browserWebSocketOrigin(location)).toString();
}

export function normalizeRuntimeConfig(
  config: RawUiRuntimeConfig,
  location: LocationLike = window.location,
): UiRuntimeConfig {
  if (typeof config.websocketUrl !== "string" || config.websocketUrl.trim() === "") {
    throw new Error('runtime config "websocketUrl" must be a non-empty string');
  }

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
    websocketUrl: resolveWebSocketUrl(config.websocketUrl, location),
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
