/**
 * Camera stream panel logic: decodes JPEG frames via sharp (native async)
 * and renders as ANSI half-block art lines.
 *
 * Uses a single combined decode effect for all cameras to avoid
 * React hooks-in-loop violations. Merges multiple camera panels into
 * single pre-composed <Text> lines so Ink doesn't try to measure ANSI widths.
 */

import React, { useState, useEffect, useRef, useMemo } from "react";
import { Box, Text } from "ink";
import sharp from "sharp";
import { renderToAnsiLines } from "../lib/ansi-renderer.js";
import type { CameraFrame } from "../lib/websocket.js";

const RESET = "\x1b[0m";

/** Decoded camera frame as ANSI lines. */
interface DecodedFrame {
  lines: string[];
  /** The pixel width these lines were decoded at. */
  decodedWidth: number;
  /** The pixel height these lines were decoded at. */
  decodedHeight: number;
}

interface PendingDecode {
  key: string;
  jpegData: Buffer;
}

interface CameraRowProps {
  cameras: Array<{ name: string; frame: CameraFrame | undefined }>;
  /** Total width available for ALL cameras combined (excluding info panel). */
  totalWidth: number;
  panelHeight: number;
  infoPanelLines?: string[];
  /** If true, the right border connects to an adjacent info panel. */
  hasRightPanel?: boolean;
}

/**
 * Renders multiple camera panels side-by-side as pre-composed text lines.
 *
 * Instead of using Ink's flexbox (which can't measure ANSI escape codes),
 * this component manually merges each camera's ANSI lines into single
 * combined strings with proper box-drawing borders.
 *
 * Width math (visible chars):
 *   totalWidth includes the outer left │ and outer right │.
 *   With N cameras and (N-1) inner separator │ chars plus 2 outer │:
 *   perCameraContentWidth = floor((totalWidth - 2 - (N-1)) / N)
 */
