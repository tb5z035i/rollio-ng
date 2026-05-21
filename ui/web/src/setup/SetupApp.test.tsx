import { render, screen, act, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import SetupApp from "./SetupApp";
import { createFakeWebSocketFactory } from "../test/fake-websocket";
import type { SetupStateMessage } from "../lib/protocol";

function runtimeConfigStub() {
  return {
    mode: "setup" as const,
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

function devicesStepSnapshot(): SetupStateMessage {
  return {
    type: "setup_state",
    step: "devices",
    step_index: 0,
    step_name: "Devices",
    total_steps: 5,
    output_path: "/tmp/config.toml",
    resume_mode: false,
    status: "editing",
    warnings: [],
    identify_device: null,
    subpanel_target: null,
    available_devices: [
      {
        name: "cam0",
        display_name: "Pseudo Camera 0",
        device_type: "camera",
        driver: "rollio-device-pseudo",
        id: "pseudo_camera_0",
        camera_profiles: [],
        supported_modes: [],
        current: {
          name: "cam0",
          driver: "rollio-device-pseudo",
          id: "pseudo_camera_0",
          bus_root: "cam0",
          channels: [
            {
              channel_type: "color",
              kind: "camera",
              enabled: true,
              preview_enabled: true,
              record_enabled: true,
            },
          ],
        },
      },
    ],
    config: {
      project_name: "test",
      mode: "intervention",
      episode: { format: "lerobot-v2.1", fps: 30 },
      devices: [],
      pairings: [],
      storage: { backend: "local", output_path: "./out" },
    },
  };
}

describe("SetupApp", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it("shows the connecting screen until the first setup_state arrives", () => {
    vi.useFakeTimers();
    const { sockets, factory } = createFakeWebSocketFactory();
    render(
      <SetupApp
        runtimeConfig={runtimeConfigStub()}
        controlSocketOptions={{ websocketFactory: factory }}
      />,
    );
    expect(
      screen.getByText(/Connecting to controller/i),
    ).toBeDefined();
    expect(sockets).toHaveLength(1);
  });

  it("renders the stepper and devices step once setup_state arrives", () => {
    vi.useFakeTimers();
    const { sockets, factory } = createFakeWebSocketFactory();
    render(
      <SetupApp
        runtimeConfig={runtimeConfigStub()}
        controlSocketOptions={{ websocketFactory: factory }}
      />,
    );

    act(() => {
      sockets[0].open();
    });
    // The connected hook fires `setup_get_state` on open.
    expect(sockets[0].sent).toContain(
      JSON.stringify({ type: "command", action: "setup_get_state" }),
    );

    act(() => {
      sockets[0].emitMessage(JSON.stringify(devicesStepSnapshot()));
      vi.advanceTimersByTime(64); // batch interval
    });

    expect(screen.getByRole("heading", { name: /Devices/i })).toBeDefined();
    // Step indicator shows "Step 1 / 5".
    expect(screen.getByText(/Step 1 \/ 5/)).toBeDefined();
  });

  it("dispatches setup_next_step when the operator clicks the States chip", () => {
    vi.useFakeTimers();
    const { sockets, factory } = createFakeWebSocketFactory();
    render(
      <SetupApp
        runtimeConfig={runtimeConfigStub()}
        controlSocketOptions={{ websocketFactory: factory }}
      />,
    );
    act(() => {
      sockets[0].open();
      sockets[0].emitMessage(JSON.stringify(devicesStepSnapshot()));
      vi.advanceTimersByTime(64);
    });

    // Stepper chip click → setup_jump_step value=states
    const stepper = screen.getByText("Step 1 / 5").parentElement!;
    const statesChip = within(stepper).getByRole("button", { name: /States/ });
    act(() => {
      statesChip.click();
    });
    expect(sockets[0].sent).toContain(
      JSON.stringify({
        type: "command",
        action: "setup_jump_step",
        value: "states",
      }),
    );
  });

  it("dispatches setup_toggle_device when the operator unchecks a row", () => {
    vi.useFakeTimers();
    const { sockets, factory } = createFakeWebSocketFactory();
    render(
      <SetupApp
        runtimeConfig={runtimeConfigStub()}
        controlSocketOptions={{ websocketFactory: factory }}
      />,
    );
    act(() => {
      sockets[0].open();
      sockets[0].emitMessage(JSON.stringify(devicesStepSnapshot()));
      vi.advanceTimersByTime(64);
    });

    // The device-row checkbox is the first checkbox in the table.
    const checkboxes = screen.getAllByRole("checkbox");
    expect(checkboxes.length).toBeGreaterThan(0);
    act(() => {
      checkboxes[0].click();
    });
    expect(sockets[0].sent).toContain(
      JSON.stringify({
        type: "command",
        action: "setup_toggle_device",
        name: "cam0",
      }),
    );
  });
});
