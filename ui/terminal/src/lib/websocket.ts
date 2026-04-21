/**
 * WebSocket client hooks for the rollio terminal UI.
 *
 * The UI now opens **two** sockets to the backend:
 *
 * - **Control socket** (`useControlSocket`) — JSON only. Carries setup
 *   commands/state and episode commands/state. Long-lived: it stays connected
 *   for the entire session (the rollio-control-server sidecar is only stopped
 *   on shutdown). The control plane no longer flaps when identify swaps
 *   the visualizer.
 * - **Preview socket** (`usePreviewSocket`) — binary frames + robot state +
 *   `set_preview_size` negotiation. On-demand during setup (only while the
 *   preview runtime is up), always-on during collect.
 *
 * Splitting them removes the single-WebSocket coupling that made the wizard
 * freeze when the visualizer was hot-swapped.
 *
 * Performance optimizations carry over from the previous single-hook
 * implementation:
 * - useRef for accumulator maps to avoid re-renders on every message
 * - Batched state flush at ~60fps
 */

import { useState, useEffect, useRef, useCallback } from "react";
import WebSocket from "ws";
import {
  encodeCommand,
  parseBinaryMessage,
  parseJsonMessage,
  type EpisodeStatusMessage,
  type RobotStateKind,
  type RobotStateMessage,
  type SetupStateMessage,
  type StreamInfoMessage,
} from "./protocol.js";
import {
  incrementGauge,
  nowMs,
  recordTiming,
  setGauge,
} from "./debug-metrics.js";

/** Camera frame data for rendering. */
export interface CameraFrame {
  jpegData: Buffer;
  previewWidth: number;
  previewHeight: number;
  timestampNs: number;
  frameIndex: number;
  receivedAtWallTimeMs: number;
  sequence: number;
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
  /** Optional end-effector lifecycle metadata reused from the legacy field. */
  endEffectorStatus?: RobotStateMessage["end_effector_status"];
  endEffectorFeedbackValid?: boolean;
}

/** Backoff schedule for both hooks. First retry is immediate so the preview
 * socket reconnects cleanly when the visualizer comes back up after an
 * identify swap. */
const RECONNECT_DELAYS = [0, 200, 800, 3000, 10000];
const BATCH_INTERVAL_MS = 16;
/** Cap queued commands while the socket is down. */
const MAX_PENDING_OUTBOUND = 256;

interface ReconnectingSocket {
  /** Send a text frame, queueing it if the socket is currently down. */
  send: (msg: string) => void;
  /** Tear down the socket and any reconnect timer. */
  close: () => void;
  /** Current connectedness, for callers that want to surface UI state. */
  connectedRef: React.MutableRefObject<boolean>;
}

interface ReconnectHandlers {
  onOpen?: (ws: WebSocket) => void;
  onText?: (text: string) => void;
  onBinary?: (buffer: Buffer) => void;
  onConnectedChange?: (connected: boolean) => void;
}

/**
 * `active=false` is the "do not connect" state. The hook tears down any open
 * socket, clears reconnect timers, and stops trying. When `active` flips back
 * to `true`, the backoff is reset to 0 and connection resumes immediately.
 *
 * This avoids the wizard freezing the preview socket in a 10 s backoff window
 * while the visualizer was actually up (debug session 8d351b confirmed
 * `~14 s` first-frame latency without this gating).
 */
