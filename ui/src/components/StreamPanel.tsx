import React, { useEffect, useMemo, useRef, useState } from "react";
import { Box, Text } from "ink";
import sharp from "sharp";
import type { CameraFrame } from "../lib/websocket.js";
import {
  incrementGauge,
  nowMs,
  recordTiming,
  setGauge,
} from "../lib/debug-metrics.js";
import {
  createAsciiRendererBackend,
  type AsciiRenderLayout,
} from "../lib/renderers/index.js";

const RESET = "\x1b[0m";
const DECODE_COMMIT_INTERVAL_MS = 16;
const SHARP_DECODE_CONCURRENCY = 6;
const TARGET_TOTAL_DECODE_FPS = 360;
const MAX_DECODE_FPS_PER_CAMERA = 60;
const MIN_DECODE_FPS_PER_CAMERA = 30;
const BLACK_BACKGROUND = { r: 0, g: 0, b: 0, alpha: 1 };
const CAMERA_RENDERER_ID = "ts-harri";

sharp.concurrency(SHARP_DECODE_CONCURRENCY);

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

interface PresentedFrame {
  lines: string[];
  frameKey: string;
  sourceTimestampNs: number;
  sourceFrameIndex: number;
  decodedWidth: number;
  decodedHeight: number;
  outputBytes: number;
}

interface PendingDecode {
  key: string;
  jpegData: Buffer;
  sourceTimestampNs: number;
  sourceFrameIndex: number;
  previewWidth: number;
  previewHeight: number;
}

interface CameraRowProps {
  cameras: Array<{ name: string; frame: CameraFrame | undefined }>;
  previewRaster: CameraPreviewRaster;
  infoPanelLines?: string[];
  hasRightPanel?: boolean;
}

export interface CameraPreviewRaster {
  columns: number;
  rows: number;
  width: number;
  height: number;
}

interface PreparedRaster {
  data: Buffer;
  width: number;
  height: number;
}

function describeCameraCellGrid(
  totalWidth: number,
  panelHeight: number,
  numCameras: number,
): Pick<CameraPreviewRaster, "columns" | "rows"> {
  const safeCameraCount = Math.max(1, numCameras);
  const innerSeparators = safeCameraCount - 1;
  return {
    columns: Math.max(
      4,
      Math.floor((totalWidth - 2 - innerSeparators) / safeCameraCount),
    ),
    rows: Math.max(1, panelHeight - 2),
  };
}

export function describeCameraPreviewRaster(
  totalWidth: number,
  panelHeight: number,
  numCameras: number,
): CameraPreviewRaster {
  const grid = describeCameraCellGrid(totalWidth, panelHeight, numCameras);
  const raster = createAsciiRendererBackend(CAMERA_RENDERER_ID).describeRaster({
    columns: grid.columns,
    rows: grid.rows,
  });
  return {
    ...grid,
    width: raster.width,
    height: raster.height,
  };
}

