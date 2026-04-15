import { describe, expect, it, vi } from "vitest";
import {
  loadRuntimeConfig,
  normalizeRuntimeConfig,
  resolveWebSocketUrl,
} from "./runtime-config";

const httpLocation = {
  protocol: "http:",
  host: "rollio.local:3000",
} satisfies Pick<Location, "protocol" | "host">;

const httpsLocation = {
  protocol: "https:",
  host: "rollio.example",
} satisfies Pick<Location, "protocol" | "host">;

describe("runtime config", () => {
  it("resolves same-origin websocket paths with ws on http pages", () => {
    const runtimeConfig = normalizeRuntimeConfig(
      {
        websocketUrl: "/ws",
        episodeKeyBindings: {
          startKey: "s",
          stopKey: "e",
          keepKey: "k",
          discardKey: "x",
        },
      },
      httpLocation,
    );

    expect(runtimeConfig.websocketUrl).toBe("ws://rollio.local:3000/ws");
  });

  it("resolves same-origin websocket paths with wss on https pages", () => {
    expect(resolveWebSocketUrl("/ws", httpsLocation)).toBe("wss://rollio.example/ws");
  });

  it("normalizes absolute https websocket targets to wss", () => {
    expect(resolveWebSocketUrl("https://rollio.example/ws", httpLocation)).toBe(
      "wss://rollio.example/ws",
    );
  });

  it("loads runtime config from the backend endpoint", async () => {
    const fetchImpl = vi.fn<typeof fetch>().mockResolvedValue(
      new Response(
        JSON.stringify({
          websocketUrl: "/ws",
          episodeKeyBindings: {
            startKey: "s",
            stopKey: "e",
            keepKey: "k",
            discardKey: "x",
          },
        }),
        {
          status: 200,
          headers: {
            "Content-Type": "application/json",
          },
        },
      ),
    );

    const runtimeConfig = await loadRuntimeConfig(fetchImpl, httpLocation);

    expect(fetchImpl).toHaveBeenCalledWith(
      "/api/runtime-config",
      expect.objectContaining({
        cache: "no-store",
      }),
    );
    expect(runtimeConfig.websocketUrl).toBe("ws://rollio.local:3000/ws");
  });
});
