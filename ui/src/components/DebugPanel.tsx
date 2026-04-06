import React, { useEffect, useMemo, useRef, useState } from "react";
import { Box, Text } from "ink";
import type { DebugSnapshot, TimingMetricSnapshot } from "../lib/debug-metrics.js";
import type { StreamInfoMessage } from "../lib/protocol.js";

const DEBUG_PANEL_CONTENT_LINES = 17;
export const DEBUG_PANEL_HEIGHT = DEBUG_PANEL_CONTENT_LINES + 2;
const FPS_WINDOW_MS = 1000;
const FPS_LAG_TOLERANCE = 0.25;

interface FpsSample {
  capturedAtMs: number;
  receivedCounts: Record<string, number>;
  presentedCounts: Record<string, number>;
}

interface PipelineFpsMetrics {
  wsReceivedPerCamera: Record<string, number>;
  presentedPerCamera: Record<string, number>;
  laggingSource: boolean;
  laggingWs: boolean;
  laggingRender: boolean;
}

interface DebugPanelProps {
  width: number;
  snapshot: DebugSnapshot;
  streamInfo: StreamInfoMessage | null;
}

export function DebugPanel({ width, snapshot, streamInfo }: DebugPanelProps) {
  const fpsSamplesRef = useRef<FpsSample[]>([]);
  const [pipelineFpsMetrics, setPipelineFpsMetrics] = useState<PipelineFpsMetrics>({
    wsReceivedPerCamera: {},
    presentedPerCamera: {},
    laggingSource: false,
    laggingWs: false,
    laggingRender: false,
  });
  const innerWidth = Math.max(0, width - 2);
  const headerText = "─ Debug (press d to hide) ";
  const headerPad = Math.max(0, width - headerText.length - 2);
  const topBorder = `┌${headerText}${"─".repeat(headerPad)}┐`;
  const bottomBorder = `└${"─".repeat(Math.max(0, width - 2))}┘`;
  const cameraNames = useMemo(
    () => getCameraNames(snapshot, streamInfo),
    [snapshot, streamInfo],
  );
  const rendererBackend = gaugeValue(snapshot, "stream.renderer_backend", "n/a");

  useEffect(() => {
    const receivedCounts: Record<string, number> = {};
    const presentedCounts: Record<string, number> = {};
    for (const cameraName of cameraNames) {
      receivedCounts[cameraName] = numericGaugeValue(
        snapshot,
        `ws.frames_received_total.${cameraName}`,
      );
      presentedCounts[cameraName] = numericGaugeValue(
        snapshot,
        `stream.frames_presented_total.${cameraName}`,
      );
    }

    const nextSamples = fpsSamplesRef.current.filter(
      (sample) => snapshot.capturedAtMs - sample.capturedAtMs <= FPS_WINDOW_MS,
    );
    nextSamples.push({
      capturedAtMs: snapshot.capturedAtMs,
      receivedCounts,
      presentedCounts,
    });
    fpsSamplesRef.current = nextSamples;

    const wsReceivedPerCamera: Record<string, number> = {};
    const presentedPerCamera: Record<string, number> = {};
    if (nextSamples.length >= 2 && cameraNames.length > 0) {
      const firstSample = nextSamples[0];
      const lastSample = nextSamples[nextSamples.length - 1];
      const elapsedMs = lastSample.capturedAtMs - firstSample.capturedAtMs;
      if (elapsedMs > 0) {
        for (const cameraName of cameraNames) {
          wsReceivedPerCamera[cameraName] =
            ((lastSample.receivedCounts[cameraName] -
              firstSample.receivedCounts[cameraName]) *
              1000) /
            elapsedMs;
          presentedPerCamera[cameraName] =
            ((lastSample.presentedCounts[cameraName] -
              firstSample.presentedCounts[cameraName]) *
              1000) /
            elapsedMs;
        }
      }
    }

    const laggingSource = cameraNames.some((cameraName) => {
      const sourceFps = sourceFpsEstimate(streamInfo, cameraName);
      const publishedFps = publishedFpsEstimate(streamInfo, cameraName);
      return (
        sourceFps !== null &&
        publishedFps !== null &&
        publishedFps + FPS_LAG_TOLERANCE < sourceFps
      );
    });
    const laggingWs = cameraNames.some((cameraName) => {
      const receivedFps = wsReceivedPerCamera[cameraName];
      const publishedFps = publishedFpsTarget(streamInfo, cameraName);
      return (
        Number.isFinite(receivedFps) &&
        publishedFps !== null &&
        receivedFps + FPS_LAG_TOLERANCE < publishedFps
      );
    });
    const laggingRender = cameraNames.some((cameraName) => {
      const receivedFps = wsReceivedPerCamera[cameraName];
      const presentedFps = presentedPerCamera[cameraName];
      return (
        Number.isFinite(receivedFps) &&
        Number.isFinite(presentedFps) &&
        presentedFps + FPS_LAG_TOLERANCE < receivedFps
      );
    });

    setPipelineFpsMetrics({
      wsReceivedPerCamera,
      presentedPerCamera,
      laggingSource,
      laggingWs,
      laggingRender,
    });
  }, [cameraNames, snapshot, streamInfo]);

  const laggingSummary = ` Lagging: source=${pipelineFpsMetrics.laggingSource ? "yes" : "no"} | ws=${pipelineFpsMetrics.laggingWs ? "yes" : "no"} | render=${pipelineFpsMetrics.laggingRender ? "yes" : "no"} | latency=${formatTriple(snapshot.timings["stream.latency.displayed"])}`;
  const laggingColor = pipelineFpsMetrics.laggingWs ||
    pipelineFpsMetrics.laggingRender
    ? "red"
    : pipelineFpsMetrics.laggingSource
      ? "yellow"
      : "green";

  const rows = [
    {
      text: padLine(
        laggingSummary,
        innerWidth,
      ),
      color: laggingColor,
      bold: true,
    },
    {
      text: padLine(
        ` Source fps: ${formatCameraMetrics(cameraNames, (cameraName) => sourceFpsEstimate(streamInfo, cameraName), formatFpsValue)}`,
        innerWidth,
      ),
    },
    {
      text: padLine(
        ` Visualizer published: ${formatCameraMetrics(cameraNames, (cameraName) => publishedFpsEstimate(streamInfo, cameraName), formatFpsValue)} | cfg=${formatPreviewConfig(streamInfo)}`,
        innerWidth,
      ),
    },
    {
      text: padLine(
        ` WS received: ${formatCameraMetrics(cameraNames, (cameraName) => pipelineFpsMetrics.wsReceivedPerCamera[cameraName], formatFpsValue)}`,
        innerWidth,
      ),
    },
    {
      text: padLine(
        ` UI presented: ${formatCameraMetrics(cameraNames, (cameraName) => pipelineFpsMetrics.presentedPerCamera[cameraName], formatFpsValue)} | cap=${gaugeValue(snapshot, "stream.decode_fps_cap")}fps`,
        innerWidth,
      ),
    },
    {
      text: padLine(
        ` Renderer: ${rendererBackend} (${gaugeValue(snapshot, "stream.renderer_kind")}/${gaugeValue(snapshot, "stream.renderer_algorithm")}) | raster=${gaugeValue(snapshot, "stream.target_width")}x${gaugeValue(snapshot, "stream.target_height")} | out=${gaugeValue(snapshot, "stream.output_columns")}x${gaugeValue(snapshot, "stream.output_rows")}`,
        innerWidth,
      ),
    },
    {
      text: padLine(
        formatHarriWorkerSummary(rendererBackend, snapshot),
        innerWidth,
      ),
    },
    {
      text: padLine(
        ` Source/JPEG: ${formatCameraSourceStats(cameraNames, snapshot, streamInfo)}`,
        innerWidth,
      ),
    },
    {
      text: padLine(
        ` Hot render: total=${formatHottestTiming(snapshot, cameraNames, "stream.render.total")} | sample=${formatHottestTiming(snapshot, cameraNames, "stream.render.sample")} | lookup=${formatHottestTiming(snapshot, cameraNames, "stream.render.lookup")}`,
        innerWidth,
      ),
    },
    {
      text: padLine(
        ` Resize/queue: resize=${formatHottestTiming(snapshot, cameraNames, "stream.decode.resize")} | queue=${formatLast(snapshot.timings["stream.render.queue_wait"])} | depth=${gaugeValue(snapshot, "stream.render_queue_depth", "0")} | stale=${gaugeValue(snapshot, "stream.render_stale_drops", "0")} | active=${gaugeValue(snapshot, "stream.render_active_camera", "Idle")}`,
        innerWidth,
      ),
    },
    {
      text: padLine(
        ` Target/output: cells=${gaugeValue(snapshot, "stream.target_visible_cells")} | out=${formatBytes(numericGaugeValue(snapshot, "stream.output_bytes", Number.NaN))} | rows=${gaugeValue(snapshot, "stream.output_frame_rows")} | hot/cam=${formatHottestGauge(snapshot, cameraNames, "stream.render_output_bytes", formatBytes)}`,
        innerWidth,
      ),
    },
    {
      text: padLine(
        ` Display/cache: latency=${formatCameraMetrics(cameraNames, (cameraName) => currentDisplayedLatencyMs(snapshot, cameraName), formatLatencyValue)} | sgr=${formatHottestGauge(snapshot, cameraNames, "stream.ansi_sgr_per_cell", formatRatioValue)} | hits=${formatHottestGauge(snapshot, cameraNames, "stream.render_cache_hits", formatCountValue)} | misses=${formatHottestGauge(snapshot, cameraNames, "stream.render_cache_misses", formatCountValue)} | finalize=${formatLast(snapshot.timings["stream.finalize"])}`,
        innerWidth,
      ),
    },
    {
      text: padLine(
        ` Layout: ${gaugeValue(snapshot, "ui.layout")} | WS: ${gaugeValue(snapshot, "ws.connected")} | Stream info: ${gaugeValue(snapshot, "ui.stream_info_available")}`,
        innerWidth,
      ),
    },
    {
      text: padLine(
        ` App render: ${formatTriple(snapshot.timings["app.render"])} | WS flush: ${formatTriple(snapshot.timings["ws.flush"])}`,
        innerWidth,
      ),
    },
    {
      text: padLine(
        ` Decode total: ${formatTriple(snapshot.timings["stream.decode.total"])} | Receive latency: ${formatTriple(snapshot.timings["ws.frame_latency.receive"])}`,
        innerWidth,
      ),
    },
    {
      text: padLine(
        ` Resize: ${formatLast(snapshot.timings["stream.decode.resize"])} | ANSI: ${formatLast(snapshot.timings["stream.decode.ansi"])} | Compose: ${formatLast(snapshot.timings["stream.compose"])}`,
        innerWidth,
      ),
    },
    {
      text: padLine(
        ` Parse bin/json: ${formatLast(snapshot.timings["ws.parse.binary"])} / ${formatLast(snapshot.timings["ws.parse.json"])} | Cameras/robots: ${gaugeValue(snapshot, "ui.camera_count")}/${gaugeValue(snapshot, "ui.robot_count")}`,
        innerWidth,
      ),
    },
    {
      text: padLine(
        ` Queue p/a/d: ${gaugeValue(snapshot, "stream.pending_decodes")}/${gaugeValue(snapshot, "stream.active_decodes")}/${gaugeValue(snapshot, "stream.decoded_frames")} | Presented/rx: ${gaugeValue(snapshot, "stream.frames_presented_total")}/${gaugeValue(snapshot, "ws.frames_received_total")}`,
        innerWidth,
      ),
    },
  ];

  return (
    <Box flexDirection="column" width={width}>
      <Text dimColor>{topBorder}</Text>
      {rows.map((row, index) => (
        <Text key={index} color={row.color} bold={row.bold}>
          {`│${row.text}│`}
        </Text>
      ))}
      <Text dimColor>{bottomBorder}</Text>
    </Box>
  );
}

