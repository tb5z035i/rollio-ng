const FRAME_TYPE_JPEG = 0x01;
const FRAME_TYPE_ENCODED_PACKET = 0x03;
const textDecoder = new TextDecoder("utf-8");

export const CODEC_NAMES: Record<number, string> = {
  0: "h264",
  1: "h265",
  2: "av1",
  3: "rvl",
  4: "mjpg",
};

export function codecName(codecId: number): string {
  return CODEC_NAMES[codecId] ?? `codec ${codecId}`;
}

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

export interface CameraFrameMessage {
  type: "camera_frame";
  name: string;
  timestampNs: number;
  frameIndex: number;
  previewWidth: number;
  previewHeight: number;
  jpegData: Uint8Array;
}

/** One encoded preview packet (kind 0x03). Payload is the
 *  codec-specific access unit (Annex B AU for H.264, RVL frame for
 *  RVL, etc.). Keyframes carry inline SPS/PPS so the frontend's
 *  WebCodecs decoder auto-configures from the first key packet
 *  alone — there is no separate config message. */
export interface EncodedPacketMessage {
  type: "encoded_packet";
  name: string;
  codecId: number;
  isKeyframe: boolean;
  /** Codec PTS in µs, monotonic from recording start. Used by WebCodecs
   *  for decoder ordering. */
  ptsUs: number;
  sequence: number;
  /** Camera capture wall-clock µs since UNIX epoch — propagated by the
   *  encoder unchanged. Use this (not `ptsUs`) for end-to-end latency
   *  metrics that compare against `Date.now()`. */
  sourceTimestampUs: number;
  /** Coded width — needed to configure the WebCodecs decoder on the
   *  first keyframe per camera. */
  width: number;
  height: number;
  payload: Uint8Array;
}

export type BinaryWsMessage = CameraFrameMessage | EncodedPacketMessage;

/**
 * Single-state-kind sample emitted by the visualizer. The UI aggregates
 * messages with the same `name` into one channel block keyed on
 * `state_kind` so joint position / velocity / effort rows for the same arm
 * collapse into a single visual panel.
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
   *  Empty / missing means "no driver-reported limits". */
  value_min?: number[];
  value_max?: number[];
  end_effector_status?: EndEffectorStatus;
  end_effector_feedback_valid?: boolean;
}

export interface StreamInfoCamera {
  name: string;
  source_width: number | null;
  source_height: number | null;
  preview_resizable?: boolean;
  preview_resize_policy?: "dynamic" | "fixed-source";
  latest_timestamp_ms: number | null;
  latest_frame_index: number | null;
  received_fps_estimate: number | null;
  bytes_per_sec: number | null;
  keyframe_age_ms: number | null;
  /**
   * True when the active encoder's output dims are pinned to source dims
   * (passthrough mode). UI must not send `set_preview_size` against the
   * preview stream while this is set; the encoder would reject any size
   * other than the source size and clutter the visualizer log.
   */
  scaling_locked?: boolean;
}

export interface StreamInfoMessage {
  type: "stream_info";
  server_timestamp_ms: number;
  /** Visualizer's preview output mode. */
  preview_output_mode: "jpeg" | "encoded";
  active_preview_width: number;
  active_preview_height: number;
  cameras: StreamInfoCamera[];
  robots: string[];
}

export interface EpisodeStatusMessage {
  type: "episode_status";
  state: "idle" | "recording" | "pending";
  episode_count: number;
  elapsed_ms: number;
}

// ---------------------------------------------------------------------------
// Setup wizard types.
//
// Mirror the wire shape emitted by the Rust controller's `SetupSession`
// (`controller/src/setup/state.rs`). The same JSON envelopes drive both the
// Ink terminal UI and the web SPA — these definitions are lifted from
// `ui/terminal/src/lib/protocol.ts` so the two backends stay byte-compatible.
// ---------------------------------------------------------------------------

export interface SetupCameraProfile {
  width: number;
  height: number;
  fps: number;
  pixel_format: string;
  native_pixel_format?: string | null;
  stream: string | null;
  channel: number | null;
}

