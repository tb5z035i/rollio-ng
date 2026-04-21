import { useCallback, useEffect, useRef, useState } from "react";
import {
  encodeCommand,
  parseBinaryMessage,
  parseJsonMessage,
  type EpisodeStatusMessage,
  type RobotStateKind,
  type RobotStateMessage,
  type StreamInfoMessage,
} from "./protocol";
import {
  incrementGauge,
  nowMs,
  recordTiming,
  setGauge,
} from "./debug-metrics";

export interface CameraFrame {
  objectUrl: string;
  previewWidth: number;
  previewHeight: number;
  timestampNs: number;
  frameIndex: number;
  receivedAtWallTimeMs: number;
  sequence: number;
  jpegBytes: number;
}

/** A single state-kind sample retained for an aggregated robot channel. */
export interface RobotChannelSample {
  values: number[];
  valueMin: number[];
  valueMax: number[];
  timestampMs: number;
  numJoints: number;
}

/**
 * Per-channel snapshot built from the per-state-kind `robot_state` messages.
 * The UI renders one panel per `AggregatedRobotChannel` instead of one panel
 * per (channel, state_kind) pair so joint position / velocity / effort rows
 * for the same arm collapse into a single visual block.
 */
export interface AggregatedRobotChannel {
  name: string;
  states: Partial<Record<RobotStateKind, RobotChannelSample>>;
  /** Latest non-zero timestamp across all kinds; useful for sorting. */
  lastTimestampMs: number;
  endEffectorStatus?: RobotStateMessage["end_effector_status"];
  endEffectorFeedbackValid?: boolean;
}

export interface ControlSocketState {
  episodeStatus: EpisodeStatusMessage | null;
  connected: boolean;
  send: (msg: string) => void;
}

export interface PreviewSocketState {
  frames: Map<string, CameraFrame>;
  robotChannels: Map<string, AggregatedRobotChannel>;
  streamInfo: StreamInfoMessage | null;
  connected: boolean;
  send: (msg: string) => void;
}

export interface WebSocketLike extends EventTarget {
  binaryType: BinaryType;
  readyState: number;
  send(data: string): void;
  close(): void;
}

export type WebSocketFactory = (url: string) => WebSocketLike;

export interface UseControlSocketOptions {
  websocketFactory?: WebSocketFactory;
}

export interface UsePreviewSocketOptions {
  websocketFactory?: WebSocketFactory;
  objectUrlFactory?: (jpegData: Uint8Array) => string;
  revokeObjectUrl?: (url: string) => void;
}

const WS_OPEN = 1;
const BATCH_INTERVAL_MS = 16;
const RECONNECT_DELAYS = [1000, 2000, 4000, 10000] as const;

export function reconnectDelayMs(attempt: number): number {
  return RECONNECT_DELAYS[Math.min(attempt, RECONNECT_DELAYS.length - 1)];
}

function defaultObjectUrlFactory(jpegData: Uint8Array): string {
  const buffer = new Uint8Array(jpegData).buffer;
  return URL.createObjectURL(new Blob([buffer], { type: "image/jpeg" }));
}

function defaultWebSocketFactory(url: string): WebSocketLike {
  return new WebSocket(url);
}

function defaultRevokeObjectUrl(url: string): void {
  URL.revokeObjectURL(url);
}

async function toArrayBuffer(data: unknown): Promise<ArrayBuffer | null> {
  if (data instanceof ArrayBuffer) {
    return data;
  }
  if (ArrayBuffer.isView(data)) {
    const view = data;
    const bytes = new Uint8Array(view.byteLength);
    bytes.set(new Uint8Array(view.buffer, view.byteOffset, view.byteLength));
    return bytes.buffer;
  }
  if (data instanceof Blob) {
    return await data.arrayBuffer();
  }
  return null;
}

function revokeFrameUrls(
  frames: Map<string, CameraFrame>,
  revokeObjectUrl: (url: string) => void,
): void {
  for (const frame of frames.values()) {
    revokeObjectUrl(frame.objectUrl);
  }
}

interface SocketHandlers {
  onOpen?: (ws: WebSocketLike) => void;
  onText?: (text: string) => void;
  onArrayBuffer?: (buffer: ArrayBuffer) => void;
  onConnectedChange?: (connected: boolean) => void;
}