function formatLast(metric: TimingMetricSnapshot | undefined): string {
  return metric ? formatMs(metric.lastMs) : "n/a";
}

function formatTriple(metric: TimingMetricSnapshot | undefined): string {
  if (!metric) return "n/a";
  return `${formatMs(metric.lastMs)} last | ${formatMs(metric.avgMs)} avg | ${formatMs(metric.maxMs)} max`;
}

function formatMs(value: number): string {
  if (value >= 100) return `${value.toFixed(0)}ms`;
  if (value >= 10) return `${value.toFixed(1)}ms`;
  return `${value.toFixed(2)}ms`;
}

function gaugeValue(
  snapshot: DebugSnapshot,
  name: string,
  fallback = "n/a",
): string {
  const gauge = snapshot.gauges[name];
  return gauge ? String(gauge.value) : fallback;
}

function numericGaugeValue(
  snapshot: DebugSnapshot,
  name: string,
  fallback = 0,
): number {
  const gauge = snapshot.gauges[name];
  return gauge && typeof gauge.value === "number" ? gauge.value : fallback;
}

function formatFps(value: number): string {
  return `${value.toFixed(1)}fps`;
}

function padLine(text: string, width: number): string {
  const trimmed = text.substring(0, width);
  return trimmed + " ".repeat(Math.max(0, width - trimmed.length));
}

