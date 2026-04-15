/**
 * WebSocket client hook with auto-reconnect.
 *
 * Performance optimizations:
 * - Uses useRef for mutable frame/state maps to avoid re-renders on every message
 * - Batches state updates at ~60fps via setInterval to coalesce rapid updates
 * - Tracks frame references for change detection
 */

import { useState, useEffect, useRef, useCallback } from "react";
import WebSocket from "ws";
import {
  encodeCommand,
  parseBinaryMessage,
  parseJsonMessage,
  type EpisodeStatusMessage,
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

/** Return type of the useWebSocket hook. */
export interface WebSocketState {
  frames: Map<string, CameraFrame>;
  robotStates: Map<string, RobotStateMessage>;
  streamInfo: StreamInfoMessage | null;
  episodeStatus: EpisodeStatusMessage | null;
  setupState: SetupStateMessage | null;
  connected: boolean;
  send: (msg: string) => void;
}

const RECONNECT_DELAYS = [1000, 2000, 4000, 10000]; // exponential backoff
const BATCH_INTERVAL_MS = 16; // ~60fps state flush

/**
 * React hook that manages a WebSocket connection to the Visualizer.
 *
 * Automatically reconnects on disconnect with exponential backoff.
 * Batches incoming data updates to React state at ~60fps to avoid
 * excessive re-renders.
 */
export function useWebSocket(url: string): WebSocketState {
  const [connected, setConnected] = useState(false);
  const [frames, setFrames] = useState<Map<string, CameraFrame>>(
    () => new Map(),
  );
  const [robotStates, setRobotStates] = useState<
    Map<string, RobotStateMessage>
  >(() => new Map());
  const [streamInfo, setStreamInfo] = useState<StreamInfoMessage | null>(null);
  const [episodeStatus, setEpisodeStatus] = useState<EpisodeStatusMessage | null>(
    null,
  );
  const [setupState, setSetupState] = useState<SetupStateMessage | null>(null);

  // Mutable refs for accumulating data between batch flushes
  const framesRef = useRef<Map<string, CameraFrame>>(new Map());
  const robotStatesRef = useRef<Map<string, RobotStateMessage>>(new Map());
  const streamInfoRef = useRef<StreamInfoMessage | null>(null);
  const episodeStatusRef = useRef<EpisodeStatusMessage | null>(null);
  const setupStateRef = useRef<SetupStateMessage | null>(null);
  const dirtyRef = useRef(false);
  const wsRef = useRef<WebSocket | null>(null);
  const frameSequenceRef = useRef(0);
  const reconnectAttemptRef = useRef(0);
  const reconnectTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const mountedRef = useRef(true);

  const send = useCallback((msg: string) => {
    if (wsRef.current?.readyState === WebSocket.OPEN) {
      wsRef.current.send(msg);
    }
  }, []);

  useEffect(() => {
    mountedRef.current = true;
    setGauge("ws.connected", "Disconnected");
    setGauge("ws.frames_received_total", 0);
    setGauge("ws.robot_messages_total", 0);
    setGauge("ws.frame_count", 0);
    setGauge("ws.robot_state_count", 0);
    setGauge("ws.stream_info_status", "Unavailable");
    setGauge("ws.episode_status", "Unavailable");
    setGauge("ws.setup_status", "Unavailable");

    // Batch flush interval: push ref data into React state at ~60fps
    const flushInterval = setInterval(() => {
      if (dirtyRef.current && mountedRef.current) {
        const flushStartMs = nowMs();
        dirtyRef.current = false;
        setFrames(new Map(framesRef.current));
        setRobotStates(new Map(robotStatesRef.current));
        setStreamInfo(streamInfoRef.current);
        setEpisodeStatus(episodeStatusRef.current);
        setSetupState(setupStateRef.current);
        recordTiming("ws.flush", nowMs() - flushStartMs);
        setGauge("ws.frame_count", framesRef.current.size);
        setGauge("ws.robot_state_count", robotStatesRef.current.size);
      }
    }, BATCH_INTERVAL_MS);

    const streamInfoInterval = setInterval(() => {
      if (wsRef.current?.readyState === WebSocket.OPEN) {
        wsRef.current.send(encodeCommand("get_stream_info"));
      }
    }, 1000);

    function connect() {
      if (!mountedRef.current) return;

      const ws = new WebSocket(url);
      wsRef.current = ws;

      ws.binaryType = "nodebuffer";

      ws.on("open", () => {
        if (!mountedRef.current) return;
        reconnectAttemptRef.current = 0;
        setConnected(true);
        setGauge("ws.connected", "Connected");
        ws.send(encodeCommand("get_stream_info"));
      });

      ws.on("message", (data: Buffer | string, isBinary: boolean) => {
        if (!mountedRef.current) return;

        if (isBinary && Buffer.isBuffer(data)) {
          const parseStartMs = nowMs();
          const msg = parseBinaryMessage(data);
          recordTiming("ws.parse.binary", nowMs() - parseStartMs);
          if (msg) {
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
          }
        } else {
          const text = typeof data === "string" ? data : data.toString("utf-8");
          const parseStartMs = nowMs();
          const msg = parseJsonMessage(text);
          recordTiming("ws.parse.json", nowMs() - parseStartMs);
          if (msg?.type === "robot_state") {
            robotStatesRef.current.set(msg.name, msg);
            dirtyRef.current = true;
            incrementGauge("ws.robot_messages_total");
            setGauge("ws.robot_state_count", robotStatesRef.current.size);
          } else if (msg?.type === "stream_info") {
            streamInfoRef.current = msg;
            dirtyRef.current = true;
            setGauge("ws.stream_info_status", "Ready");
            setGauge("ws.preview_fps_config", msg.configured_preview_fps);
            setGauge(
              "ws.active_preview_size",
              `${msg.active_preview_width}x${msg.active_preview_height}`,
            );
          } else if (msg?.type === "episode_status") {
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
        }
      });

      ws.on("close", () => {
        if (!mountedRef.current) return;
        setConnected(false);
        setGauge("ws.connected", "Disconnected");
        setGauge("ws.stream_info_status", "Unavailable");
        setGauge("ws.active_preview_size", "Unavailable");
        setGauge("ws.episode_status", "Unavailable");
        setGauge("ws.setup_status", "Unavailable");
        streamInfoRef.current = null;
        episodeStatusRef.current = null;
        setupStateRef.current = null;
        dirtyRef.current = true;
        wsRef.current = null;
        scheduleReconnect();
      });

      ws.on("error", (err: Error) => {
        // Suppress ECONNREFUSED noise during reconnect
        if (
          !("code" in err) ||
          (err as NodeJS.ErrnoException).code !== "ECONNREFUSED"
        ) {
          // Only log unexpected errors
        }
        // Close will fire after error, triggering reconnect
      });
    }

    function scheduleReconnect() {
      if (!mountedRef.current) return;
      const attempt = reconnectAttemptRef.current;
      const delay =
        RECONNECT_DELAYS[Math.min(attempt, RECONNECT_DELAYS.length - 1)];
      reconnectAttemptRef.current = attempt + 1;
      reconnectTimerRef.current = setTimeout(connect, delay);
    }

    connect();

    return () => {
      mountedRef.current = false;
      clearInterval(flushInterval);
      clearInterval(streamInfoInterval);
      if (reconnectTimerRef.current) {
        clearTimeout(reconnectTimerRef.current);
      }
      if (wsRef.current) {
        wsRef.current.close();
        wsRef.current = null;
      }
      streamInfoRef.current = null;
      episodeStatusRef.current = null;
      setupStateRef.current = null;
      setGauge("ws.connected", "Disconnected");
    };
  }, [url]);

  return {
    frames,
    robotStates,
    streamInfo,
    episodeStatus,
    setupState,
    connected,
    send,
  };
}