interface UseReconnectOptions {
  websocketFactory: WebSocketFactory;
  binaryType: BinaryType;
  handlers: SocketHandlers;
}

function useReconnectingSocket(url: string, options: UseReconnectOptions) {
  const wsRef = useRef<WebSocketLike | null>(null);
  const reconnectAttemptRef = useRef(0);
  const reconnectTimerRef = useRef<number | null>(null);
  const mountedRef = useRef(true);
  const handlersRef = useRef(options.handlers);
  handlersRef.current = options.handlers;

  const send = useCallback((msg: string) => {
    if (wsRef.current?.readyState === WS_OPEN) {
      wsRef.current.send(msg);
    }
  }, []);

  useEffect(() => {
    mountedRef.current = true;
    const factory = options.websocketFactory;
    const binaryType = options.binaryType;

    const scheduleReconnect = () => {
      if (!mountedRef.current) {
        return;
      }
      const attempt = reconnectAttemptRef.current;
      const delay = reconnectDelayMs(attempt);
      reconnectAttemptRef.current = attempt + 1;
      reconnectTimerRef.current = window.setTimeout(connect, delay);
    };

    const connect = () => {
      if (!mountedRef.current) {
        return;
      }

      const ws = factory(url);
      wsRef.current = ws;
      ws.binaryType = binaryType;

      const onOpen = () => {
        if (!mountedRef.current) return;
        reconnectAttemptRef.current = 0;
        handlersRef.current.onConnectedChange?.(true);
        handlersRef.current.onOpen?.(ws);
      };

      const onMessage = (event: Event) => {
        if (!mountedRef.current) return;
        const messageEvent = event as MessageEvent<unknown>;
        if (typeof messageEvent.data === "string") {
          handlersRef.current.onText?.(messageEvent.data);
          return;
        }
        void (async () => {
          const buffer = await toArrayBuffer(messageEvent.data);
          if (!buffer || !mountedRef.current) return;
          handlersRef.current.onArrayBuffer?.(buffer);
        })();
      };

      const onClose = () => {
        if (!mountedRef.current) return;
        handlersRef.current.onConnectedChange?.(false);
        wsRef.current = null;
        scheduleReconnect();
      };

      const onError = () => {
        // close handler drives reconnect behavior
      };

      ws.addEventListener("open", onOpen);
      ws.addEventListener("message", onMessage);
      ws.addEventListener("close", onClose);
      ws.addEventListener("error", onError);
    };

    connect();

    return () => {
      mountedRef.current = false;
      if (reconnectTimerRef.current != null) {
        window.clearTimeout(reconnectTimerRef.current);
      }
      if (wsRef.current) {
        wsRef.current.close();
        wsRef.current = null;
      }
    };
  }, [options.binaryType, options.websocketFactory, url]);

  return { send };
}

// ---------------------------------------------------------------------------
// Control socket — episode status + episode commands.
// ---------------------------------------------------------------------------

export function useControlSocket(
  url: string,
  options: UseControlSocketOptions = {},
): ControlSocketState {
  const websocketFactory = options.websocketFactory ?? defaultWebSocketFactory;

  const [connected, setConnected] = useState(false);
  const [episodeStatus, setEpisodeStatus] = useState<EpisodeStatusMessage | null>(
    null,
  );

  const episodeStatusRef = useRef<EpisodeStatusMessage | null>(null);
  const dirtyRef = useRef(false);

  const handlers = useRef<SocketHandlers>({
    onConnectedChange: (value) => {
      if (value) {
        setGauge("ws.control.connected", "Connected");
      } else {
        setGauge("ws.control.connected", "Disconnected");
        setGauge("ws.episode_status", "Unavailable");
      }
      setConnected(value);
    },
    onText: (text) => {
      const parseStartMs = nowMs();
      const msg = parseJsonMessage(text);
      recordTiming("ws.control.parse", nowMs() - parseStartMs);
      if (msg?.type === "episode_status") {
        episodeStatusRef.current = msg;
        dirtyRef.current = true;
        setGauge("ws.episode_status", msg.state);
        setGauge("ws.episode_count", msg.episode_count);
        setGauge("ws.episode_elapsed_ms", msg.elapsed_ms);
      }
    },
  }).current;

  const { send } = useReconnectingSocket(url, {
    websocketFactory,
    binaryType: "arraybuffer",
    handlers,
  });

  useEffect(() => {
    setGauge("ws.control.connected", "Disconnected");
    const flushInterval = window.setInterval(() => {
      if (!dirtyRef.current) return;
      dirtyRef.current = false;
      setEpisodeStatus(episodeStatusRef.current);
    }, BATCH_INTERVAL_MS);
    return () => window.clearInterval(flushInterval);
  }, []);

  return { episodeStatus, connected, send };
}

