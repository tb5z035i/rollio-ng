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

/** Decoded camera frame as ANSI lines. */
interface DecodedFrame {
  lines: string[];
}

interface CameraRowProps {
  cameras: Array<{ name: string; frame: CameraFrame | undefined }>;
  /** Total width available for ALL cameras combined (excluding info panel). */
  totalWidth: number;
  panelHeight: number;
  infoPanelLines?: string[];
}

/**
 * Renders multiple camera panels side-by-side as pre-composed text lines.
 *
 * Instead of using Ink's flexbox (which can't measure ANSI escape codes),
 * this component manually merges each camera's ANSI lines into single
 * combined strings. Each output <Text> has the correct visible width.
 *
 * Width math:
 *   With N cameras and (N-1) separator │ chars:
 *   perCameraWidth = floor((totalWidth - (N-1)) / N)
 *   Each camera's ANSI content is exactly perCameraWidth visible chars wide.
 *   Headers and bottom borders match this width.
 */
export function CameraRow({
  cameras,
  totalWidth,
  panelHeight,
  infoPanelLines,
}: CameraRowProps) {
  const numCams = cameras.length;
  const separators = numCams - 1;
  const perCamWidth = Math.max(4, Math.floor((totalWidth - separators) / numCams));
  const contentCharHeight = Math.max(1, panelHeight - 2); // minus top/bottom border
  const targetPixelHeight = Math.max(2, contentCharHeight * 2); // ×2 for half-block

  // Track decoded frames for all cameras in a single state object
  const [decodedFrames, setDecodedFrames] = useState<Map<string, DecodedFrame>>(
    () => new Map(),
  );
  const lastJpegsRef = useRef<Map<string, Buffer | null>>(new Map());

  // Single effect that decodes all cameras
  useEffect(() => {
    let cancelled = false;

    for (const cam of cameras) {
      const jpegData = cam.frame?.jpegData ?? null;
      const lastJpeg = lastJpegsRef.current.get(cam.name) ?? null;

      if (jpegData === lastJpeg) continue;
      lastJpegsRef.current.set(cam.name, jpegData);

      if (!jpegData || jpegData.length === 0) {
        setDecodedFrames((prev) => {
          const next = new Map(prev);
          next.delete(cam.name);
          return next;
        });
        continue;
      }

      const camName = cam.name;

      sharp(jpegData)
        .resize(perCamWidth, targetPixelHeight, { fit: "fill" })
        .removeAlpha()
        .raw()
        .toBuffer({ resolveWithObject: true })
        .then(({ data, info }) => {
          if (cancelled) return;
          const lines = renderToAnsiLines(data, info.width, info.height);
          setDecodedFrames((prev) => {
            const next = new Map(prev);
            next.set(camName, { lines });
            return next;
          });
        })
        .catch(() => {});
    }

    return () => {
      cancelled = true;
    };
  }, [cameras, perCamWidth, targetPixelHeight]);

  // Build merged output lines
  const outputLines = useMemo(() => {
    const result: string[] = [];

    // === Top border ===
    let topLine = "";
    for (let c = 0; c < numCams; c++) {
      const name = cameras[c].name;
      const label = `── ${name} `;
      const remaining = Math.max(0, perCamWidth - label.length);
      topLine += label + "─".repeat(remaining);
      if (c < numCams - 1) topLine += "┬";
    }
    result.push(topLine);

    // === Content lines ===
    for (let row = 0; row < contentCharHeight; row++) {
      let line = "";
      for (let c = 0; c < numCams; c++) {
        const decoded = decodedFrames.get(cameras[c].name);
        if (decoded && row < decoded.lines.length) {
          // ANSI content — exactly perCamWidth visible chars
          line += decoded.lines[row] + "\x1b[0m";
        } else {
          // Placeholder
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
        if (c < numCams - 1) line += "│";
      }
      result.push(line);
    }

    // === Bottom border ===
    let bottomLine = "";
    for (let c = 0; c < numCams; c++) {
      bottomLine += "─".repeat(perCamWidth);
      if (c < numCams - 1) bottomLine += "┴";
    }
    result.push(bottomLine);

    return result;
  }, [cameras, decodedFrames, numCams, perCamWidth, contentCharHeight]);

  // Merge info panel lines on the right if provided
  const finalLines = useMemo(() => {
    if (!infoPanelLines || infoPanelLines.length === 0) return outputLines;

    return outputLines.map((line, i) => {
      const infoLine = i < infoPanelLines.length ? infoPanelLines[i] : "";
      return line + infoLine;
    });
  }, [outputLines, infoPanelLines]);

  return (
    <Box flexDirection="column">
      {finalLines.map((line, i) => (
        <Text key={i}>{line}</Text>
      ))}
    </Box>
  );
}
