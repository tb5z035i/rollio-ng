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
});
