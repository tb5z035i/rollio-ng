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

export type CommandAction =
  | "get_stream_info"
  | "set_preview_size"
  | "episode_start"
  | "episode_stop"
  | "episode_keep"
  | "episode_discard";

export interface CommandMessage {
  type: "command";
  action: CommandAction;
  width?: number;
  height?: number;
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
): RobotStateMessage | StreamInfoMessage | EpisodeStatusMessage | null {
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
    return null;
  } catch {
    return null;
  }
}

export function encodeCommand(
  action: CommandAction,
  fields: Partial<Pick<CommandMessage, "width" | "height">> = {},
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
