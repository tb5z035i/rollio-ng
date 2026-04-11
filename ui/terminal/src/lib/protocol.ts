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
export interface RobotStateMessage {
  type: "robot_state";
  name: string;
  timestamp_ns: number;
  num_joints: number;
  positions: number[];
  velocities: number[];
  efforts: number[];
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

/**
 * Encode a command message as JSON for sending to the Visualizer.
 */
export function encodeCommand(
  action: CommandAction,
  fields: Partial<Pick<CommandMessage, "width" | "height">> = {},
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