export function CameraRow({
  cameras,
  previewRaster,
  infoPanelLines,
  hasRightPanel = false,
}: CameraRowProps) {
  const numCams = cameras.length;
  const perCamWidth = previewRaster.columns;
  const contentCharHeight = previewRaster.rows;
  const rendererRaster = useMemo(
    () => ({ width: previewRaster.width, height: previewRaster.height }),
    [previewRaster.height, previewRaster.width],
  );
  const renderLayout = useMemo<AsciiRenderLayout>(
    () => ({
      columns: perCamWidth,
      rows: contentCharHeight,
    }),
    [contentCharHeight, perCamWidth],
  );
  const perCameraDecodeFps = Math.max(
    MIN_DECODE_FPS_PER_CAMERA,
    Math.min(
      MAX_DECODE_FPS_PER_CAMERA,
      Math.floor(TARGET_TOTAL_DECODE_FPS / Math.max(1, numCams)),
    ),
  );
  const perCameraDecodeIntervalMs = 1000 / perCameraDecodeFps;
  const asciiRendererBackend = useMemo(
    () => createAsciiRendererBackend(CAMERA_RENDERER_ID),
    [],
  );

  const [presentedFrames, setPresentedFrames] = useState<Map<string, PresentedFrame>>(
    () => new Map(),
  );
  const presentedFramesRef = useRef<Map<string, PresentedFrame>>(new Map());
  const presentedFramesDirtyRef = useRef(false);
  const committedFrameKeyRef = useRef<Map<string, string>>(new Map());
  const requestedDecodeKeyRef = useRef<Map<string, string>>(new Map());
  const pendingDecodeRef = useRef<Map<string, PendingDecode>>(new Map());
  const activeDecodeRef = useRef<Set<string>>(new Set());
  const lastDecodeStartedAtRef = useRef<Map<string, number>>(new Map());
  const renderQueueTailRef = useRef<Promise<void>>(Promise.resolve());
  const queuedRenderCountRef = useRef(0);
  const activeRenderCameraRef = useRef<string | null>(null);
  const isMountedRef = useRef(true);

  const clearPresentedFrame = (cameraName: string) => {
    if (!presentedFramesRef.current.has(cameraName)) {
      return;
    }
    presentedFramesRef.current.delete(cameraName);
    presentedFramesDirtyRef.current = true;
  };

  const updateDecodeGauges = () => {
    setGauge("stream.pending_decodes", pendingDecodeRef.current.size);
    setGauge("stream.active_decodes", activeDecodeRef.current.size);
    setGauge("stream.renderer_backend", asciiRendererBackend.id);
    setGauge("stream.renderer_kind", asciiRendererBackend.kind);
    setGauge("stream.renderer_algorithm", asciiRendererBackend.algorithm);
    setGauge("stream.output_columns", renderLayout.columns);
    setGauge("stream.output_rows", renderLayout.rows);
    setGauge("stream.target_width", rendererRaster.width);
    setGauge("stream.target_height", rendererRaster.height);
    setGauge("stream.target_char_height", contentCharHeight);
    setGauge("stream.target_cells_per_camera", perCamWidth * contentCharHeight);
    setGauge(
      "stream.target_pixels_per_camera",
      rendererRaster.width * rendererRaster.height,
    );
    setGauge("stream.decode_fps_cap", perCameraDecodeFps);
    setGauge("stream.decode_interval_ms", perCameraDecodeIntervalMs);
    setGauge("stream.sharp_concurrency", SHARP_DECODE_CONCURRENCY);
    setGauge("stream.render_queue_depth", queuedRenderCountRef.current);
    setGauge("stream.render_active_camera", activeRenderCameraRef.current ?? "Idle");
  };

  useEffect(() => {
    isMountedRef.current = true;
    setGauge("stream.frames_presented_total", 0);
    const flushPresentedFrames = setInterval(() => {
      if (!isMountedRef.current || !presentedFramesDirtyRef.current) {
        return;
      }

      const flushStartMs = nowMs();
      presentedFramesDirtyRef.current = false;
      let presentedFrameCount = 0;
      for (const [cameraName, frame] of presentedFramesRef.current) {
        if (committedFrameKeyRef.current.get(cameraName) === frame.frameKey) {
          continue;
        }
        committedFrameKeyRef.current.set(cameraName, frame.frameKey);
        presentedFrameCount += 1;
        incrementGauge(`stream.frames_presented_total.${cameraName}`);
        const displayedLatencyMs = Math.max(
          0,
          Date.now() - frame.sourceTimestampNs / 1_000_000,
        );
        setGauge(`stream.display_latency_ms.${cameraName}`, displayedLatencyMs);
        setGauge(
          `stream.displayed_source_timestamp_ns.${cameraName}`,
          frame.sourceTimestampNs,
        );
        setGauge(`stream.displayed_frame_index.${cameraName}`, frame.sourceFrameIndex);
        recordTiming("stream.latency.displayed", displayedLatencyMs);
      }

      for (const cameraName of Array.from(committedFrameKeyRef.current.keys())) {
        if (!presentedFramesRef.current.has(cameraName)) {
          committedFrameKeyRef.current.delete(cameraName);
        }
      }

      setPresentedFrames(new Map(presentedFramesRef.current));
      recordTiming("stream.decode.commit", nowMs() - flushStartMs);
      setGauge("stream.decoded_frames", presentedFramesRef.current.size);
      if (presentedFrameCount > 0) {
        incrementGauge("stream.frames_presented_total", presentedFrameCount);
      }
    }, DECODE_COMMIT_INTERVAL_MS);

    return () => {
      isMountedRef.current = false;
      clearInterval(flushPresentedFrames);
      void asciiRendererBackend.dispose?.();
      presentedFramesRef.current.clear();
      presentedFramesDirtyRef.current = false;
      committedFrameKeyRef.current.clear();
      requestedDecodeKeyRef.current.clear();
      pendingDecodeRef.current.clear();
      activeDecodeRef.current.clear();
      lastDecodeStartedAtRef.current.clear();
      renderQueueTailRef.current = Promise.resolve();
      queuedRenderCountRef.current = 0;
      activeRenderCameraRef.current = null;
    };
  }, [asciiRendererBackend]);

  useEffect(() => {
    void asciiRendererBackend.prepare?.();
  }, [asciiRendererBackend]);

  useEffect(() => {
    requestedDecodeKeyRef.current.clear();
    pendingDecodeRef.current.clear();
    lastDecodeStartedAtRef.current.clear();
    committedFrameKeyRef.current.clear();
    presentedFramesRef.current.clear();
    presentedFramesDirtyRef.current = true;
    updateDecodeGauges();
  }, [
    renderLayout.columns,
    renderLayout.rows,
    rendererRaster.height,
    rendererRaster.width,
  ]);

  useEffect(() => {
    const activeNames = new Set(cameras.map((camera) => camera.name));

    const waitForRenderTurn = async (cameraName: string) => {
      queuedRenderCountRef.current += 1;
      updateDecodeGauges();

      let releaseRenderTurn: (() => void) | undefined;
      const queuedTurn = new Promise<void>((resolve) => {
        releaseRenderTurn = resolve;
      });
      const previousTurn = renderQueueTailRef.current.catch(() => undefined);
      renderQueueTailRef.current = previousTurn.then(() => queuedTurn);
      await previousTurn;

      queuedRenderCountRef.current = Math.max(0, queuedRenderCountRef.current - 1);
      activeRenderCameraRef.current = cameraName;
      updateDecodeGauges();

      return () => {
        activeRenderCameraRef.current = null;
        releaseRenderTurn?.();
        updateDecodeGauges();
      };
    };

    const pumpDecode = (cameraName: string) => {
      if (activeDecodeRef.current.has(cameraName)) {
        return;
      }

      const initialPending = pendingDecodeRef.current.get(cameraName);
      if (!initialPending) {
        return;
      }

      activeDecodeRef.current.add(cameraName);
      updateDecodeGauges();

      void (async () => {
        try {
          let pending: PendingDecode | undefined = initialPending;

          while (isMountedRef.current && pending) {
            pendingDecodeRef.current.delete(cameraName);
            updateDecodeGauges();

            try {
              const lastDecodeStartedAt =
                lastDecodeStartedAtRef.current.get(cameraName);
              if (lastDecodeStartedAt !== undefined) {
                const waitMs =
                  lastDecodeStartedAt + perCameraDecodeIntervalMs - nowMs();
                if (waitMs > 1) {
                  recordTiming("stream.decode.wait", waitMs);
                  await sleep(waitMs);
                  if (!isMountedRef.current) {
                    return;
                  }

                  const latestPending = pendingDecodeRef.current.get(cameraName);
                  if (latestPending) {
                    pending = latestPending;
                    pendingDecodeRef.current.delete(cameraName);
                    updateDecodeGauges();
                  }
                }
              }

              lastDecodeStartedAtRef.current.set(cameraName, nowMs());
              const totalDecodeStartMs = nowMs();
              const resizeStartMs = nowMs();
              const preparedRaster = await prepareRendererRaster(
                pending.jpegData,
                rendererRaster.width,
                rendererRaster.height,
              );
              const resizeDurationMs = nowMs() - resizeStartMs;

              if (!isMountedRef.current) {
                return;
              }
              if (requestedDecodeKeyRef.current.get(cameraName) !== pending.key) {
                incrementGauge("stream.render_stale_drops");
                incrementGauge(`stream.render_stale_drops.${cameraName}`);
                pending = pendingDecodeRef.current.get(cameraName);
                continue;
              }

              const renderQueueWaitStartMs = nowMs();
              const releaseRenderTurn = await waitForRenderTurn(cameraName);
              let renderResult;
              try {
                const renderQueueWaitMs = nowMs() - renderQueueWaitStartMs;
                recordTiming("stream.render.queue_wait", renderQueueWaitMs);
                recordTiming(
                  `stream.render.queue_wait.${cameraName}`,
                  renderQueueWaitMs,
                );
                if (!isMountedRef.current) {
                  return;
                }
                if (requestedDecodeKeyRef.current.get(cameraName) !== pending.key) {
                  incrementGauge("stream.render_stale_drops");
                  incrementGauge(`stream.render_stale_drops.${cameraName}`);
                  pending = pendingDecodeRef.current.get(cameraName);
                  continue;
                }

                renderResult = await asciiRendererBackend.render({
                  pixels: preparedRaster.data,
                  width: preparedRaster.width,
                  height: preparedRaster.height,
                  layout: renderLayout,
                });
              } finally {
                releaseRenderTurn();
              }

              const totalDecodeDurationMs = nowMs() - totalDecodeStartMs;
              recordTiming("stream.decode.resize", resizeDurationMs);
              recordTiming(`stream.decode.resize.${cameraName}`, resizeDurationMs);
              recordTiming(
                "stream.render.total",
                renderResult.stats.timings.totalMs,
              );
              recordTiming(
                `stream.render.total.${cameraName}`,
                renderResult.stats.timings.totalMs,
              );
              if (renderResult.stats.timings.sampleMs !== undefined) {
                recordTiming(
                  "stream.render.sample",
                  renderResult.stats.timings.sampleMs,
                );
                recordTiming(
                  `stream.render.sample.${cameraName}`,
                  renderResult.stats.timings.sampleMs,
                );
              }
              if (renderResult.stats.timings.lookupMs !== undefined) {
                recordTiming(
                  "stream.render.lookup",
                  renderResult.stats.timings.lookupMs,
                );
                recordTiming(
                  `stream.render.lookup.${cameraName}`,
                  renderResult.stats.timings.lookupMs,
                );
              }
              if (renderResult.stats.timings.assembleMs !== undefined) {
                recordTiming(
                  "stream.render.assemble",
                  renderResult.stats.timings.assembleMs,
                );
                recordTiming(
                  `stream.render.assemble.${cameraName}`,
                  renderResult.stats.timings.assembleMs,
                );
              }
              if (renderResult.stats.timings.ansiMs !== undefined) {
                recordTiming("stream.decode.ansi", renderResult.stats.timings.ansiMs);
                recordTiming(
                  `stream.decode.ansi.${cameraName}`,
                  renderResult.stats.timings.ansiMs,
                );
              }
              if (renderResult.stats.timings.adapterMs !== undefined) {
                recordTiming(
                  "stream.render.adapter",
                  renderResult.stats.timings.adapterMs,
                );
                recordTiming(
                  `stream.render.adapter.${cameraName}`,
                  renderResult.stats.timings.adapterMs,
                );
              }
              recordTiming("stream.decode.total", totalDecodeDurationMs);
              recordTiming(`stream.decode.total.${cameraName}`, totalDecodeDurationMs);
              setGauge(
                `stream.preview_resolution.${cameraName}`,
                `${pending.previewWidth}x${pending.previewHeight}`,
              );
              setGauge(
                `stream.preview_pixels.${cameraName}`,
                pending.previewWidth * pending.previewHeight,
              );
              setGauge(`stream.jpeg_bytes.${cameraName}`, pending.jpegData.length);
              setGauge(
                `stream.decoded_resolution.${cameraName}`,
                `${preparedRaster.width}x${preparedRaster.height}`,
              );
              setGauge(
                `stream.output_resolution.${cameraName}`,
                `${renderResult.stats.outputColumns}x${renderResult.stats.outputRows}`,
              );
              setGauge(`stream.ansi_cells.${cameraName}`, renderResult.stats.cellCount);
              setGauge(
                `stream.ansi_sgr_changes.${cameraName}`,
                renderResult.stats.sgrChangeCount ?? 0,
              );
              setGauge(
                `stream.ansi_sgr_per_cell.${cameraName}`,
                renderResult.stats.cellCount > 0
                  ? (renderResult.stats.sgrChangeCount ?? 0) /
                      renderResult.stats.cellCount
                  : 0,
              );
              setGauge(
                `stream.render_output_bytes.${cameraName}`,
                renderResult.stats.outputBytes,
              );
              setGauge(
                `stream.render_cache_hits.${cameraName}`,
                renderResult.stats.cacheHits ?? 0,
              );
              setGauge(
                `stream.render_cache_misses.${cameraName}`,
                renderResult.stats.cacheMisses ?? 0,
              );
              setGauge(
                `stream.render_sample_count.${cameraName}`,
                renderResult.stats.sampleCount ?? 0,
              );
              presentedFramesRef.current.set(cameraName, {
                lines: renderResult.lines,
                frameKey: pending.key,
                sourceTimestampNs: pending.sourceTimestampNs,
                sourceFrameIndex: pending.sourceFrameIndex,
                decodedWidth: preparedRaster.width,
                decodedHeight: preparedRaster.height,
                outputBytes: renderResult.stats.outputBytes,
              });
              presentedFramesDirtyRef.current = true;
            } catch {
              if (!isMountedRef.current) {
                return;
              }
            }

            pending = pendingDecodeRef.current.get(cameraName);
          }
        } finally {
          activeDecodeRef.current.delete(cameraName);
          updateDecodeGauges();
          if (isMountedRef.current && pendingDecodeRef.current.has(cameraName)) {
            pumpDecode(cameraName);
          }
        }
      })();
    };

    for (const camera of cameras) {
      const frame = camera.frame;
      if (!frame?.jpegData || frame.jpegData.length === 0) {
        requestedDecodeKeyRef.current.delete(camera.name);
        pendingDecodeRef.current.delete(camera.name);
        clearPresentedFrame(camera.name);
        updateDecodeGauges();
        continue;
      }

      const decodeKey = [
        frame.sequence,
        frame.previewWidth,
        frame.previewHeight,
        rendererRaster.width,
        rendererRaster.height,
        renderLayout.columns,
        renderLayout.rows,
      ].join(":");
      if (requestedDecodeKeyRef.current.get(camera.name) === decodeKey) {
        continue;
      }

      requestedDecodeKeyRef.current.set(camera.name, decodeKey);
      pendingDecodeRef.current.set(camera.name, {
        key: decodeKey,
        jpegData: frame.jpegData,
        sourceTimestampNs: frame.timestampNs,
        sourceFrameIndex: frame.frameIndex,
        previewWidth: frame.previewWidth,
        previewHeight: frame.previewHeight,
      });
      updateDecodeGauges();
      pumpDecode(camera.name);
    }

    for (const cameraName of Array.from(requestedDecodeKeyRef.current.keys())) {
      if (activeNames.has(cameraName)) {
        continue;
      }
      requestedDecodeKeyRef.current.delete(cameraName);
      pendingDecodeRef.current.delete(cameraName);
      clearPresentedFrame(cameraName);
      updateDecodeGauges();
    }
  }, [
    asciiRendererBackend,
    cameras,
    contentCharHeight,
    perCameraDecodeFps,
    perCameraDecodeIntervalMs,
    renderLayout,
    rendererRaster,
  ]);

  useEffect(() => {
    setGauge("stream.rendered_cameras", numCams);
    setGauge("stream.decoded_frames", presentedFrames.size);
    setGauge("stream.target_visible_cells", numCams * perCamWidth * contentCharHeight);
  }, [contentCharHeight, numCams, perCamWidth, presentedFrames.size]);

  const outputResult = useMemo(() => {
    const composeStartMs = nowMs();
    const result: string[] = [];
    const topRight = hasRightPanel ? "┬" : "┐";
    const midRight = hasRightPanel ? "│" : "│";
    const botRight = hasRightPanel ? "┴" : "┘";

    let topLine = "┌";
    for (let index = 0; index < numCams; index++) {
      const name = cameras[index]?.name ?? `camera_${index}`;
      const label = `─ ${name} `;
      const remaining = Math.max(0, perCamWidth - label.length);
      topLine += label + "─".repeat(remaining);
      topLine += index < numCams - 1 ? "┬" : topRight;
    }
    result.push(topLine);

    for (let row = 0; row < contentCharHeight; row++) {
      let line = "│";
      for (let index = 0; index < numCams; index++) {
        const frame = presentedFrames.get(cameras[index]?.name ?? "");
        if (frame && row < frame.lines.length) {
          line += frame.lines[row] + RESET;
        } else if (row === Math.floor(contentCharHeight / 2)) {
          const message = "╌ No signal ╌";
          const pad = Math.max(0, perCamWidth - message.length);
          const left = Math.floor(pad / 2);
          const right = pad - left;
          line += " ".repeat(left) + message + " ".repeat(right);
        } else {
          line += " ".repeat(perCamWidth);
        }
        line += index < numCams - 1 ? "│" : midRight;
      }
      result.push(line);
    }

    let bottomLine = "└";
    for (let index = 0; index < numCams; index++) {
      bottomLine += "─".repeat(perCamWidth);
      bottomLine += index < numCams - 1 ? "┴" : botRight;
    }
    result.push(bottomLine);

    return {
      lines: result,
      composeDurationMs: nowMs() - composeStartMs,
    };
  }, [
    cameras,
    contentCharHeight,
    hasRightPanel,
    numCams,
    perCamWidth,
    presentedFrames,
  ]);

  useEffect(() => {
    recordTiming("stream.compose", outputResult.composeDurationMs);
  }, [outputResult.composeDurationMs]);

  const finalOutputResult = useMemo(() => {
    const finalizeStartMs = nowMs();
    const finalLines =
      !infoPanelLines || infoPanelLines.length === 0
        ? outputResult.lines.map((line) => line + RESET)
        : outputResult.lines.map((line, index) => {
            const infoLine = index < infoPanelLines.length ? infoPanelLines[index] : "";
            return line + infoLine + RESET;
          });
    const finalText = finalLines.join("\n");
    const textOutputBytes = Buffer.byteLength(finalText, "utf8");
    return {
      finalLines,
      finalText,
      finalizeDurationMs: nowMs() - finalizeStartMs,
      presentationBytes: textOutputBytes,
      textOutputBytes,
    };
  }, [infoPanelLines, outputResult.lines]);

  useEffect(() => {
    recordTiming("stream.finalize", finalOutputResult.finalizeDurationMs);
    setGauge("stream.output_frame_rows", finalOutputResult.finalLines.length);
    setGauge("stream.output_bytes", finalOutputResult.presentationBytes);
    setGauge("stream.text_output_bytes", finalOutputResult.textOutputBytes);
  }, [
    finalOutputResult.finalLines.length,
    finalOutputResult.finalizeDurationMs,
    finalOutputResult.presentationBytes,
    finalOutputResult.textOutputBytes,
  ]);

  return (
    <Box flexDirection="column">
      <Text wrap="end">{finalOutputResult.finalText}</Text>
    </Box>
  );
}