export function CameraRow({
  cameras,
  totalWidth,
  panelHeight,
  infoPanelLines,
  hasRightPanel = false,
}: CameraRowProps) {
  const numCams = cameras.length;
  // 2 for outer borders, (numCams-1) for inner separators
  const innerSeparators = numCams - 1;
  const perCamWidth = Math.max(
    4,
    Math.floor((totalWidth - 2 - innerSeparators) / numCams),
  );
  const contentCharHeight = Math.max(1, panelHeight - 2); // minus top/bottom border
  const targetPixelHeight = Math.max(2, contentCharHeight * 2); // ×2 for half-block

  // Track decoded frames for all cameras
  const [decodedFrames, setDecodedFrames] = useState<Map<string, DecodedFrame>>(
    () => new Map(),
  );
  const requestedDecodeKeyRef = useRef<Map<string, string>>(new Map());
  const pendingDecodeRef = useRef<Map<string, PendingDecode>>(new Map());
  const activeDecodeRef = useRef<Set<string>>(new Set());
  const isMountedRef = useRef(true);

  const clearDecodedFrame = (camName: string) => {
    setDecodedFrames((prev) => {
      if (!prev.has(camName)) return prev;
      const next = new Map(prev);
      next.delete(camName);
      return next;
    });
  };

  useEffect(() => {
    isMountedRef.current = true;
    return () => {
      isMountedRef.current = false;
      requestedDecodeKeyRef.current.clear();
      pendingDecodeRef.current.clear();
      activeDecodeRef.current.clear();
    };
  }, []);

  useEffect(() => {
    const black = { r: 0, g: 0, b: 0 };
    const activeNames = new Set(cameras.map((cam) => cam.name));

    const pumpDecode = (camName: string) => {
      if (activeDecodeRef.current.has(camName)) return;
      const initialPending = pendingDecodeRef.current.get(camName);
      if (!initialPending) return;

      activeDecodeRef.current.add(camName);

      void (async () => {
        try {
          let pending: PendingDecode | undefined = initialPending;

          while (isMountedRef.current && pending) {
            pendingDecodeRef.current.delete(camName);

            try {
              const { data, info } = await sharp(pending.jpegData)
                .flatten({ background: black })
                .resize(perCamWidth, targetPixelHeight, {
                  fit: "fill",
                  kernel: sharp.kernel.nearest,
                })
                .raw()
                .toBuffer({ resolveWithObject: true });

              if (!isMountedRef.current) return;
              if (requestedDecodeKeyRef.current.get(camName) !== pending.key) {
                pending = pendingDecodeRef.current.get(camName);
                continue;
              }

              const lines = renderToAnsiLines(data, info.width, info.height);
              setDecodedFrames((prev) => {
                const next = new Map(prev);
                next.set(camName, {
                  lines,
                  decodedWidth: info.width,
                  decodedHeight: info.height,
                });
                return next;
              });
            } catch {
              if (!isMountedRef.current) return;
            }

            pending = pendingDecodeRef.current.get(camName);
          }
        } finally {
          activeDecodeRef.current.delete(camName);
          if (isMountedRef.current && pendingDecodeRef.current.has(camName)) {
            pumpDecode(camName);
          }
        }
      })();
    };

    for (const cam of cameras) {
      const frame = cam.frame;

      if (!frame?.jpegData || frame.jpegData.length === 0) {
        requestedDecodeKeyRef.current.delete(cam.name);
        pendingDecodeRef.current.delete(cam.name);
        clearDecodedFrame(cam.name);
        continue;
      }

      const decodeKey = `${frame.sequence}:${perCamWidth}x${targetPixelHeight}`;
      if (requestedDecodeKeyRef.current.get(cam.name) === decodeKey) {
        continue;
      }

      requestedDecodeKeyRef.current.set(cam.name, decodeKey);
      pendingDecodeRef.current.set(cam.name, {
        key: decodeKey,
        jpegData: frame.jpegData,
      });
      pumpDecode(cam.name);
    }

    for (const name of Array.from(requestedDecodeKeyRef.current.keys())) {
      if (activeNames.has(name)) continue;
      requestedDecodeKeyRef.current.delete(name);
      pendingDecodeRef.current.delete(name);
      clearDecodedFrame(name);
    }
  }, [cameras, perCamWidth, targetPixelHeight]);

  // Build merged output lines with proper box-drawing borders
  const outputLines = useMemo(() => {
    const result: string[] = [];

    // Right-edge chars depend on whether an info panel is attached
    const topRight = hasRightPanel ? "┬" : "┐";
    const midRight = hasRightPanel ? "│" : "│";
    const botRight = hasRightPanel ? "┴" : "┘";

    // === Top border: ┌─ camera_0 ─┬─ camera_1 ─┐  (or ┬ if info panel) ===
    let topLine = "┌";
    for (let c = 0; c < numCams; c++) {
      const name = cameras[c].name;
      const label = `─ ${name} `;
      const remaining = Math.max(0, perCamWidth - label.length);
      topLine += label + "─".repeat(remaining);
      topLine += c < numCams - 1 ? "┬" : topRight;
    }
    result.push(topLine);

    // === Content lines: │<ansi>│<ansi>│ ===
    for (let row = 0; row < contentCharHeight; row++) {
      let line = "│";
      for (let c = 0; c < numCams; c++) {
        const decoded = decodedFrames.get(cameras[c].name);
        if (decoded && row < decoded.lines.length) {
          line += decoded.lines[row] + "\x1b[0m";
        } else {
          if (row === Math.floor(contentCharHeight / 2)) {
            const msg = "╌ No signal ╌";
            const pad = Math.max(0, perCamWidth - msg.length);
            const left = Math.floor(pad / 2);
            const right = pad - left;
            line += " ".repeat(left) + msg + " ".repeat(right);
          } else {
            line += " ".repeat(perCamWidth);
          }
        }
        line += c < numCams - 1 ? "│" : midRight;
      }
      result.push(line);
    }

    // === Bottom border: └──────┴──────┘  (or ┴ if info panel) ===
    let bottomLine = "└";
    for (let c = 0; c < numCams; c++) {
      bottomLine += "─".repeat(perCamWidth);
      bottomLine += c < numCams - 1 ? "┴" : botRight;
    }
    result.push(bottomLine);

    return result;
  }, [cameras, decodedFrames, numCams, perCamWidth, contentCharHeight]);

  // Merge info panel lines on the right if provided
  const finalLines = useMemo(() => {
    if (!infoPanelLines || infoPanelLines.length === 0) {
      return outputLines.map((line) => line + RESET);
    }

    return outputLines.map((line, i) => {
      const infoLine = i < infoPanelLines.length ? infoPanelLines[i] : "";
      return line + infoLine + RESET;
    });
  }, [outputLines, infoPanelLines]);

  return (
    <Box flexDirection="column">
      {finalLines.map((line, i) => (
        <Text key={i} wrap="end">
          {line}
        </Text>
      ))}
    </Box>
  );
}