function formatFpsValue(value: number | null | undefined): string {
  return Number.isFinite(value) ? formatFps(value as number) : "n/a";
}

function formatLatencyValue(value: number | null | undefined): string {
  return Number.isFinite(value) ? formatMs(value as number) : "n/a";
}

function formatBytes(value: number | null | undefined): string {
  if (!Number.isFinite(value)) return "n/a";
  const numericValue = value as number;
  if (numericValue >= 1024 * 1024) {
    return `${(numericValue / (1024 * 1024)).toFixed(2)}MiB`;
  }
  if (numericValue >= 1024) {
    return `${(numericValue / 1024).toFixed(1)}KiB`;
  }
  return `${numericValue.toFixed(0)}B`;
}

function formatGaugeMs(snapshot: DebugSnapshot, name: string): string {
  const value = numericGaugeValue(snapshot, name, Number.NaN);
  return Number.isFinite(value) ? formatMs(value) : "n/a";
}

function formatHarriWorkerSummary(
  rendererBackend: string,
  snapshot: DebugSnapshot,
): string {
  if (rendererBackend !== "ts-harri") {
    return ` Harri worker: inactive | last=${gaugeValue(snapshot, "stream.harri_worker.state", "n/a")} | log=${gaugeValue(snapshot, "stream.harri_worker.last_log", "n/a")}`;
  }

  return (
    ` Harri worker: mode=${gaugeValue(snapshot, "stream.harri_worker.mode", "n/a")}` +
    ` | state=${gaugeValue(snapshot, "stream.harri_worker.state", "n/a")}` +
    ` | tid=${gaugeValue(snapshot, "stream.harri_worker.thread_id", "n/a")}` +
    ` | pending=${gaugeValue(snapshot, "stream.harri_worker.pending_requests", "0")}` +
    ` | rt=${formatGaugeMs(snapshot, "stream.harri_worker.last_roundtrip_ms")}` +
    ` | log=${gaugeValue(snapshot, "stream.harri_worker.last_log", "n/a")}`
  );
}