export interface SetupDeviceChannelV2 {
  channel_type: string;
  kind: "camera" | "robot";
  enabled?: boolean;
  name?: string | null;
  channel_label?: string | null;
  mode?: "free-drive" | "command-following" | "identifying" | null;
  dof?: number | null;
  publish_states?: string[];
  recorded_states?: string[];
  control_frequency_hz?: number | null;
  preview_enabled?: boolean;
  record_enabled?: boolean;
  profile?: {
    width: number;
    height: number;
    fps: number;
    pixel_format: string;
    native_pixel_format?: string | null;
  } | null;
  record?: {
    video_codec?: string;
    depth_codec?: string;
    backend?: string;
    video_backend?: string;
    depth_backend?: string;
    chroma_subsampling?: string;
    crf?: number | null;
    preset?: string | null;
    tune?: string | null;
    bit_depth?: number;
    color_space?: string;
    queue_size?: number;
  } | null;
  preview_config?: {
    output_mode?: string;
    color_codec?: string;
    depth_codec?: string;
    backend?: string;
    width?: number;
    height?: number;
    fps?: number;
    gop_seconds?: number;
    crf?: number | null;
    jpeg_quality?: number;
  } | null;
}

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
  supported_states?: string[];
  supported_commands?: string[];
  direct_joint_compatibility?: SetupDirectJointCompatibility;
  current: SetupBinaryDeviceConfig;
}

export interface SetupConfigSnapshot {
  project_name: string;
  mode: "teleop" | "intervention";
  episode: {
    format: "lerobot-v2.1" | "lerobot-v3.0" | "mcap";
    fps: number;
    chunk_size?: number;
  };
  devices: SetupBinaryDeviceConfig[];
  pairings: SetupChannelPairing[];
  controller?: {
    shutdown_timeout_ms: number;
    child_poll_interval_ms: number;
  };
  visualizer?: {
    port: number;
  };
  assembler?: {
    missing_eos_timeout_ms: number;
    staging_dir: string;
    staging_slots: number;
  };
  storage: {
    backend: "local" | "http" | "dataloop";
    output_path: string;
    endpoint?: string | null;
    queue_size?: number;
    dataloop_project_id?: string | null;
    dataloop_token?: string | null;
  };
  monitor?: {
    metrics_frequency_hz: number;
  };
  ui?: {
    http_host: string;
    http_port: number;
    start_key?: string;
    stop_key?: string;
    keep_key?: string;
    discard_key?: string;
  };
}

export type SetupStep = "devices" | "states" | "pairing" | "storage" | "preview";

export interface SetupStateMessage {
  type: "setup_state";
  step: SetupStep;
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
  subpanel_target?: string | null;
}

export type SetupCommandAction =
  | "setup_get_state"
  | "setup_prev_step"
  | "setup_next_step"
  | "setup_jump_step"
  | "setup_toggle_device"
  | "setup_set_device_name"
  | "setup_toggle_identify"
  | "setup_open_subpanel"
  | "setup_close_subpanel"
  | "setup_subpanel_toggle_preview_enabled"
  | "setup_subpanel_toggle_record_enabled"
  | "setup_subpanel_cycle_primary"
  | "setup_subpanel_cycle_record_field"
  | "setup_subpanel_set_record_field"
  | "setup_subpanel_cycle_preview_field"
  | "setup_subpanel_set_preview_field"
  | "setup_subpanel_set_control_frequency_hz"
  | "setup_open_add_picker"
  | "setup_add_pseudo_camera"
  | "setup_add_pseudo_robot"
  | "setup_add_command_device"
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
  | "setup_set_project_name"
  | "setup_set_storage_output_path"
  | "setup_set_storage_endpoint"
  | "setup_set_dataloop_project_id"
  | "setup_set_dataloop_token"
  | "setup_set_ui_http_host"
  | "setup_set_ui_http_port"
  | "setup_set_ui_start_key"
  | "setup_set_ui_stop_key"
  | "setup_set_ui_keep_key"
  | "setup_set_ui_discard_key"
  | "setup_set_episode_fps"
  | "setup_set_episode_chunk_size"
  | "setup_set_controller_shutdown_timeout_ms"
  | "setup_set_controller_child_poll_interval_ms"
  | "setup_set_visualizer_port"
  | "setup_set_assembler_missing_eos_timeout_ms"
  | "setup_set_assembler_staging_dir"
  | "setup_set_assembler_staging_slots"
  | "setup_set_storage_queue_size"
  | "setup_set_monitor_metrics_frequency_hz"
  | "setup_save"
  | "setup_cancel";

