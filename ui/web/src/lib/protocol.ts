const FRAME_TYPE_JPEG = 0x01;
const FRAME_TYPE_ENCODED_CONFIG = 0x02;
const FRAME_TYPE_ENCODED_PACKET = 0x03;
const textDecoder = new TextDecoder("utf-8");

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

/** Codec config message (encoded preview mode, kind 0x02). The
 *  `description` is the AVCC bytes the visualizer already converted
 *  from Annex B SPS/PPS so the web UI can hand it to WebCodecs
 *  verbatim. */
export interface EncodedConfigMessage {
  type: "encoded_config";
  name: string;
  codecId: number; // matches EncodedCodecId discriminant
  width: number;
  height: number;
  description: Uint8Array;
}

/** One encoded preview packet (kind 0x03). Payload is the
 *  codec-specific access unit (Annex B AU for H.264, RVL frame for
 *  RVL, etc.). */
export interface EncodedPacketMessage {
  type: "encoded_packet";
  name: string;
  codecId: number;
  isKeyframe: boolean;
  ptsUs: number;
  sequence: number;
  payload: Uint8Array;
}

export type BinaryWsMessage =
  | CameraFrameMessage
  | EncodedConfigMessage
  | EncodedPacketMessage;

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
  latest_timestamp_ms: number | null;
  latest_frame_index: number | null;
  received_fps_estimate: number | null;
  bytes_per_sec: number | null;
  keyframe_age_ms: number | null;
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
    case FRAME_TYPE_ENCODED_CONFIG: {
      const headerEnd = bodyStart + 1 + 4 + 4 + 4;
      if (data.byteLength < headerEnd) return null;
      const codecId = view.getUint8(bodyStart);
      const width = view.getUint32(bodyStart + 1, true);
      const height = view.getUint32(bodyStart + 5, true);
      const descLen = view.getUint32(bodyStart + 9, true);
      if (data.byteLength < headerEnd + descLen) return null;
      const description = new Uint8Array(data.slice(headerEnd, headerEnd + descLen));
      return {
        type: "encoded_config",
        name,
        codecId,
        width,
        height,
        description,
      };
    }
    case FRAME_TYPE_ENCODED_PACKET: {
      const headerEnd = bodyStart + 1 + 1 + 8 + 8 + 4;
      if (data.byteLength < headerEnd) return null;
      const codecId = view.getUint8(bodyStart);
      const flags = view.getUint8(bodyStart + 1);
      const ptsUs = Number(view.getBigUint64(bodyStart + 2, true));
      const sequence = Number(view.getBigUint64(bodyStart + 10, true));
      const payloadLen = view.getUint32(bodyStart + 18, true);
      if (data.byteLength < headerEnd + payloadLen) return null;
      const payload = new Uint8Array(data.slice(headerEnd, headerEnd + payloadLen));
      return {
        type: "encoded_packet",
        name,
        codecId,
        isKeyframe: (flags & 0x01) !== 0,
        ptsUs,
        sequence,
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
