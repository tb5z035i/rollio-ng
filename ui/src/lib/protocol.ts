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
  width: number;
  height: number;
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

/** Command sent from UI to Visualizer. */
export interface CommandMessage {
  type: "command";
  action: string;
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
 *   Bytes N+1..4: original width (u32 LE)
 *   Bytes N+5..8: original height (u32 LE)
 *   Remaining:    JPEG payload
 */
export function parseBinaryMessage(
  data: Buffer,
): CameraFrameMessage | null {
  if (data.length < 1 + 2) return null;

  const typeTag = data[0];
  if (typeTag !== FRAME_TYPE_JPEG) return null;

  const nameLen = data.readUInt16LE(1);
  const headerEnd = 3 + nameLen + 4 + 4;
  if (data.length < headerEnd) return null;

  const name = data.subarray(3, 3 + nameLen).toString("utf-8");
  const width = data.readUInt32LE(3 + nameLen);
  const height = data.readUInt32LE(3 + nameLen + 4);
  const jpegData = Buffer.from(data.subarray(headerEnd));

  return {
    type: "camera_frame",
    name,
    width,
    height,
    jpegData,
  };
}

/**
 * Parse a JSON WebSocket text message into a RobotStateMessage.
 */
export function parseJsonMessage(text: string): RobotStateMessage | null {
  try {
    const obj = JSON.parse(text);
    if (obj && obj.type === "robot_state") {
      return obj as RobotStateMessage;
    }
    return null;
  } catch {
    return null;
  }
}

/**
 * Encode a command message as JSON for sending to the Visualizer.
 */
export function encodeCommand(action: string): string {
  return JSON.stringify({ type: "command", action });
}