function useReconnectingSocket(
  url: string,
  handlers: ReconnectHandlers,
  active: boolean = true,
): ReconnectingSocket {
  const wsRef = useRef<WebSocket | null>(null);
  const reconnectAttemptRef = useRef(0);
  const reconnectTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const mountedRef = useRef(true);
  const pendingOutboundRef = useRef<string[]>([]);
  const connectedRef = useRef(false);
  const handlersRef = useRef(handlers);
  handlersRef.current = handlers;

  const send = useCallback((msg: string) => {
    if (wsRef.current?.readyState === WebSocket.OPEN) {
      wsRef.current.send(msg);
      return;
    }
    while (pendingOutboundRef.current.length >= MAX_PENDING_OUTBOUND) {
      pendingOutboundRef.current.shift();
    }
    pendingOutboundRef.current.push(msg);
  }, []);

  useEffect(() => {
    mountedRef.current = true;

    function flushPendingOutbound() {
      const ws = wsRef.current;
      if (!ws || ws.readyState !== WebSocket.OPEN) return;
      const queue = pendingOutboundRef.current;
      while (queue.length > 0) {
        const m = queue.shift();
        if (m) ws.send(m);
      }
    }

    function setConnected(value: boolean) {
      connectedRef.current = value;
      handlersRef.current.onConnectedChange?.(value);
    }

    function connect() {
      if (!mountedRef.current) return;
      if (!active) return;
      const ws = new WebSocket(url);
      wsRef.current = ws;
      ws.binaryType = "nodebuffer";

      ws.on("open", () => {
        if (!mountedRef.current) return;
        reconnectAttemptRef.current = 0;
        setConnected(true);
        handlersRef.current.onOpen?.(ws);
        flushPendingOutbound();
      });

      ws.on("message", (data: Buffer | string, isBinary: boolean) => {
        if (!mountedRef.current) return;
        if (isBinary && Buffer.isBuffer(data)) {
          handlersRef.current.onBinary?.(data);
        } else {
          const text = typeof data === "string" ? data : data.toString("utf-8");
          handlersRef.current.onText?.(text);
        }
      });

      ws.on("close", () => {
        if (!mountedRef.current) return;
        setConnected(false);
        wsRef.current = null;
        scheduleReconnect();
      });

      ws.on("error", (err: Error) => {
        // Suppress ECONNREFUSED noise during reconnect; close fires after.
        if (
          !("code" in err) ||
          (err as NodeJS.ErrnoException).code !== "ECONNREFUSED"
        ) {
          // intentionally silent — surfaced via close
        }
      });
    }

    function scheduleReconnect() {
      if (!mountedRef.current) return;
      if (!active) return;
      const attempt = reconnectAttemptRef.current;
      const delay =
        RECONNECT_DELAYS[Math.min(attempt, RECONNECT_DELAYS.length - 1)];
      reconnectAttemptRef.current = attempt + 1;
      reconnectTimerRef.current = setTimeout(connect, delay);
    }

    if (active) {
      // Fresh activation always starts at attempt 0 (delay 0) so a flip from
      // inactive→active produces an immediate connect rather than a
      // backoff-delayed retry.
      reconnectAttemptRef.current = 0;
      connect();
    }

    return () => {
      mountedRef.current = false;
      pendingOutboundRef.current = [];
      if (reconnectTimerRef.current) clearTimeout(reconnectTimerRef.current);
      if (wsRef.current) {
        wsRef.current.close();
        wsRef.current = null;
      }
    };
  }, [active, url]);

  const close = useCallback(() => {
    mountedRef.current = false;
    if (reconnectTimerRef.current) clearTimeout(reconnectTimerRef.current);
    if (wsRef.current) {
      wsRef.current.close();
      wsRef.current = null;
    }
  }, []);

  return { send, close, connectedRef };
}

// ---------------------------------------------------------------------------
// Control socket
// ---------------------------------------------------------------------------

export interface ControlSocketState {
  connected: boolean;
  send: (msg: string) => void;
  episodeStatus: EpisodeStatusMessage | null;
  setupState: SetupStateMessage | null;
}

