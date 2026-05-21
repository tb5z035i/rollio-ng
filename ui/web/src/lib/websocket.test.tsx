import { renderHook, act } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { createFakeWebSocketFactory } from "../test/fake-websocket";
import {
  reconnectDelayMs,
  useControlSocket,
  usePreviewSocket,
} from "./websocket";

describe("useControlSocket", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it("reconnects after the control socket closes", () => {
    vi.useFakeTimers();
    const { sockets, factory } = createFakeWebSocketFactory();
    const { result } = renderHook(() =>
      useControlSocket("ws://127.0.0.1:9091", { websocketFactory: factory }),
    );

    expect(sockets).toHaveLength(1);

    act(() => {
      sockets[0].open();
    });
    expect(result.current.connected).toBe(true);

    act(() => {
      sockets[0].close();
    });
    expect(result.current.connected).toBe(false);

    act(() => {
      vi.advanceTimersByTime(reconnectDelayMs(0));
    });
    expect(sockets).toHaveLength(2);

    act(() => {
      sockets[1].open();
    });
    expect(result.current.connected).toBe(true);
  });

  it("surfaces `setup_state` messages alongside `episode_status`", () => {
    vi.useFakeTimers();
    const { sockets, factory } = createFakeWebSocketFactory();
    const { result } = renderHook(() =>
      useControlSocket("ws://127.0.0.1:9091", { websocketFactory: factory }),
    );

    act(() => {
      sockets[0].open();
    });
    expect(result.current.setupState).toBeNull();

    act(() => {
      sockets[0].emitMessage(
        JSON.stringify({
          type: "setup_state",
          step: "devices",
          step_index: 0,
          step_name: "Devices",
          total_steps: 5,
          output_path: "/tmp/config.toml",
          resume_mode: false,
          status: "editing",
          warnings: [],
          available_devices: [],
          config: {
            project_name: "test",
            mode: "intervention",
            episode: { format: "lerobot-v2.1", fps: 30 },
            devices: [],
            pairings: [],
            storage: { backend: "local", output_path: "./out" },
          },
        }),
      );
      // The hook batches state updates on a 16ms interval; advance
      // through the next flush so React commits the new state.
      vi.advanceTimersByTime(64);
    });

    expect(result.current.setupState?.step).toBe("devices");
    expect(result.current.setupState?.total_steps).toBe(5);
  });
});

describe("usePreviewSocket", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it("reconnects and requests stream info on every open", () => {
    vi.useFakeTimers();
    const { sockets, factory } = createFakeWebSocketFactory();
    const options = {
      websocketFactory: factory,
      objectUrlFactory: () => "blob:mock",
      revokeObjectUrl: vi.fn(),
    };

    const { result } = renderHook(() =>
      usePreviewSocket("ws://127.0.0.1:19090", options),
    );

    expect(sockets).toHaveLength(1);
    act(() => {
      sockets[0].open();
    });
    expect(result.current.connected).toBe(true);
    expect(sockets[0].sent).toContain(
      JSON.stringify({ type: "command", action: "get_stream_info" }),
    );

    act(() => {
      sockets[0].close();
    });
    expect(result.current.connected).toBe(false);

    act(() => {
      vi.advanceTimersByTime(reconnectDelayMs(0));
    });
    expect(sockets).toHaveLength(2);

    act(() => {
      sockets[1].open();
    });
    expect(result.current.connected).toBe(true);
    expect(sockets[1].sent).toContain(
      JSON.stringify({ type: "command", action: "get_stream_info" }),
    );
  });

  it("does not open a socket when `enabled: false`", () => {
    const { sockets, factory } = createFakeWebSocketFactory();
    const options = {
      websocketFactory: factory,
      objectUrlFactory: () => "blob:mock",
      revokeObjectUrl: vi.fn(),
      enabled: false,
    };
    renderHook(() =>
      usePreviewSocket("ws://127.0.0.1:19090", options),
    );
    expect(sockets).toHaveLength(0);
  });
});
