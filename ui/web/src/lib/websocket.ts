import { useCallback, useEffect, useRef, useState } from "react";
import {
  encodeCommand,
  parseBinaryMessage,
  parseJsonMessage,
  type EpisodeStatusMessage,
  type RobotStateKind,
  type RobotStateMessage,
  type SetupStateMessage,
  type StreamInfoMessage,
} from "./protocol";
import {
  incrementGauge,
  nowMs,
  recordTiming,
  setGauge,
} from "./debug-metrics";
import {
  PreviewDecoderRegistry,
  type DecodedFrame,
  type DecoderRegistry,
} from "./preview-decoder";

/**
 * Tagged union emitted from `usePreviewSocket`. Two payload kinds:
 *
 * * `"jpeg"` — JPEG output mode. The legacy `<img src={objectUrl}>`
 *   render path consumes this verbatim.
 * * `"video"` — encoded output mode (H.264 today). The `videoFrame`
 *   is the WebCodecs decoder's output and must be drawn into a
 *   `<canvas>` synchronously before the next replacement frame
 *   arrives — this socket layer takes ownership of `videoFrame.close()`
 *   when it replaces or evicts the entry.
 */
export interface JpegCameraFrame {
  kind: "jpeg";
  objectUrl: string;
  previewWidth: number;
  previewHeight: number;
  timestampNs: number;
  frameIndex: number;
  receivedAtWallTimeMs: number;
  sequence: number;
  jpegBytes: number;
}

export interface VideoCameraFrame {
  kind: "video";
  videoFrame: VideoFrame;
  width: number;
  height: number;
  /** Codec PTS in µs (relative to recording start). Not safe to
   *  compare against wall-clock — use `sourceTimestampUs` for that. */
  timestampUs: number;
  /** Camera capture wall-clock µs since UNIX epoch. Compare with
   *  `Date.now() * 1000` for a meaningful end-to-end latency. */
  sourceTimestampUs: number;
  receivedAtWallTimeMs: number;
  sequence: number;
  payloadBytes: number;
  codecId: number;
}

export type CameraFrame = JpegCameraFrame | VideoCameraFrame;

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
  /** Latest `setup_state` snapshot from `rollio-control-server` when
   *  the SPA is running in setup mode. `null` while waiting for the
   *  first publish (the wizard polls the controller every loop tick,
   *  so this fills in within ~50 ms of connect). */
  setupState: SetupStateMessage | null;
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
  /** Factory for the WebCodecs decoder seam. Defaults to a fresh
   *  `PreviewDecoderRegistry`. Tests substitute a fake here to avoid
   *  needing the WebCodecs API in jsdom. */
  decoderRegistryFactory?: () => DecoderRegistry;
  /** When `false`, skip opening the WebSocket entirely. Used by the
   *  setup wizard to gate the preview socket on identify / preview
   *  steps so we don't spam the gateway with "upstream absent" reconnect
   *  loops when no visualizer process is running. Defaults to `true`. */
  enabled?: boolean;
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

function releaseFrame(
  frame: CameraFrame,
  revokeObjectUrl: (url: string) => void,
): void {
  if (frame.kind === "jpeg") {
    revokeObjectUrl(frame.objectUrl);
  } else {
    try {
      frame.videoFrame.close();
    } catch {
      /* a closed VideoFrame throws on second close; safe */
    }
  }
}