function formatRatioValue(value: number | null | undefined): string {
  return Number.isFinite(value) ? `${(value as number).toFixed(2)}` : "n/a";
}

function formatCountValue(value: number | null | undefined): string {
  return Number.isFinite(value) ? `${Math.round(value as number)}` : "n/a";
}

function formatCameraMetrics(
  cameraNames: string[],
  getValue: (cameraName: string) => number | null | undefined,
  formatValue: (value: number | null | undefined) => string,
): string {
  if (cameraNames.length === 0) return "n/a";
  return cameraNames
    .map((cameraName) => `${cameraName}=${formatValue(getValue(cameraName))}`)
    .join(", ");
}

function getCameraNames(
  snapshot: DebugSnapshot,
  streamInfo: StreamInfoMessage | null,
): string[] {
  if (streamInfo && streamInfo.cameras.length > 0) {
    return streamInfo.cameras.map((camera) => camera.name);
  }

  return Object.keys(snapshot.gauges)
    .filter((name) => name.startsWith("stream.frames_presented_total."))
    .map((name) => name.substring("stream.frames_presented_total.".length))
    .sort();
}

function sourceFpsEstimate(
  streamInfo: StreamInfoMessage | null,
  cameraName: string,
): number | null {
  return cameraInfo(streamInfo, cameraName)?.source_fps_estimate ?? null;
}

function publishedFpsEstimate(
  streamInfo: StreamInfoMessage | null,
  cameraName: string,
): number | null {
  const info = cameraInfo(streamInfo, cameraName);
  if (!info) return null;
  return info.published_fps_estimate ?? previewConfigTarget(streamInfo);
}

function publishedFpsTarget(
  streamInfo: StreamInfoMessage | null,
  cameraName: string,
): number | null {
  return publishedFpsEstimate(streamInfo, cameraName);
}

