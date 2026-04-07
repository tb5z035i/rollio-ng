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
  type AsciiPixelFormat,
  type AsciiCellGeometry,
  type AsciiRenderLayout,
  type AsciiRendererId,
} from "../lib/renderers/index.js";

const RESET = "\x1b[0m";
const DECODE_COMMIT_INTERVAL_MS = 16;
const SHARP_DECODE_CONCURRENCY = 6;
const TARGET_TOTAL_DECODE_FPS = 360;
const MAX_DECODE_FPS_PER_CAMERA = 60;
const MIN_DECODE_FPS_PER_CAMERA = 30;
const BLACK_BACKGROUND = { r: 0, g: 0, b: 0, alpha: 1 };

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
  cellGeometry: AsciiCellGeometry;
  rendererId: AsciiRendererId;
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
  cellGeometry: AsciiCellGeometry,
  rendererId: AsciiRendererId,
): CameraPreviewRaster {
  const grid = describeCameraCellGrid(totalWidth, panelHeight, numCameras);
  const raster = createAsciiRendererBackend(rendererId, {
    cellGeometry,
  }).describeRaster({
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
  cellGeometry,
  rendererId,
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
    () => createAsciiRendererBackend(rendererId, { cellGeometry }),
    [cellGeometry, rendererId],
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
    setGauge("stream.renderer_pixel_format", asciiRendererBackend.pixelFormat);
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
      void asciiRendererBackend.dispose?.().catch(() => undefined);
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
    void asciiRendererBackend.prepare?.().catch(() => undefined);
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
                asciiRendererBackend.pixelFormat,
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

              setGauge("stream.renderer_kind", asciiRendererBackend.kind);
              setGauge("stream.renderer_algorithm", asciiRendererBackend.algorithm);
              const useMainThreadRenderQueue = asciiRendererBackend.kind !== "worker";
              const renderQueueWaitStartMs = nowMs();
              const releaseRenderTurn = useMainThreadRenderQueue
                ? await waitForRenderTurn(cameraName)
                : () => undefined;
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
              if (requestedDecodeKeyRef.current.get(cameraName) !== pending.key) {
                incrementGauge("stream.render_stale_drops");
                incrementGauge(`stream.render_stale_drops.${cameraName}`);
              }
              setGauge(`stream.render_error.${cameraName}`, "None");
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
            } catch (error) {
              incrementGauge("stream.render_errors");
              incrementGauge(`stream.render_errors.${cameraName}`);
              setGauge(
                `stream.render_error.${cameraName}`,
                error instanceof Error ? error.message : String(error),
              );
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

export async function prepareRendererRaster(
  jpegData: Buffer,
  targetWidth: number,
  targetHeight: number,
  pixelFormat: AsciiPixelFormat = "rgb24",
): Promise<PreparedRaster> {
  if (pixelFormat === "luma8") {
    return await prepareGrayscaleRaster(jpegData, targetWidth, targetHeight);
  }

  const decoded = await sharp(jpegData, {
    sequentialRead: true,
  })
    .raw()
    .toBuffer({ resolveWithObject: true });

  const normalized = normalizeDecodedRasterToRgb(
    decoded.data,
    decoded.info.width,
    decoded.info.height,
    decoded.info.channels as 1 | 2 | 3 | 4,
  );

  if (
    normalized.width === targetWidth &&
    normalized.height === targetHeight
  ) {
    return {
      data: normalized.data,
      width: normalized.width,
      height: normalized.height,
    };
  }

  return await resizePreparedRaster(
    normalized.data,
    normalized.width,
    normalized.height,
    targetWidth,
    targetHeight,
    3,
  );
}

async function prepareGrayscaleRaster(
  jpegData: Buffer,
  targetWidth: number,
  targetHeight: number,
): Promise<PreparedRaster> {
  const decoded = await sharp(jpegData, {
    sequentialRead: true,
  })
    .raw()
    .toBuffer({ resolveWithObject: true });

  const normalized = normalizeDecodedRasterToLuma(
    decoded.data,
    decoded.info.width,
    decoded.info.height,
    decoded.info.channels as 1 | 2 | 3 | 4,
  );

  if (
    normalized.width === targetWidth &&
    normalized.height === targetHeight
  ) {
    return {
      data: normalized.data,
      width: normalized.width,
      height: normalized.height,
    };
  }

  return await resizePreparedRaster(
    normalized.data,
    normalized.width,
    normalized.height,
    targetWidth,
    targetHeight,
    1,
  );
}

function normalizeDecodedRasterToRgb(
  data: Buffer,
  sourceWidth: number,
  sourceHeight: number,
  channels: 1 | 2 | 3 | 4,
): PreparedRaster {
  if (channels === 3) {
    return {
      data,
      width: sourceWidth,
      height: sourceHeight,
    };
  }

  const pixelCount = sourceWidth * sourceHeight;
  const output = Buffer.alloc(pixelCount * 3);

  for (let idx = 0; idx < pixelCount; idx++) {
    const sourceOffset = idx * channels;
    const targetOffset = idx * 3;

    if (channels === 1 || channels === 2) {
      const value = channels === 2
        ? Math.round((data[sourceOffset] * data[sourceOffset + 1]) / 255)
        : data[sourceOffset];
      output[targetOffset] = value;
      output[targetOffset + 1] = value;
      output[targetOffset + 2] = value;
      continue;
    }

    output[targetOffset] = data[sourceOffset];
    output[targetOffset + 1] = data[sourceOffset + 1];
    output[targetOffset + 2] = data[sourceOffset + 2];
  }

  return {
    data: output,
    width: sourceWidth,
    height: sourceHeight,
  };
}

function normalizeDecodedRasterToLuma(
  data: Buffer,
  sourceWidth: number,
  sourceHeight: number,
  channels: 1 | 2 | 3 | 4,
): PreparedRaster {
  if (channels === 1) {
    return {
      data,
      width: sourceWidth,
      height: sourceHeight,
    };
  }

  const pixelCount = sourceWidth * sourceHeight;
  const output = Buffer.alloc(pixelCount);

  for (let idx = 0; idx < pixelCount; idx++) {
    const sourceOffset = idx * channels;
    if (channels === 2) {
      output[idx] = Math.round((data[sourceOffset] * data[sourceOffset + 1]) / 255);
      continue;
    }

    const r = data[sourceOffset];
    const g = data[sourceOffset + 1];
    const b = data[sourceOffset + 2];
    let luma = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    if (channels === 4) {
      luma = (luma * data[sourceOffset + 3]) / 255;
    }
    output[idx] = Math.round(luma);
  }

  return {
    data: output,
    width: sourceWidth,
    height: sourceHeight,
  };
}

async function resizePreparedRaster(
  data: Buffer,
  sourceWidth: number,
  sourceHeight: number,
  targetWidth: number,
  targetHeight: number,
  channels: 1 | 3,
): Promise<PreparedRaster> {
  let pipeline = sharp(data, {
    raw: {
      width: sourceWidth,
      height: sourceHeight,
      channels,
    },
  })
    .resize(targetWidth, targetHeight, {
      fit: "cover",
      position: "centre",
      background: BLACK_BACKGROUND,
      kernel: sharp.kernel.nearest,
    });
  if (channels === 1) {
    pipeline = pipeline.extractChannel(0);
  }
  const resized = await pipeline.raw().toBuffer({ resolveWithObject: true });

  return {
    data: resized.data,
    width: resized.info.width,
    height: resized.info.height,
  };
}