export type CommandAction =
  | "get_stream_info"
  | "set_preview_size"
  | "episode_start"
  | "episode_stop"
  | "episode_keep"
  | "episode_discard"
  | SetupCommandAction;

export interface CommandMessage {
  type: "command";
  action: CommandAction;
  width?: number;
  height?: number;
  /** Subject of the command (channel/device/pairing name, etc.). Used by
   *  every `setup_*` action that addresses a row. */
  name?: string;
  index?: number;
  delta?: number;
  value?: string;
  /** Sub-field selector inside the subject — used by the generic
   *  subpanel record/preview field setters. */
  field?: string;
}

export function parseBinaryMessage(data: ArrayBuffer): BinaryWsMessage | null {
  if (data.byteLength < 3) {
    return null;
  }
  const view = new DataView(data);
  const typeTag = view.getUint8(0);
  const nameLen = view.getUint16(1, true);
  if (data.byteLength < 3 + nameLen) {
    return null;
  }
  const name = textDecoder.decode(new Uint8Array(data, 3, nameLen));
  const bodyStart = 3 + nameLen;

  switch (typeTag) {
    case FRAME_TYPE_JPEG: {
      const headerEnd = bodyStart + 8 + 8 + 4 + 4;
      if (data.byteLength < headerEnd) return null;
      const timestampNs = Number(view.getBigUint64(bodyStart, true));
      const frameIndex = Number(view.getBigUint64(bodyStart + 8, true));
      const width = view.getUint32(bodyStart + 16, true);
      const height = view.getUint32(bodyStart + 20, true);
      const jpegData = new Uint8Array(data.slice(headerEnd));
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
    case FRAME_TYPE_ENCODED_PACKET: {
      const headerEnd = bodyStart + 1 + 1 + 8 + 8 + 8 + 4 + 4 + 4;
      if (data.byteLength < headerEnd) return null;
      const codecId = view.getUint8(bodyStart);
      const flags = view.getUint8(bodyStart + 1);
      const ptsUs = Number(view.getBigUint64(bodyStart + 2, true));
      const sequence = Number(view.getBigUint64(bodyStart + 10, true));
      const sourceTimestampUs = Number(view.getBigUint64(bodyStart + 18, true));
      const width = view.getUint32(bodyStart + 26, true);
      const height = view.getUint32(bodyStart + 30, true);
      const payloadLen = view.getUint32(bodyStart + 34, true);
      if (data.byteLength < headerEnd + payloadLen) return null;
      const payload = new Uint8Array(data.slice(headerEnd, headerEnd + payloadLen));
      return {
        type: "encoded_packet",
        name,
        codecId,
        isKeyframe: (flags & 0x01) !== 0,
        ptsUs,
        sequence,
        sourceTimestampUs,
        width,
        height,
        payload,
      };
    }
    default:
      return null;
  }
}

export function parseJsonMessage(
  text: string,
):
  | RobotStateMessage
  | StreamInfoMessage
  | EpisodeStatusMessage
  | SetupStateMessage
  | null {
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

export function encodeCommand(
  action: CommandAction,
  fields: Partial<
    Pick<CommandMessage, "width" | "height" | "name" | "index" | "delta" | "value" | "field">
  > = {},
): string {
  return JSON.stringify({ type: "command", action, ...fields });
}

export function encodeSetPreviewSize(width: number, height: number): string {
  return encodeCommand("set_preview_size", { width, height });
}

export function encodeEpisodeCommand(
  action: Extract<
    CommandAction,
    "episode_start" | "episode_stop" | "episode_keep" | "episode_discard"
  >,
): string {
  return encodeCommand(action);
}

/** Encode a `setup_*` command envelope. The Rust controller's
 *  `apply_raw_command` dispatcher (`controller/src/setup/dispatch.rs`)
 *  matches on `action` and extracts the optional fields per verb. */
export function encodeSetupCommand(
  action: SetupCommandAction,
  fields: Partial<Pick<CommandMessage, "name" | "index" | "delta" | "value" | "field">> = {},
): string {
  return encodeCommand(action, fields);
}
