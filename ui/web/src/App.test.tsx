import { render, screen, act } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import { createFakeWebSocketFactory } from "./test/fake-websocket";

function setViewport(width: number, height: number) {
  Object.defineProperty(window, "innerWidth", {
    configurable: true,
    writable: true,
    value: width,
  });
  Object.defineProperty(window, "innerHeight", {
    configurable: true,
    writable: true,
    value: height,
  });
}

function runtimeConfigStub() {
  return {
    controlWebsocketUrl: "ws://127.0.0.1:9091",
    previewWebsocketUrl: "ws://127.0.0.1:19090",
    episodeKeyBindings: {
      startKey: "s",
      stopKey: "e",
      keepKey: "k",
      discardKey: "x",
    },
  };
}

describe("App", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it("switches between wide and narrow layouts", () => {
    setViewport(1300, 900);
    const control = createFakeWebSocketFactory();
    const preview = createFakeWebSocketFactory();
    const { container } = render(
      <App
        runtimeConfig={runtimeConfigStub()}
        controlSocketOptions={{ websocketFactory: control.factory }}
        previewSocketOptions={{
          websocketFactory: preview.factory,
          objectUrlFactory: () => "blob:mock",
          revokeObjectUrl: vi.fn(),
        }}
      />,
    );

    expect(container.querySelector(".camera-layout--wide")).not.toBeNull();

    act(() => {
      setViewport(900, 900);
      window.dispatchEvent(new Event("resize"));
    });

    expect(container.querySelector(".camera-layout--narrow")).not.toBeNull();
  });

  it("episode keys go through the control socket and preview size goes through the preview socket", async () => {
    vi.useFakeTimers();
    setViewport(1300, 900);
    const control = createFakeWebSocketFactory();
    const preview = createFakeWebSocketFactory();

    render(
      <App
        runtimeConfig={runtimeConfigStub()}
        controlSocketOptions={{ websocketFactory: control.factory }}
        previewSocketOptions={{
          websocketFactory: preview.factory,
          objectUrlFactory: () => "blob:mock",
          revokeObjectUrl: vi.fn(),
        }}
      />,
    );

    await act(async () => {
      preview.sockets[0].open();
      control.sockets[0].open();
      await Promise.resolve();
      await vi.advanceTimersByTimeAsync(1);
    });

    expect(preview.sockets[0].sent).toContain(
      JSON.stringify({ type: "command", action: "get_stream_info" }),
    );
    expect(
      preview.sockets[0].sent.some((message) => {
        const parsed = JSON.parse(message) as {
          type: string;
          action: string;
          width?: number;
          height?: number;
        };
        return (
          parsed.type === "command" &&
          parsed.action === "set_preview_size" &&
          typeof parsed.width === "number" &&
          parsed.width > 0 &&
          typeof parsed.height === "number" &&
          parsed.height > 0
        );
      }),
    ).toBe(true);

    act(() => {
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "d" }));
    });
    expect(screen.getByText(/Debug \(press d to hide\)/)).toBeTruthy();

    act(() => {
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "s" }));
    });
    expect(control.sockets[0].sent).toContain(
      JSON.stringify({ type: "command", action: "episode_start" }),
    );
    // Preview socket must not see episode commands.
    expect(
      preview.sockets[0].sent.find((message) =>
        message.includes("episode_start"),
      ),
    ).toBeUndefined();
  });
});
