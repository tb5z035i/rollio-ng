/**
 * Camera stream panel: decodes JPEG frames via sharp (native async)
 * and renders as ANSI half-block art.
 *
 * Performance:
 * - sharp decode+resize runs in libvips native thread pool (non-blocking)
 * - ANSI rendering uses pre-computed LUT for O(1) color mapping
 * - Only re-decodes when jpegData reference changes
 */

import React, { useState, useEffect, useRef, useMemo } from "react";
import { Box, Text } from "ink";
import sharp from "sharp";
import { renderToAnsi } from "../lib/ansi-renderer.js";

interface StreamPanelProps {
  jpegData: Buffer | null;
  name: string;
  panelWidth: number;
  panelHeight: number;
}

export function StreamPanel({
  jpegData,
  name,
  panelWidth,
  panelHeight,
}: StreamPanelProps) {
  const [rgbPixels, setRgbPixels] = useState<Buffer | null>(null);
  const [pixelWidth, setPixelWidth] = useState(0);
  const [pixelHeight, setPixelHeight] = useState(0);
  const lastJpegRef = useRef<Buffer | null>(null);
  const decodeSeqRef = useRef(0);

  // Target dimensions for decode+resize (account for borders)
  const targetWidth = Math.max(1, panelWidth - 2);
  const targetHeight = Math.max(2, (panelHeight - 2) * 2); // ×2 for half-block

  useEffect(() => {
    // Skip if same JPEG buffer reference
    if (jpegData === lastJpegRef.current) return;
    lastJpegRef.current = jpegData;

    if (!jpegData || jpegData.length === 0) {
      setRgbPixels(null);
      return;
    }

    // Increment sequence number to handle out-of-order async completions
    const seq = ++decodeSeqRef.current;

    // Async decode+resize via sharp (native, non-blocking)
    sharp(jpegData)
      .resize(targetWidth, targetHeight, { fit: "fill" })
      .removeAlpha()
      .raw()
      .toBuffer({ resolveWithObject: true })
      .then(({ data, info }) => {
        // Only apply if this is still the latest decode request
        if (seq === decodeSeqRef.current) {
          setRgbPixels(data);
          setPixelWidth(info.width);
          setPixelHeight(info.height);
        }
      })
      .catch(() => {
        // Silently ignore decode errors (corrupted frame, etc.)
      });
  }, [jpegData, targetWidth, targetHeight]);

  // Render ANSI art from decoded pixels (memoized on pixel data)
  const ansiOutput = useMemo(() => {
    if (!rgbPixels || pixelWidth === 0 || pixelHeight === 0) return null;
    return renderToAnsi(rgbPixels, pixelWidth, pixelHeight);
  }, [rgbPixels, pixelWidth, pixelHeight]);

  // Border chars
  const headerText = `─ ${name} `;
  const headerPad = Math.max(0, panelWidth - headerText.length - 2);
  const topBorder = `┌${headerText}${"─".repeat(headerPad)}┐`;
  const bottomBorder = `└${"─".repeat(panelWidth - 2)}┘`;

  // Content area height (in character rows)
  const contentHeight = Math.max(1, panelHeight - 2);

  if (!ansiOutput) {
    // Placeholder: no signal
    const placeholderLines: string[] = [];
    for (let i = 0; i < contentHeight; i++) {
      if (i === Math.floor(contentHeight / 2)) {
        const msg = "╌ No signal ╌";
        const pad = Math.max(0, panelWidth - 2 - msg.length);
        const left = Math.floor(pad / 2);
        const right = pad - left;
        placeholderLines.push(
          `│${" ".repeat(left)}${msg}${" ".repeat(right)}│`,
        );
      } else {
        placeholderLines.push(`│${" ".repeat(panelWidth - 2)}│`);
      }
    }

    return (
      <Box flexDirection="column" width={panelWidth}>
        <Text dimColor>{topBorder}</Text>
        {placeholderLines.map((line, i) => (
          <Text key={i} dimColor>
            {line}
          </Text>
        ))}
        <Text dimColor>{bottomBorder}</Text>
      </Box>
    );
  }

  // Render with ANSI content
  const lines = ansiOutput.split("\n");

  return (
    <Box flexDirection="column" width={panelWidth}>
      <Text dimColor>{topBorder}</Text>
      {lines.map((line, i) => (
        <Text key={i}>
          {"│"}
          {line}
          {"\x1b[0m│"}
        </Text>
      ))}
      <Text dimColor>{bottomBorder}</Text>
    </Box>
  );
}