async function prepareRendererRaster(
  jpegData: Buffer,
  targetWidth: number,
  targetHeight: number,
): Promise<PreparedRaster> {
  const decoded = await sharp(jpegData, {
    sequentialRead: true,
  })
    .raw()
    .toBuffer({ resolveWithObject: true });

  if (
    decoded.info.channels === 3 &&
    decoded.info.width === targetWidth &&
    decoded.info.height === targetHeight
  ) {
    return {
      data: decoded.data,
      width: decoded.info.width,
      height: decoded.info.height,
    };
  }

  if (
    decoded.info.channels === 3 &&
    decoded.info.width <= targetWidth &&
    decoded.info.height <= targetHeight
  ) {
    return centerPadRgbRaster(
      decoded.data,
      decoded.info.width,
      decoded.info.height,
      targetWidth,
      targetHeight,
    );
  }

  return await resizePreparedRaster(
    decoded.data,
    decoded.info.width,
    decoded.info.height,
    decoded.info.channels as 1 | 2 | 3 | 4,
    targetWidth,
    targetHeight,
  );
}

function centerPadRgbRaster(
  data: Buffer,
  sourceWidth: number,
  sourceHeight: number,
  targetWidth: number,
  targetHeight: number,
): PreparedRaster {
  const output = Buffer.alloc(targetWidth * targetHeight * 3, 0);
  const offsetX = Math.floor((targetWidth - sourceWidth) / 2);
  const offsetY = Math.floor((targetHeight - sourceHeight) / 2);
  const sourceStride = sourceWidth * 3;

  for (let row = 0; row < sourceHeight; row++) {
    const sourceStart = row * sourceStride;
    const targetStart = ((offsetY + row) * targetWidth + offsetX) * 3;
    data.copy(output, targetStart, sourceStart, sourceStart + sourceStride);
  }

  return {
    data: output,
    width: targetWidth,
    height: targetHeight,
  };
}

async function resizePreparedRaster(
  data: Buffer,
  sourceWidth: number,
  sourceHeight: number,
  channels: 1 | 2 | 3 | 4,
  targetWidth: number,
  targetHeight: number,
): Promise<PreparedRaster> {
  const resized = await sharp(data, {
    raw: {
      width: sourceWidth,
      height: sourceHeight,
      channels,
    },
  })
    .resize(targetWidth, targetHeight, {
      fit: "contain",
      position: "centre",
      background: BLACK_BACKGROUND,
      kernel: sharp.kernel.nearest,
    })
    .raw()
    .toBuffer({ resolveWithObject: true });

  return {
    data: resized.data,
    width: resized.info.width,
    height: resized.info.height,
  };
}