// ---------------------------------------------------------------------------
// Preview socket — camera frames + robot state + set_preview_size.
// ---------------------------------------------------------------------------

export function usePreviewSocket(
  url: string,
  options: UsePreviewSocketOptions = {},
): PreviewSocketState {
  const websocketFactory = options.websocketFactory ?? defaultWebSocketFactory;
  const objectUrlFactory = options.objectUrlFactory ?? defaultObjectUrlFactory;
  const revokeObjectUrl = options.revokeObjectUrl ?? defaultRevokeObjectUrl;

  const [connected, setConnected] = useState(false);
  const [frames, setFrames] = useState<Map<string, CameraFrame>>(() => new Map());
  const [robotChannels, setRobotChannels] = useState<
    Map<string, AggregatedRobotChannel>
  >(() => new Map());
  const [streamInfo, setStreamInfo] = useState<StreamInfoMessage | null>(null);

  const framesRef = useRef<Map<string, CameraFrame>>(new Map());
  const robotChannelsRef = useRef<Map<string, AggregatedRobotChannel>>(new Map());
  const streamInfoRef = useRef<StreamInfoMessage | null>(null);
  const dirtyRef = useRef(false);
  const frameSequenceRef = useRef(0);
  const wsRef = useRef<WebSocketLike | null>(null);

  const handlers = useRef<SocketHandlers>({
    onOpen: (ws) => {
      wsRef.current = ws;
      ws.send(encodeCommand("get_stream_info"));
    },
    onConnectedChange: (value) => {
      if (value) {
        setGauge("ws.preview.connected", "Connected");
      } else {
        setGauge("ws.preview.connected", "Disconnected");
        setGauge("ws.stream_info_status", "Unavailable");
        // Drop any cached preview-plane state when the socket flaps so the
        // UI doesn't show stale frames or robot positions.
        revokeFrameUrls(framesRef.current, revokeObjectUrl);
        framesRef.current.clear();
        robotChannelsRef.current.clear();
        streamInfoRef.current = null;
        frameSequenceRef.current = 0;
        dirtyRef.current = true;
        wsRef.current = null;
      }
      setConnected(value);
    },
    onText: (text) => {
      const parseStartMs = nowMs();
      const msg = parseJsonMessage(text);
      recordTiming("ws.parse.json", nowMs() - parseStartMs);
      if (msg?.type === "robot_state") {
        applyRobotStateSample(robotChannelsRef.current, msg);
        dirtyRef.current = true;
        incrementGauge("ws.robot_messages_total");
        setGauge("ws.robot_state_count", robotChannelsRef.current.size);
      } else if (msg?.type === "stream_info") {
        streamInfoRef.current = msg;
        dirtyRef.current = true;
        setGauge("ws.stream_info_status", "Ready");
        setGauge("ws.preview_fps_config", msg.configured_preview_fps);
        setGauge(
          "ws.active_preview_size",
          `${msg.active_preview_width}x${msg.active_preview_height}`,
        );
      }
    },
    onArrayBuffer: (buffer) => {
      const parseStartMs = nowMs();
      const msg = parseBinaryMessage(buffer);
      recordTiming("ws.parse.binary", nowMs() - parseStartMs);
      if (!msg) return;

      const previous = framesRef.current.get(msg.name);
      if (previous) {
        revokeObjectUrl(previous.objectUrl);
      }

      const receivedAtWallTimeMs = Date.now();
      const receiveLatencyMs = Math.max(
        0,
        receivedAtWallTimeMs - msg.timestampNs / 1_000_000,
      );
      const sequence = ++frameSequenceRef.current;
      const objectUrl = objectUrlFactory(msg.jpegData);

      framesRef.current.set(msg.name, {
        objectUrl,
        previewWidth: msg.previewWidth,
        previewHeight: msg.previewHeight,
        timestampNs: msg.timestampNs,
        frameIndex: msg.frameIndex,
        receivedAtWallTimeMs,
        sequence,
        jpegBytes: msg.jpegData.byteLength,
      });
      dirtyRef.current = true;
      incrementGauge("ws.frames_received_total");
      incrementGauge(`ws.frames_received_total.${msg.name}`);
      setGauge("ws.frame_count", framesRef.current.size);
      setGauge(`ws.frame_latency_ms.${msg.name}`, receiveLatencyMs);
      setGauge(`ws.frame_index.${msg.name}`, msg.frameIndex);
      setGauge(`ws.jpeg_bytes.${msg.name}`, msg.jpegData.byteLength);
      recordTiming("ws.frame_latency.receive", receiveLatencyMs);
    },
  }).current;

  const { send } = useReconnectingSocket(url, {
    websocketFactory,
    binaryType: "arraybuffer",
    handlers,
  });

  useEffect(() => {
    setGauge("ws.preview.connected", "Disconnected");
    setGauge("ws.frames_received_total", 0);
    setGauge("ws.robot_messages_total", 0);
    setGauge("ws.frame_count", 0);
    setGauge("ws.robot_state_count", 0);
    setGauge("ws.stream_info_status", "Unavailable");

    const flushInterval = window.setInterval(() => {
      if (!dirtyRef.current) return;
      const flushStartMs = nowMs();
      dirtyRef.current = false;
      setFrames(new Map(framesRef.current));
      setRobotChannels(new Map(robotChannelsRef.current));
      setStreamInfo(streamInfoRef.current);
      recordTiming("ws.flush", nowMs() - flushStartMs);
      setGauge("ws.frame_count", framesRef.current.size);
      setGauge("ws.robot_state_count", robotChannelsRef.current.size);
    }, BATCH_INTERVAL_MS);

    const streamInfoInterval = window.setInterval(() => {
      const ws = wsRef.current;
      if (ws?.readyState === WS_OPEN) {
        ws.send(encodeCommand("get_stream_info"));
      }
    }, 1000);

    return () => {
      window.clearInterval(flushInterval);
      window.clearInterval(streamInfoInterval);
      revokeFrameUrls(framesRef.current, revokeObjectUrl);
      framesRef.current.clear();
      robotChannelsRef.current.clear();
      streamInfoRef.current = null;
    };
  }, [revokeObjectUrl]);

  return { frames, robotChannels, streamInfo, connected, send };
}

