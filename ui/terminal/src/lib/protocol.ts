/**
 * WebSocket protocol message types and parsers.
 *
 * Binary messages (Visualizer → UI): camera frames
 * Text/JSON messages (both directions): robot state, commands
 */

/** Parsed camera frame from a binary WebSocket message. */
export interface CameraFrameMessage {
  type: "camera_frame";
  name: string;
  timestampNs: number;
  frameIndex: number;
  previewWidth: number;
  previewHeight: number;
  jpegData: Buffer;
}

/** Parsed robot state from a JSON WebSocket message. */
export type EndEffectorStatus = "unknown" | "disabled" | "enabled";

/**
 * State-kind tag emitted by the visualizer on each `robot_state` message.
 *
 * The visualizer publishes one message per (channel, state_kind) pair so the
 * UI can group rows belonging to the same channel into a single panel
 * (joint position + velocity + effort + optional EE pose / parallel gripper
 * channels). Lists every kind the rollio backend currently emits.
 */
export type RobotStateKind =
  | "joint_position"
  | "joint_velocity"
  | "joint_effort"
  | "end_effector_pose"
  | "end_effector_twist"
  | "end_effector_wrench"
  | "parallel_position"
  | "parallel_velocity"
  | "parallel_effort";

/**
 * Single-state-kind sample emitted by the visualizer. The UI aggregates these
 * by `name` into a {@link AggregatedRobotChannel} keyed by channel id and
 * keyed-on `state_kind` entries.
 */
export interface RobotStateMessage {
  type: "robot_state";
  name: string;
  /** Backend timestamp in microseconds (visualizer wire format). */
  timestamp_us: number;
  num_joints: number;
  /** Element values for `state_kind`. Unit depends on the kind: rad / rad·s⁻¹
   *  / Nm for joint kinds, m / m·s⁻¹ / N for parallel kinds, mixed for poses.
   */
  values: number[];
  state_kind: RobotStateKind;
  /** Optional per-element envelope reported by the device driver.
   *  Empty arrays mean "no driver-reported limits" — the UI should fall back
   *  to sensible defaults (see RobotStatePanel). */
  value_min?: number[];
  value_max?: number[];
  end_effector_status?: EndEffectorStatus;
  end_effector_feedback_valid?: boolean;
}

/** Metadata about websocket-published streams from the visualizer. */
export interface StreamInfoCamera {
  name: string;
  source_width: number | null;
  source_height: number | null;
  latest_timestamp_ns: number | null;
  latest_frame_index: number | null;
  source_fps_estimate: number | null;
  published_fps_estimate: number | null;
  last_published_timestamp_ns: number | null;
}

export interface StreamInfoMessage {
  type: "stream_info";
  server_timestamp_ns: number;
  configured_preview_fps: number;
  max_preview_width: number;
  max_preview_height: number;
  active_preview_width: number;
  active_preview_height: number;
  preview_workers: number;
  jpeg_quality: number;
  cameras: StreamInfoCamera[];
  robots: string[];
}

/** Command sent from UI to Visualizer. */
export interface EpisodeStatusMessage {
  type: "episode_status";
  state: "idle" | "recording" | "pending";
  episode_count: number;
  elapsed_ms: number;
}

export interface SetupCameraProfile {
  width: number;
  height: number;
  fps: number;
  pixel_format: string;
  native_pixel_format?: string | null;
  stream: string | null;
  channel: number | null;
}

/** Single channel on a device-binary entry (matches rollio-types DeviceChannelConfigV2 JSON). */
export interface SetupDeviceChannelV2 {
  channel_type: string;
  kind: "camera" | "robot";
  enabled?: boolean;
  /** User-facing per-channel name. Editing the row mutates this field
   * (instead of the parent device's `name`/`bus_root`). May be absent in
   * very old configs. */
  name?: string | null;
  /** Display label provided by the device executable's query response
   * (e.g., "AIRBOT E2", "V4L2 Camera"). Falls back to the device-level
   * `display_name` when missing. */
  channel_label?: string | null;
  mode?: "free-drive" | "command-following" | "identifying" | null;
  dof?: number | null;
  publish_states?: string[];
  recorded_states?: string[];
  control_frequency_hz?: number | null;
  profile?: {
    width: number;
    height: number;
    fps: number;
    pixel_format: string;
    native_pixel_format?: string | null;
  } | null;
}

/** Device row in setup wizard (rollio-types BinaryDeviceConfig). */
export interface SetupBinaryDeviceConfig {
  name: string;
  executable?: string | null;
  driver: string;
  id: string;
  bus_root: string;
  channels: SetupDeviceChannelV2[];
  extra?: Record<string, unknown>;
}