function previewConfigTarget(streamInfo: StreamInfoMessage | null): number | null {
  if (!streamInfo) return null;
  return streamInfo.configured_preview_fps > 0
    ? streamInfo.configured_preview_fps
    : null;
}

function formatPreviewConfig(streamInfo: StreamInfoMessage | null): string {
  if (!streamInfo) return "n/a";
  const preview = streamInfo.configured_preview_fps > 0
    ? `${streamInfo.configured_preview_fps}fps`
    : "unthrottled";
  return (
    `${preview} | cfg=${streamInfo.max_preview_width}x${streamInfo.max_preview_height}` +
    ` | active=${streamInfo.active_preview_width}x${streamInfo.active_preview_height}` +
    ` | q=${streamInfo.jpeg_quality}` +
    ` | workers=${streamInfo.preview_workers}`
  );
}

function cameraInfo(
  streamInfo: StreamInfoMessage | null,
  cameraName: string,
) {
  return streamInfo?.cameras.find((camera) => camera.name === cameraName) ?? null;
}

function currentDisplayedLatencyMs(
  snapshot: DebugSnapshot,
  cameraName: string,
): number | null {
  const sourceTimestampNs = numericGaugeValue(
    snapshot,
    `stream.displayed_source_timestamp_ns.${cameraName}`,
    Number.NaN,
  );
  if (!Number.isFinite(sourceTimestampNs)) return null;
  return Math.max(0, Date.now() - sourceTimestampNs / 1_000_000);
}

function timingForCamera(
  snapshot: DebugSnapshot,
  baseName: string,
  cameraName: string,
): TimingMetricSnapshot | undefined {
  return snapshot.timings[`${baseName}.${cameraName}`];
}

function formatHottestTiming(
  snapshot: DebugSnapshot,
  cameraNames: string[],
  baseName: string,
): string {
  let hottestCamera: string | null = null;
  let hottestMetric: TimingMetricSnapshot | undefined;

  for (const cameraName of cameraNames) {
    const metric = timingForCamera(snapshot, baseName, cameraName);
    if (!metric) continue;
    if (!hottestMetric || metric.lastMs > hottestMetric.lastMs) {
      hottestMetric = metric;
      hottestCamera = cameraName;
    }
  }

  if (!hottestCamera || !hottestMetric) return "n/a";
  return `${hottestCamera} ${formatMs(hottestMetric.lastMs)}`;
}

function formatHottestGauge(
  snapshot: DebugSnapshot,
  cameraNames: string[],
  baseName: string,
  formatValue: (value: number | null | undefined) => string,
): string {
  let hottestCamera: string | null = null;
  let hottestValue = Number.NEGATIVE_INFINITY;

  for (const cameraName of cameraNames) {
    const value = numericGaugeValue(
      snapshot,
      `${baseName}.${cameraName}`,
      Number.NaN,
    );
    if (!Number.isFinite(value)) continue;
    if (value > hottestValue) {
      hottestValue = value;
      hottestCamera = cameraName;
    }
  }

  if (!hottestCamera || !Number.isFinite(hottestValue)) return "n/a";
  return `${hottestCamera} ${formatValue(hottestValue)}`;
}

function formatCameraSourceStats(
  cameraNames: string[],
  snapshot: DebugSnapshot,
  streamInfo: StreamInfoMessage | null,
): string {
  if (cameraNames.length === 0) return "n/a";
  return cameraNames
    .map((cameraName) => {
      const previewResolution = gaugeValue(
        snapshot,
        `stream.preview_resolution.${cameraName}`,
      );
      const camera = cameraInfo(streamInfo, cameraName);
      const sourceResolution =
        camera?.source_width != null && camera.source_height != null
          ? `${camera.source_width}x${camera.source_height}`
          : previewResolution;
      const jpegBytes = numericGaugeValue(
        snapshot,
        `stream.jpeg_bytes.${cameraName}`,
        Number.NaN,
      );
      const resolutionSummary =
        previewResolution !== "n/a" && previewResolution !== sourceResolution
          ? `${sourceResolution}->${previewResolution}`
          : sourceResolution;
      return `${cameraName}=${resolutionSummary}/${formatBytes(jpegBytes)}`;
    })
    .join(", ");
}
