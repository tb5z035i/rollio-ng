import { describe, expect, it } from "vitest";
import {
  encodeEpisodeCommand,
  encodeSetPreviewSize,
  parseBinaryMessage,
  parseJsonMessage,
} from "./protocol";

describe("protocol", () => {
  it("encodes preview resize commands", () => {
    expect(JSON.parse(encodeSetPreviewSize(320, 180))).toEqual({
      type: "command",
      action: "set_preview_size",
      width: 320,
      height: 180,
    });
  });

  it("parses binary JPEG preview frames", () => {
    const name = "camera_0";
    const nameBytes = new TextEncoder().encode(name);
    const jpegPayload = new Uint8Array([1, 2, 3, 4]);
    const buffer = new ArrayBuffer(1 + 2 + nameBytes.length + 8 + 8 + 4 + 4 + jpegPayload.length);
    const view = new DataView(buffer);
    let offset = 0;

    view.setUint8(offset, 0x01);
    offset += 1;
    view.setUint16(offset, nameBytes.length, true);
    offset += 2;
    new Uint8Array(buffer, offset, nameBytes.length).set(nameBytes);
    offset += nameBytes.length;
    view.setBigUint64(offset, 123n, true);
    offset += 8;
    view.setBigUint64(offset, 456n, true);
    offset += 8;
    view.setUint32(offset, 160, true);
    offset += 4;
    view.setUint32(offset, 90, true);
    offset += 4;
    new Uint8Array(buffer, offset, jpegPayload.length).set(jpegPayload);

    expect(parseBinaryMessage(buffer)).toEqual({
      type: "camera_frame",
      name,
      timestampNs: 123,
      frameIndex: 456,
      previewWidth: 160,
      previewHeight: 90,
      jpegData: jpegPayload,
    });
  });

  it("encodes episode commands", () => {
    expect(JSON.parse(encodeEpisodeCommand("episode_keep"))).toEqual({
      type: "command",
      action: "episode_keep",
    });
  });

  it("parses robot_state JSON in the visualizer wire format", () => {
    expect(
      parseJsonMessage(
        JSON.stringify({
          type: "robot_state",
          name: "airbot_play_arm",
          timestamp_us: 999_000,
          num_joints: 6,
          values: [0.1, 0.2, 0.3, 0.4, 0.5, 0.6],
          state_kind: "joint_position",
        }),
      ),
    ).toMatchObject({
      type: "robot_state",
      name: "airbot_play_arm",
      timestamp_us: 999_000,
      num_joints: 6,
      values: [0.1, 0.2, 0.3, 0.4, 0.5, 0.6],
      state_kind: "joint_position",
    });
  });

  it("parses episode status JSON", () => {
    expect(
      parseJsonMessage(
        JSON.stringify({
          type: "episode_status",
          state: "recording",
          episode_count: 3,
          elapsed_ms: 5250,
        }),
      ),
    ).toEqual({
      type: "episode_status",
      state: "recording",
      episode_count: 3,
      elapsed_ms: 5250,
    });
  });
});