export function useControlSocket(url: string): ControlSocketState {
  const [connected, setConnected] = useState(false);
  const [episodeStatus, setEpisodeStatus] = useState<EpisodeStatusMessage | null>(
    null,
  );
  const [setupState, setSetupState] = useState<SetupStateMessage | null>(null);

  const episodeStatusRef = useRef<EpisodeStatusMessage | null>(null);
  const setupStateRef = useRef<SetupStateMessage | null>(null);
  const dirtyRef = useRef(false);

  const handlers = useRef<ReconnectHandlers>({
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

  const { send } = useReconnectingSocket(url, handlers);

  useEffect(() => {
    setGauge("ws.control.connected", "Disconnected");
    const flushInterval = setInterval(() => {
      if (!dirtyRef.current) return;
      dirtyRef.current = false;
      setEpisodeStatus(episodeStatusRef.current);
      setSetupState(setupStateRef.current);
    }, BATCH_INTERVAL_MS);
    return () => clearInterval(flushInterval);
  }, []);

  return { connected, send, episodeStatus, setupState };
}

// ---------------------------------------------------------------------------
// Preview socket
// ---------------------------------------------------------------------------

export interface PreviewSocketState {
  connected: boolean;
  send: (msg: string) => void;
  frames: Map<string, CameraFrame>;
  robotChannels: Map<string, AggregatedRobotChannel>;
  streamInfo: StreamInfoMessage | null;
}

export function usePreviewSocket(
  url: string,
  active: boolean = true,
): PreviewSocketState {
  const [connected, setConnected] = useState(false);
  const [frames, setFrames] = useState<Map<string, CameraFrame>>(
    () => new Map(),
  );
  const [robotChannels, setRobotChannels] = useState<
    Map<string, AggregatedRobotChannel>
  >(() => new Map());
  const [streamInfo, setStreamInfo] = useState<StreamInfoMessage | null>(null);

  const framesRef = useRef<Map<string, CameraFrame>>(new Map());
  const robotChannelsRef = useRef<Map<string, AggregatedRobotChannel>>(
    new Map(),
  );
  const streamInfoRef = useRef<StreamInfoMessage | null>(null);
  const dirtyRef = useRef(false);
  const frameSequenceRef = useRef(0);

  const handlers = useRef<ReconnectHandlers>({
    onConnectedChange: (value) => {
      if (value) {
        setGauge("ws.preview.connected", "Connected");
      } else {
        setGauge("ws.preview.connected", "Disconnected");
        setGauge("ws.stream_info_status", "Unavailable");
        // When the preview socket flaps, drop the live data; control state
        // is intentionally untouched (it's owned by the other socket).
        framesRef.current = new Map();
        robotChannelsRef.current = new Map();
        streamInfoRef.current = null;
        frameSequenceRef.current = 0;
        dirtyRef.current = true;
        setGauge("ws.frame_count", 0);
        setGauge("ws.robot_state_count", 0);
      }
      setConnected(value);
    },
    onOpen: (ws) => {
      ws.send(encodeCommand("get_stream_info"));
    },
    onBinary: (data) => {
      const parseStartMs = nowMs();
      const msg = parseBinaryMessage(data);
      recordTiming("ws.parse.binary", nowMs() - parseStartMs);
      if (!msg) return;
      const receivedAtWallTimeMs = Date.now();
      const receiveLatencyMs = Math.max(
        0,
        receivedAtWallTimeMs - msg.timestampNs / 1_000_000,
      );
      const sequence = ++frameSequenceRef.current;
      framesRef.current.set(msg.name, {
        jpegData: msg.jpegData,
        previewWidth: msg.previewWidth,
        previewHeight: msg.previewHeight,
        timestampNs: msg.timestampNs,
        frameIndex: msg.frameIndex,
        receivedAtWallTimeMs,
        sequence,
      });
      dirtyRef.current = true;
      incrementGauge("ws.frames_received_total");
      incrementGauge(`ws.frames_received_total.${msg.name}`);
      setGauge("ws.frame_count", framesRef.current.size);
      setGauge(`ws.frame_latency_ms.${msg.name}`, receiveLatencyMs);
      setGauge(`ws.frame_index.${msg.name}`, msg.frameIndex);
      recordTiming("ws.frame_latency.receive", receiveLatencyMs);
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
  }).current;

  const { send } = useReconnectingSocket(url, handlers, active);

  useEffect(() => {
    setGauge("ws.preview.connected", "Disconnected");

    const flushInterval = setInterval(() => {
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

    const streamInfoInterval = setInterval(() => {
      // We cannot reach the WebSocket directly from here, but `send` queues
      // the request and our reconnect logic flushes the queue on open, so
      // keeping the periodic poll on the queue is harmless when offline.
      send(encodeCommand("get_stream_info"));
    }, 1000);

    return () => {
      clearInterval(flushInterval);
      clearInterval(streamInfoInterval);
    };
  }, [send]);

  return { connected, send, frames, robotChannels, streamInfo };
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
function applyRobotStateSample(
  channels: Map<string, AggregatedRobotChannel>,
  msg: RobotStateMessage,
): void {
  const existing = channels.get(msg.name);
  // Convert wire-format microseconds to milliseconds for the UI internal
  // display fields (which keep the historical `*Ms` naming for display).
  const timestampMs = Math.floor(msg.timestamp_us / 1_000);
  const sample: RobotChannelSample = {
    values: msg.values,
    valueMin: msg.value_min ?? [],
    valueMax: msg.value_max ?? [],
    timestampMs,
    numJoints: msg.num_joints,
  };
  const states: AggregatedRobotChannel["states"] = existing
    ? { ...existing.states }
    : {};
  states[msg.state_kind] = sample;
  channels.set(msg.name, {
    name: msg.name,
    states,
    lastTimestampMs: Math.max(existing?.lastTimestampMs ?? 0, timestampMs),
    endEffectorStatus: msg.end_effector_status ?? existing?.endEffectorStatus,
    endEffectorFeedbackValid:
      msg.end_effector_feedback_valid ?? existing?.endEffectorFeedbackValid,
  });
}