export type MappingPolicy = "direct-joint" | "cartesian" | "parallel";

export interface SetupChannelPairing {
  leader_device: string;
  leader_channel_type: string;
  follower_device: string;
  follower_channel_type: string;
  mapping: MappingPolicy;
  leader_state: string;
  follower_command: string;
  joint_index_map: number[];
  joint_scales: number[];
}

export interface SetupDirectJointPeer {
  driver: string;
  channel_type: string;
}

export interface SetupDirectJointCompatibility {
  can_lead?: SetupDirectJointPeer[];
  can_follow?: SetupDirectJointPeer[];
}

export interface SetupAvailableDevice {
  name: string;
  display_name: string;
  device_type: "camera" | "robot";
  driver: string;
  id: string;
  camera_profiles: SetupCameraProfile[];
  supported_modes: Array<"free-drive" | "command-following" | "identifying">;
  /** All robot state kinds the driver advertises it can publish on this
   *  channel. The "States" sub-step uses this list to render the
   *  toggleable publish/recorded options. Empty for camera channels. */
  supported_states?: string[];
  /** All robot command kinds the driver accepts on this channel. The
   *  "Pairing" picker uses this to filter follower candidates per
   *  policy. Empty for camera channels. */
  supported_commands?: string[];
  /** Driver-advertised direct-joint compatibility whitelist; the
   *  pairing picker uses this to enforce the two-sided whitelist for
   *  DirectJoint pairs. */
  direct_joint_compatibility?: SetupDirectJointCompatibility;
  current: SetupBinaryDeviceConfig;
}

export type EncoderCodec = "h264" | "h265" | "av1" | "rvl";
export type EncoderBackend = "auto" | "cpu" | "nvidia" | "vaapi";

export interface SetupConfigSnapshot {
  project_name: string;
  mode: "teleop" | "intervention";
  episode: {
    format: "lerobot-v2.1" | "lerobot-v3.0" | "mcap";
  };
  /** Native project layout: one binary per row with nested channels. */
  devices: SetupBinaryDeviceConfig[];
  pairings: SetupChannelPairing[];
  encoder: {
    video_codec: EncoderCodec;
    depth_codec: EncoderCodec;
    /** Legacy global backend hint (kept for backwards compatibility). The
     *  wizard now drives the per-codec backend via `video_backend` and
     *  `depth_backend`; this field is the shared default the controller
     *  falls back to when those are unset. */
    backend?: EncoderBackend;
    /** Backend used to encode color/IR streams (paired with `video_codec`). */
    video_backend?: EncoderBackend;
    /** Backend used to encode depth streams (paired with `depth_codec`). */
    depth_backend?: EncoderBackend;
  };
  storage: {
    backend: "local" | "http";
    output_path: string;
    endpoint?: string | null;
  };
  /** Browser UI server runtime config. Defaults are filled in by the
   *  controller when absent in the saved TOML. */
  ui?: {
    http_host: string;
    http_port: number;
  };
}

export interface SetupStateMessage {
  type: "setup_state";
  step: "devices" | "states" | "pairing" | "storage" | "preview";
  step_index: number;
  step_name: string;
  total_steps: number;
  output_path: string;
  resume_mode: boolean;
  status: "editing" | "saved" | "cancelled";
  message?: string;
  identify_device?: string | null;
  warnings: string[];
  config: SetupConfigSnapshot;
  available_devices: SetupAvailableDevice[];
}

export type CommandAction =
  | "get_stream_info"
  | "set_preview_size"
  | "episode_start"
  | "episode_stop"
  | "episode_keep"
  | "episode_discard"
  | "setup_get_state"
  | "setup_prev_step"
  | "setup_next_step"
  | "setup_jump_step"
  | "setup_toggle_device"
  | "setup_set_device_name"
  | "setup_toggle_identify"
  | "setup_cycle_camera_profile"
  | "setup_cycle_robot_mode"
  | "setup_cycle_pair_mapping"
  | "setup_create_pairing"
  | "setup_remove_pairing"
  | "setup_set_pairing_leader"
  | "setup_set_pairing_follower"
  | "setup_set_pairing_ratio"
  | "setup_toggle_publish_state"
  | "setup_toggle_recorded_state"
  | "setup_cycle_episode_format"
  | "setup_cycle_storage_backend"
  | "setup_cycle_collection_mode"
  | "setup_cycle_video_codec"
  | "setup_cycle_depth_codec"
  | "setup_set_project_name"
  | "setup_set_storage_output_path"
  | "setup_set_storage_endpoint"
  | "setup_set_ui_http_host"
  | "setup_save"
  | "setup_cancel";