/**
 * Mutate `channels` in-place to merge a new per-state-kind sample.
 *
 * The visualizer emits one message per (channel, state_kind) pair, so we
 * accumulate all kinds belonging to the same channel id under a single
 * `AggregatedRobotChannel` entry. Old kinds for that channel are preserved
 * across updates (joint_position arriving doesn't drop the last
 * joint_velocity sample).
 */
export function applyRobotStateSample(
  channels: Map<string, AggregatedRobotChannel>,
  msg: RobotStateMessage,
): void {
  const existing = channels.get(msg.name);
  const sample: RobotChannelSample = {
    values: Array.isArray(msg.values) ? msg.values : [],
    valueMin: Array.isArray(msg.value_min) ? msg.value_min : [],
    valueMax: Array.isArray(msg.value_max) ? msg.value_max : [],
    timestampMs:
      typeof msg.timestamp_us === "number" ? Math.floor(msg.timestamp_us / 1_000) : 0,
    numJoints: typeof msg.num_joints === "number" ? msg.num_joints : 0,
  };
  const states: AggregatedRobotChannel["states"] = existing
    ? { ...existing.states }
    : {};
  states[msg.state_kind] = sample;
  channels.set(msg.name, {
    name: msg.name,
    states,
    lastTimestampMs: Math.max(existing?.lastTimestampMs ?? 0, sample.timestampMs),
    endEffectorStatus: msg.end_effector_status ?? existing?.endEffectorStatus,
    endEffectorFeedbackValid:
      msg.end_effector_feedback_valid ?? existing?.endEffectorFeedbackValid,
  });
}