function revokeFrameUrls(
  frames: Map<string, CameraFrame>,
  revokeObjectUrl: (url: string) => void,
): void {
  for (const frame of frames.values()) {
    releaseFrame(frame, revokeObjectUrl);
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
  /** When `false`, the hook returns a no-op `send` and never opens a
   *  socket. Toggling back to `true` (re-)connects on the next render. */
  enabled?: boolean;
}

function useReconnectingSocket(url: string, options: UseReconnectOptions) {
  const wsRef = useRef<WebSocketLike | null>(null);
  const reconnectAttemptRef = useRef(0);
  const reconnectTimerRef = useRef<number | null>(null);
  const mountedRef = useRef(true);
  const handlersRef = useRef(options.handlers);
  handlersRef.current = options.handlers;
  const enabled = options.enabled ?? true;

  const send = useCallback((msg: string) => {
    if (wsRef.current?.readyState === WS_OPEN) {
      wsRef.current.send(msg);
    }
  }, []);

  useEffect(() => {
    if (!enabled) {
      // Tear down any open socket from a previous `enabled=true` render
      // so the gateway sees a clean close, not a dangling subscriber.
      if (wsRef.current) {
        wsRef.current.close();
        wsRef.current = null;
      }
      if (reconnectTimerRef.current != null) {
        window.clearTimeout(reconnectTimerRef.current);
        reconnectTimerRef.current = null;
      }
      handlersRef.current.onConnectedChange?.(false);
      return;
    }
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
  }, [enabled, options.binaryType, options.websocketFactory, url]);

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
  const [setupState, setSetupState] = useState<SetupStateMessage | null>(null);

  const episodeStatusRef = useRef<EpisodeStatusMessage | null>(null);
  const setupStateRef = useRef<SetupStateMessage | null>(null);
  const dirtyRef = useRef(false);

  const handlers = useRef<SocketHandlers>({
    onConnectedChange: (value) => {
      if (value) {
        setGauge("ws.control.connected", "Connected");
      } else {
        setGauge("ws.control.connected", "Disconnected");
        setGauge("ws.episode_status", "Unavailable");
        setGauge("ws.setup_status", "Unavailable");
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
      } else if (msg?.type === "setup_state") {
        setupStateRef.current = msg;
        dirtyRef.current = true;
        setGauge("ws.setup_status", msg.status);
        setGauge("ws.setup_step", `${msg.step_index}/${msg.total_steps}`);
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
      setSetupState(setupStateRef.current);
    }, BATCH_INTERVAL_MS);
    return () => window.clearInterval(flushInterval);
  }, []);

  return { episodeStatus, setupState, connected, send };
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
  const decoderRegistryFactory =
    options.decoderRegistryFactory ?? (() => new PreviewDecoderRegistry());

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
  // Per-camera codec id for the currently-configured encoded preview
  // session. Used in the meta line and in tests; populated on the
  // first keyframe per camera, cleared on socket flap.
  const encodedCodecRef = useRef<Map<string, number>>(new Map());

  // Per-camera "configuration key" `codecId@WxH`. The frontend's
  // WebCodecs decoder is auto-configured from the first keyframe (the
  // payload's inline SPS/PPS supplies the codec string); we re-call
  // `configure()` only when the key changes (e.g. `set_preview_size`
  // restarts the session at new dims). Tracked here, not in the
  // decoder registry, so the registry's interface stays narrow.
  const decoderConfigKeyRef = useRef<Map<string, string>>(new Map());

  // The decoder registry is re-created on each mount (the factory
  // closure also lets tests substitute a fake). Stored in a ref so
  // the long-lived `handlers` closure can reach it without a
  // re-render on registry replacement.
  const decoderRegistryRef = useRef<DecoderRegistry | null>(null);
  if (decoderRegistryRef.current === null) {
    decoderRegistryRef.current = decoderRegistryFactory();
  }

  const onDecodedFrame = useCallback(
    (decoded: DecodedFrame) => {
      const codecId = encodedCodecRef.current.get(decoded.name) ?? 0;
      const previous = framesRef.current.get(decoded.name);
      if (previous) {
        releaseFrame(previous, revokeObjectUrl);
      }
      const receivedAtWallTimeMs = decoded.receivedAtWallTimeMs;
      const sequence = ++frameSequenceRef.current;
      framesRef.current.set(decoded.name, {
        kind: "video",
        videoFrame: decoded.videoFrame,
        width: decoded.width,
        height: decoded.height,
        timestampUs: decoded.timestampUs,
        sourceTimestampUs: decoded.sourceTimestampUs,
        receivedAtWallTimeMs,
        sequence,
        // Bytes-per-decoded-frame metric is best-effort and lives at
        // packet receive time (see `onArrayBuffer`); the canvas tile
        // shows the most recent payload size, not the per-frame size
        // (which is meaningless after decode).
        payloadBytes:
          (previous?.kind === "video" ? previous.payloadBytes : 0) ?? 0,
        codecId,
      });
      dirtyRef.current = true;
      incrementGauge("ui.frames_presented_total");
      incrementGauge(`ui.frames_presented_total.${decoded.name}`);
      setGauge(
        `ui.preview_resolution.${decoded.name}`,
        `${decoded.width}x${decoded.height}`,
      );
      // True end-to-end latency from camera capture to decoded frame.
      // `sourceTimestampUs` is unix-epoch µs from the encoder header
      // (propagated through `EncodedPacketHeader.source_timestamp_us`),
      // so this stays in the same time base as `Date.now()`.
      const decodeLatencyMs = Math.max(
        0,
        receivedAtWallTimeMs - decoded.sourceTimestampUs / 1_000,
      );
      setGauge(`ui.video_decode_latency_ms.${decoded.name}`, decodeLatencyMs);
    },
    [revokeObjectUrl],
  );

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
        // UI doesn't show stale frames or robot positions. Also tear
        // down every decoder so a reconnect rebuilds them from the
        // first keyframe of the new session.
        revokeFrameUrls(framesRef.current, revokeObjectUrl);
        framesRef.current.clear();
        robotChannelsRef.current.clear();
        streamInfoRef.current = null;
        encodedCodecRef.current.clear();
        decoderConfigKeyRef.current.clear();
        decoderRegistryRef.current?.closeAll();
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
        setGauge("ws.preview_output_mode", msg.preview_output_mode);
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

      if (msg.type === "encoded_packet") {
        incrementGauge(`ws.encoded_packets_total.${msg.name}`);
        // Mirror the JPEG-path bump so InfoPanel's `Frames: rx=` shows
        // a non-zero counter in encoded mode too. Without this, the
        // panel reads rx=0 forever even when packets are flowing,
        // which makes "no preview" debugging extremely misleading.
        incrementGauge("ws.frames_received_total");
        incrementGauge(`ws.frames_received_total.${msg.name}`);
        if (msg.isKeyframe) {
          incrementGauge(`ws.encoded_keyframes_total.${msg.name}`);
        }
        // Stash the most-recent payload size in the live frame entry
        // for the canvas tile's meta line; the registry's onFrame
        // callback fires asynchronously after `decode`, so we update
        // here so the value is visible by the time a decoded frame
        // lands.
        const existing = framesRef.current.get(msg.name);
        if (existing && existing.kind === "video") {
          framesRef.current.set(msg.name, {
            ...existing,
            payloadBytes: msg.payload.byteLength,
          });
          dirtyRef.current = true;
        }
        setGauge(`ws.encoded_payload_bytes.${msg.name}`, msg.payload.byteLength);

        // Auto-configure the decoder on the first keyframe per camera
        // (or whenever the (codec, width, height) key changes — e.g.
        // a `set_preview_size` restarts the session at new dims).
        // Keyframes carry inline SPS/PPS in their Annex B payload, so
        // we hand the payload itself as the "description" to
        // `configure`; `codecStringFor` parses SPS bytes to derive
        // `avc1.PPCCLL`.
        if (msg.isKeyframe) {
          const configKey = `${msg.codecId}@${msg.width}x${msg.height}`;
          const previousKey = decoderConfigKeyRef.current.get(msg.name);
          if (previousKey !== configKey) {
            decoderConfigKeyRef.current.set(msg.name, configKey);
            encodedCodecRef.current.set(msg.name, msg.codecId);
            setGauge(`ws.encoded_codec.${msg.name}`, msg.codecId);
            setGauge(`ws.encoded_codec_dims.${msg.name}`, `${msg.width}x${msg.height}`);
            incrementGauge(`ws.encoded_config_total.${msg.name}`);
            decoderRegistryRef.current?.configure(
              msg.name,
              msg.codecId,
              msg.payload,
              msg.width,
              msg.height,
              onDecodedFrame,
            );
          }
        }

        decoderRegistryRef.current?.decode(
          msg.name,
          msg.payload,
          msg.ptsUs,
          msg.sourceTimestampUs,
          msg.isKeyframe,
        );
        return;
      }

      const previous = framesRef.current.get(msg.name);
      if (previous) {
        releaseFrame(previous, revokeObjectUrl);
      }

      const receivedAtWallTimeMs = Date.now();
      const receiveLatencyMs = Math.max(
        0,
        receivedAtWallTimeMs - msg.timestampNs / 1_000_000,
      );
      const sequence = ++frameSequenceRef.current;
      const objectUrl = objectUrlFactory(msg.jpegData);

      framesRef.current.set(msg.name, {
        kind: "jpeg",
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
    enabled: options.enabled,
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
      encodedCodecRef.current.clear();
      decoderConfigKeyRef.current.clear();
      decoderRegistryRef.current?.closeAll();
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