export interface CommandMessage {
  type: "command";
  action: CommandAction;
  width?: number;
  height?: number;
  name?: string;
  index?: number;
  delta?: number;
  value?: string;
}

/** Frame encoding type tags. */
const FRAME_TYPE_JPEG = 0x01;

/**
 * Parse a binary WebSocket message into a CameraFrameMessage.
 *
 * Binary protocol:
 *   Byte 0:       frame encoding type (0x01 = JPEG)
 *   Bytes 1-2:    camera name length (u16 LE)
 *   Bytes 3..N:   camera name (UTF-8)
 *   Bytes N+1..8: original source timestamp_ns (u64 LE)
 *   Bytes N+9..16: original source frame_index (u64 LE)
 *   Bytes N+17..20: encoded preview width (u32 LE)
 *   Bytes N+21..24: encoded preview height (u32 LE)
 *   Remaining:    JPEG payload
 */
export function parseBinaryMessage(
  data: Buffer,
): CameraFrameMessage | null {
  if (data.length < 1 + 2) return null;

  const typeTag = data[0];
  if (typeTag !== FRAME_TYPE_JPEG) return null;

  const nameLen = data.readUInt16LE(1);
  const headerStart = 3 + nameLen;
  const headerEnd = headerStart + 8 + 8 + 4 + 4;
  if (data.length < headerEnd) return null;

  const name = data.subarray(3, 3 + nameLen).toString("utf-8");
  const timestampNs = Number(data.readBigUInt64LE(headerStart));
  const frameIndex = Number(data.readBigUInt64LE(headerStart + 8));
  const width = data.readUInt32LE(headerStart + 16);
  const height = data.readUInt32LE(headerStart + 20);
  // Reuse the incoming WebSocket buffer instead of copying the JPEG payload.
  const jpegData = data.subarray(headerEnd);

  return {
    type: "camera_frame",
    name,
    timestampNs,
    frameIndex,
    previewWidth: width,
    previewHeight: height,
    jpegData,
  };
}

/**
 * Parse a JSON WebSocket text message from the visualizer.
 */
export function parseJsonMessage(
  text: string,
): RobotStateMessage | StreamInfoMessage | EpisodeStatusMessage | SetupStateMessage | null {
  try {
    const obj = JSON.parse(text);
    if (obj && obj.type === "robot_state") {
      return obj as RobotStateMessage;
    }
    if (obj && obj.type === "stream_info") {
      return obj as StreamInfoMessage;
    }
    if (obj && obj.type === "episode_status") {
      return obj as EpisodeStatusMessage;
    }
    if (obj && obj.type === "setup_state") {
      return obj as SetupStateMessage;
    }
    return null;
  } catch {
    return null;
  }
}

/**
 * Encode a command message as JSON for sending to the Visualizer.
 */
export function encodeCommand(
  action: CommandAction,
  fields: Partial<
    Pick<CommandMessage, "width" | "height" | "name" | "index" | "delta" | "value">
  > = {},
): string {
  return JSON.stringify({ type: "command", action, ...fields });
}

export function encodeSetPreviewSize(width: number, height: number): string {
  return encodeCommand("set_preview_size", { width, height });
}

export function encodeEpisodeCommand(action: Extract<
  CommandAction,
  "episode_start" | "episode_stop" | "episode_keep" | "episode_discard"
>): string {
  return encodeCommand(action);
}

export function encodeSetupCommand(
  action: Extract<
    CommandAction,
    | "setup_get_state"
    | "setup_prev_step"
    | "setup_next_step"
    | "setup_jump_step"
    | "setup_toggle_device"
    | "setup_set_device_name"
    | "setup_toggle_identify"
    | "setup_cycle_camera_profile"
    | "setup_cycle_robot_mode"
    | "setup_cycle_pair_mapping"
    | "setup_create_pairing"
    | "setup_remove_pairing"
    | "setup_set_pairing_leader"
    | "setup_set_pairing_follower"
    | "setup_set_pairing_ratio"
    | "setup_toggle_publish_state"
    | "setup_toggle_recorded_state"
    | "setup_cycle_episode_format"
    | "setup_cycle_storage_backend"
    | "setup_cycle_collection_mode"
    | "setup_cycle_video_codec"
    | "setup_cycle_depth_codec"
    | "setup_set_project_name"
    | "setup_set_storage_output_path"
    | "setup_set_storage_endpoint"
    | "setup_set_ui_http_host"
    | "setup_save"
    | "setup_cancel"
  >,
  fields: Partial<Pick<CommandMessage, "name" | "index" | "delta" | "value">> = {},
): string {
  return encodeCommand(action, fields);
}
