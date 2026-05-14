export interface VideoDecoderAvailability {
  available: boolean;
  summary: string;
  detail: string;
}

interface CodecRuntime {
  VideoDecoder?: unknown;
  isSecureContext?: boolean;
  location?: Pick<Location, "protocol" | "host">;
}

function originLabel(location: CodecRuntime["location"]): string {
  if (!location) {
    return "unknown origin";
  }
  return `${location.protocol}//${location.host}`;
}

export function videoDecoderAvailability(
  runtime: CodecRuntime = globalThis,
): VideoDecoderAvailability {
  if (typeof runtime.VideoDecoder !== "undefined") {
    return {
      available: true,
      summary: "WebCodecs ready",
      detail: "VideoDecoder is available.",
    };
  }

  const origin = originLabel(runtime.location);
  if (runtime.isSecureContext === false) {
    return {
      available: false,
      summary: "WebCodecs unavailable",
      detail:
        `Encoded previews require VideoDecoder in a secure browser context; ` +
        `${origin} is not secure. Open the UI through localhost/127.0.0.1, ` +
        `HTTPS, or an SSH tunnel.`,
    };
  }

  return {
    available: false,
    summary: "WebCodecs unavailable",
    detail:
      `Encoded previews require VideoDecoder, but this browser/runtime does ` +
      `not expose it at ${origin}. Use a WebCodecs-capable browser.`,
  };
}
