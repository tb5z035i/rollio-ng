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
        controlWebsocketUrl: "/ws/control",
        previewWebsocketUrl: "/ws/preview",
        episodeKeyBindings: {
          startKey: "s",
          stopKey: "e",
          keepKey: "k",
          discardKey: "x",
        },
      },
      httpLocation,
    );

    expect(runtimeConfig.controlWebsocketUrl).toBe(
      "ws://rollio.local:3000/ws/control",
    );
    expect(runtimeConfig.previewWebsocketUrl).toBe(
      "ws://rollio.local:3000/ws/preview",
    );
  });

  it("resolves same-origin websocket paths with wss on https pages", () => {
    expect(resolveWebSocketUrl("/ws/control", httpsLocation)).toBe(
      "wss://rollio.example/ws/control",
    );
  });

  it("normalizes absolute https websocket targets to wss", () => {
    expect(
      resolveWebSocketUrl("https://rollio.example/ws/preview", httpLocation),
    ).toBe("wss://rollio.example/ws/preview");
  });

  it("defaults mode to `collect` when the field is absent (backward compat)", () => {
    const runtimeConfig = normalizeRuntimeConfig(
      {
        controlWebsocketUrl: "/ws/control",
        previewWebsocketUrl: "/ws/preview",
        episodeKeyBindings: {
          startKey: "s",
          stopKey: "e",
          keepKey: "k",
          discardKey: "x",
        },
      },
      httpLocation,
    );
    expect(runtimeConfig.mode).toBe("collect");
  });

  it("preserves an explicit `setup` mode from the runtime config endpoint", () => {
    const runtimeConfig = normalizeRuntimeConfig(
      {
        mode: "setup",
        controlWebsocketUrl: "/ws/control",
        previewWebsocketUrl: "/ws/preview",
        episodeKeyBindings: {
          startKey: "s",
          stopKey: "e",
          keepKey: "k",
          discardKey: "x",
        },
      },
      httpLocation,
    );
    expect(runtimeConfig.mode).toBe("setup");
  });

  it("rejects an unknown mode string", () => {
    expect(() =>
      normalizeRuntimeConfig(
        {
          mode: "wizard",
          controlWebsocketUrl: "/ws/control",
          previewWebsocketUrl: "/ws/preview",
          episodeKeyBindings: {
            startKey: "s",
            stopKey: "e",
            keepKey: "k",
            discardKey: "x",
          },
        },
        httpLocation,
      ),
    ).toThrow(/mode/);
  });

  it("loads runtime config from the backend endpoint", async () => {
    const fetchImpl = vi.fn<typeof fetch>().mockResolvedValue(
      new Response(
        JSON.stringify({
          controlWebsocketUrl: "/ws/control",
          previewWebsocketUrl: "/ws/preview",
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
    expect(runtimeConfig.controlWebsocketUrl).toBe(
      "ws://rollio.local:3000/ws/control",
    );
    expect(runtimeConfig.previewWebsocketUrl).toBe(
      "ws://rollio.local:3000/ws/preview",
    );
  });
});
