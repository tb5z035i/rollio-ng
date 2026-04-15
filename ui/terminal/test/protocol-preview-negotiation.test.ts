import assert from "node:assert/strict";
import test from "node:test";
import {
  encodeEpisodeCommand,
  encodeSetupCommand,
  encodeSetPreviewSize,
  parseBinaryMessage,
  parseJsonMessage,
} from "../src/lib/protocol.js";

test("encodeSetPreviewSize emits the websocket resize command", () => {
  assert.deepEqual(JSON.parse(encodeSetPreviewSize(320, 180)), {
    type: "command",
    action: "set_preview_size",
    width: 320,
    height: 180,
  });
});

test("parseBinaryMessage exposes negotiated preview dimensions", () => {
  const name = "camera_0";
  const nameBytes = Buffer.from(name, "utf-8");
  const jpegPayload = Buffer.from([1, 2, 3, 4]);
  const buffer = Buffer.alloc(1 + 2 + nameBytes.length + 8 + 8 + 4 + 4 + jpegPayload.length);
  let offset = 0;

  buffer.writeUInt8(0x01, offset);
  offset += 1;
  buffer.writeUInt16LE(nameBytes.length, offset);
  offset += 2;
  nameBytes.copy(buffer, offset);
  offset += nameBytes.length;
  buffer.writeBigUInt64LE(123n, offset);
  offset += 8;
  buffer.writeBigUInt64LE(456n, offset);
  offset += 8;
  buffer.writeUInt32LE(160, offset);
  offset += 4;
  buffer.writeUInt32LE(90, offset);
  offset += 4;
  jpegPayload.copy(buffer, offset);

  const message = parseBinaryMessage(buffer);

  assert.ok(message);
  assert.equal(message?.name, name);
  assert.equal(message?.previewWidth, 160);
  assert.equal(message?.previewHeight, 90);
  assert.deepEqual(Array.from(message?.jpegData ?? []), Array.from(jpegPayload));
});

test("encodeEpisodeCommand emits the expected websocket control message", () => {
  assert.deepEqual(JSON.parse(encodeEpisodeCommand("episode_keep")), {
    type: "command",
    action: "episode_keep",
  });
});

test("parseJsonMessage accepts episode status payloads", () => {
  const message = parseJsonMessage(
    JSON.stringify({
      type: "episode_status",
      state: "recording",
      episode_count: 3,
      elapsed_ms: 5250,
    }),
  );

  assert.deepEqual(message, {
    type: "episode_status",
    state: "recording",
    episode_count: 3,
    elapsed_ms: 5250,
  });
});

test("parseJsonMessage accepts robot state payloads with end-effector status", () => {
  const message = parseJsonMessage(
    JSON.stringify({
      type: "robot_state",
      name: "eef_g2",
      timestamp_ns: 123,
      num_joints: 1,
      positions: [0.042],
      velocities: [-0.1],
      efforts: [1.25],
      end_effector_status: "enabled",
      end_effector_feedback_valid: true,
    }),
  );

  assert.deepEqual(message, {
    type: "robot_state",
    name: "eef_g2",
    timestamp_ns: 123,
    num_joints: 1,
    positions: [0.042],
    velocities: [-0.1],
    efforts: [1.25],
    end_effector_status: "enabled",
    end_effector_feedback_valid: true,
  });
});

test("encodeSetupCommand emits setup websocket messages", () => {
  assert.deepEqual(
    JSON.parse(
      encodeSetupCommand("setup_toggle_device", {
        name: "camera_top",
        delta: 1,
      }),
    ),
    {
      type: "command",
      action: "setup_toggle_device",
      name: "camera_top",
      delta: 1,
    },
  );
});

test("encodeSetupCommand preserves value payloads for setup editors", () => {
  assert.deepEqual(
    JSON.parse(
      encodeSetupCommand("setup_set_project_name", {
        value: "demo_project",
      }),
    ),
    {
      type: "command",
      action: "setup_set_project_name",
      value: "demo_project",
    },
  );
});

test("encodeSetupCommand supports per-device name edits", () => {
  assert.deepEqual(
    JSON.parse(
      encodeSetupCommand("setup_set_device_name", {
        name: "robot|airbot-play|PZ123|-|-",
        value: "leader",
      }),
    ),
    {
      type: "command",
      action: "setup_set_device_name",
      name: "robot|airbot-play|PZ123|-|-",
      value: "leader",
    },
  );
});

test("parseJsonMessage accepts setup state payloads", () => {
  const message = parseJsonMessage(
    JSON.stringify({
      type: "setup_state",
      step: "devices",
      step_index: 1,
      step_name: "Devices",
      total_steps: 3,
      output_path: "config.toml",
      resume_mode: false,
      status: "editing",
      identify_device: "camera|realsense|123|color|-",
      warnings: [],
      config: {
        project_name: "default",
        mode: "intervention",
        episode: { format: "lerobot-v2.1" },
        devices: [],
        pairing: [],
        encoder: {
          video_codec: "h264",
          depth_codec: "rvl",
        },
        storage: { backend: "local", output_path: "./output" },
      },
      available_devices: [],
    }),
  );

  assert.deepEqual(message, {
    type: "setup_state",
    step: "devices",
    step_index: 1,
    step_name: "Devices",
    total_steps: 3,
    output_path: "config.toml",
    resume_mode: false,
    status: "editing",
    identify_device: "camera|realsense|123|color|-",
    warnings: [],
    config: {
      project_name: "default",
      mode: "intervention",
      episode: { format: "lerobot-v2.1" },
      devices: [],
      pairing: [],
      encoder: {
        video_codec: "h264",
        depth_codec: "rvl",
      },
      storage: { backend: "local", output_path: "./output" },
    },
    available_devices: [],
  });
});
