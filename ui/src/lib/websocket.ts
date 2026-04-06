/**
 * WebSocket client hook with auto-reconnect.
 *
 * Performance optimizations:
 * - Uses useRef for mutable frame/state maps to avoid re-renders on every message
 * - Batches state updates at ~30fps via setInterval to coalesce rapid updates
 * - Tracks frame references for change detection
 */

import { useState, useEffect, useRef, useCallback } from "react";
import WebSocket from "ws";
import {
  parseBinaryMessage,
  parseJsonMessage,
  type CameraFrameMessage,
  type RobotStateMessage,
} from "./protocol.js";

/** Camera frame data for rendering. */
export interface CameraFrame {
  jpegData: Buffer;
  width: number;
  height: number;
  sequence: number;
}

/** Return type of the useWebSocket hook. */
export interface WebSocketState {
  frames: Map<string, CameraFrame>;
  robotStates: Map<string, RobotStateMessage>;
  connected: boolean;
  send: (msg: string) => void;
}

const RECONNECT_DELAYS = [1000, 2000, 4000, 10000]; // exponential backoff
const BATCH_INTERVAL_MS = 33; // ~30fps state flush

/**
 * React hook that manages a WebSocket connection to the Visualizer.
 *
 * Automatically reconnects on disconnect with exponential backoff.
 * Batches incoming data updates to React state at ~30fps to avoid
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

  // Mutable refs for accumulating data between batch flushes
  const framesRef = useRef<Map<string, CameraFrame>>(new Map());
  const robotStatesRef = useRef<Map<string, RobotStateMessage>>(new Map());
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

    // Batch flush interval: push ref data into React state at ~30fps
    const flushInterval = setInterval(() => {
      if (dirtyRef.current && mountedRef.current) {
        dirtyRef.current = false;
        setFrames(new Map(framesRef.current));
        setRobotStates(new Map(robotStatesRef.current));
      }
    }, BATCH_INTERVAL_MS);

    function connect() {
      if (!mountedRef.current) return;

      const ws = new WebSocket(url);
      wsRef.current = ws;

      ws.binaryType = "nodebuffer";

      ws.on("open", () => {
        if (!mountedRef.current) return;
        reconnectAttemptRef.current = 0;
        setConnected(true);
      });

      ws.on("message", (data: Buffer | string, isBinary: boolean) => {
        if (!mountedRef.current) return;

        if (isBinary && Buffer.isBuffer(data)) {
          const msg = parseBinaryMessage(data);
          if (msg) {
            const sequence = ++frameSequenceRef.current;
            framesRef.current.set(msg.name, {
              jpegData: msg.jpegData,
              width: msg.width,
              height: msg.height,
              sequence,
            });
            dirtyRef.current = true;
          }
        } else {
          const text = typeof data === "string" ? data : data.toString("utf-8");
          const msg = parseJsonMessage(text);
          if (msg) {
            robotStatesRef.current.set(msg.name, msg);
            dirtyRef.current = true;
          }
        }
      });

      ws.on("close", () => {
        if (!mountedRef.current) return;
        setConnected(false);
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
      if (reconnectTimerRef.current) {
        clearTimeout(reconnectTimerRef.current);
      }
      if (wsRef.current) {
        wsRef.current.close();
        wsRef.current = null;
      }
    };
  }, [url]);

  return { frames, robotStates, connected, send };
}
