const FRAME_TYPE_JPEG = 0x01;
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
  source_fps_estimate: number | null;
  published_fps_estimate: number | null;
  last_published_timestamp_ms: number | null;
}

export interface StreamInfoMessage {
  type: "stream_info";
  server_timestamp_ms: number;
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

export function parseBinaryMessage(
  data: ArrayBuffer,
): CameraFrameMessage | null {
  if (data.byteLength < 3) {
    return null;
  }

  const view = new DataView(data);
  const typeTag = view.getUint8(0);
  if (typeTag !== FRAME_TYPE_JPEG) {
    return null;
  }

  const nameLen = view.getUint16(1, true);
  const headerStart = 3 + nameLen;
  const headerEnd = headerStart + 8 + 8 + 4 + 4;
  if (data.byteLength < headerEnd) {
    return null;
  }

  const name = textDecoder.decode(new Uint8Array(data, 3, nameLen));
  const timestampNs = Number(view.getBigUint64(headerStart, true));
  const frameIndex = Number(view.getBigUint64(headerStart + 8, true));
  const width = view.getUint32(headerStart + 16, true);
  const height = view.getUint32(headerStart + 20, true);
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
