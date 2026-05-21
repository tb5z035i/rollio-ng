import type { DebugSnapshot } from "../lib/debug-metrics";
import type { StreamInfoMessage } from "../lib/protocol";

interface DebugPanelProps {
  snapshot: DebugSnapshot;
  streamInfo: StreamInfoMessage | null;
}

export function DebugPanel({ snapshot, streamInfo }: DebugPanelProps) {
  const cameraNames = getCameraNames(snapshot, streamInfo);
  const previewMode = streamInfo?.preview_output_mode ?? null;
  const bytesGaugePrefix =
    previewMode === "encoded" ? "ws.encoded_payload_bytes" : "ws.jpeg_bytes";
  const lines = [
    `Layout: ${gaugeValue(snapshot, "ui.layout")} | Cameras: ${gaugeValue(snapshot, "ui.camera_count")} | Robots: ${gaugeValue(snapshot, "ui.robot_count")}`,
    `WS: ${gaugeValue(snapshot, "ws.connected")} | Stream info: ${gaugeValue(snapshot, "ws.stream_info_status")} | Episode: ${gaugeValue(snapshot, "ws.episode_status")}`,
    `Preview path: ${formatPreviewPath(streamInfo)} | target: ${gaugeValue(snapshot, "ui.preview_target")} | Active: ${gaugeValue(snapshot, "ui.preview_active")} | Active WS: ${gaugeValue(snapshot, "ws.active_preview_size")}`,
    `Frames: rx=${gaugeValue(snapshot, "ws.frames_received_total")} | robots=${gaugeValue(snapshot, "ws.robot_messages_total")} | flush=${formatTiming(snapshot, "ws.flush")}`,
    `Receive latency: ${formatTiming(snapshot, "ws.frame_latency.receive")} | Parse: bin=${formatTiming(snapshot, "ws.parse.binary")} json=${formatTiming(snapshot, "ws.parse.json")}`,
    `App render: ${formatTiming(snapshot, "app.render")} | Camera commit: ${formatTiming(snapshot, "ui.camera_commit")}`,
    formatPerCameraLine("Received fps", cameraNames, (cameraName) =>
      streamInfo?.cameras?.find((camera) => camera.name === cameraName)?.received_fps_estimate,
    ),
    formatPerCameraLine("Bytes", cameraNames, (cameraName) =>
      numericGaugeValue(snapshot, `${bytesGaugePrefix}.${cameraName}`, Number.NaN),
    ),
    formatPerCameraLine("Display ms", cameraNames, (cameraName) =>
      numericGaugeValue(snapshot, `ui.display_latency_ms.${cameraName}`, Number.NaN),
    ),
    ...(previewMode === "encoded"
      ? [
          formatPerCameraStringLine("Packets rx", cameraNames, (cameraName) =>
            gaugeStr(snapshot, `ws.encoded_packets_total.${cameraName}`),
          ),
          formatPerCameraStringLine(
            "Keyframes rx",
            cameraNames,
            (cameraName) =>
              gaugeStr(snapshot, `ws.encoded_keyframes_total.${cameraName}`),
          ),
          formatPerCameraStringLine(
            "Decoder",
            cameraNames,
            (cameraName) =>
              `${gaugeStr(snapshot, `ui.preview_decoder_state.${cameraName}`)}/${gaugeStr(snapshot, `ui.preview_decoder_codec_string.${cameraName}`)}@${gaugeStr(snapshot, `ws.encoded_codec_dims.${cameraName}`)}`,
          ),
          formatPerCameraStringLine(
            "Decoder errors",
            cameraNames,
            (cameraName) =>
              `cfg=${gaugeStr(snapshot, `ui.preview_decoder_configure_failures_total.${cameraName}`)} dec=${gaugeStr(snapshot, `ui.preview_decoder_decode_failures_total.${cameraName}`)} rt=${gaugeStr(snapshot, `ui.preview_decoder_errors_total.${cameraName}`)}`,
          ),
          formatPerCameraStringLine(
            "Frames presented",
            cameraNames,
            (cameraName) =>
              gaugeStr(snapshot, `ui.frames_presented_total.${cameraName}`),
          ),
        ]
      : []),
  ];

  return (
    <section className="panel">
      <header className="panel__header">Debug (press d to hide)</header>
      <div className="debug-panel">
        {lines.map((line, index) => (
          <div className="debug-panel__line" key={index}>
            {line}
          </div>
        ))}
      </div>
    </section>
  );
}

function formatTiming(snapshot: DebugSnapshot, name: string): string {
  const metric = snapshot.timings[name];
  if (!metric) {
    return "n/a";
  }
  return `${formatMs(metric.lastMs)} last | ${formatMs(metric.avgMs)} avg | ${formatMs(metric.maxMs)} max`;
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

function formatMs(value: number): string {
  if (value >= 100) {
    return `${value.toFixed(0)}ms`;
  }
  if (value >= 10) {
    return `${value.toFixed(1)}ms`;
  }
  return `${value.toFixed(2)}ms`;
}

function formatPerCameraLine(
  label: string,
  cameraNames: string[],
  getter: (cameraName: string) => number | null | undefined,
): string {
  if (cameraNames.length === 0) {
    return `${label}: n/a`;
  }
  return `${label}: ${cameraNames
    .map((cameraName) => `${cameraName}=${formatNumber(getter(cameraName))}`)
    .join(", ")}`;
}

function formatPerCameraStringLine(
  label: string,
  cameraNames: string[],
  getter: (cameraName: string) => string,
): string {
  if (cameraNames.length === 0) {
    return `${label}: n/a`;
  }
  return `${label}: ${cameraNames
    .map((cameraName) => `${cameraName}=${getter(cameraName)}`)
    .join(", ")}`;
}

function gaugeStr(snapshot: DebugSnapshot, name: string): string {
  const gauge = snapshot.gauges[name];
  if (!gauge) return "n/a";
  return String(gauge.value);
}

function formatNumber(value: number | null | undefined): string {
  if (!Number.isFinite(value)) {
    return "n/a";
  }
  const numericValue = value as number;
  if (numericValue >= 1024) {
    return `${(numericValue / 1024).toFixed(1)}KiB`;
  }
  return numericValue >= 10
    ? numericValue.toFixed(1)
    : numericValue.toFixed(2);
}

/// Map the visualizer's `preview_output_mode` to the operator-facing
/// codec label that drives this preview path. JPEG mode is bytes from
/// the encoder's `JpegCompressor`; encoded mode is H.264 access units
/// for color cameras (depth uses RVL but is not surfaced here).
function formatPreviewPath(streamInfo: StreamInfoMessage | null): string {
  if (!streamInfo) {
    return "n/a";
  }
  switch (streamInfo.preview_output_mode) {
    case "jpeg":
      return "JPEG";
    case "encoded":
      return "H264";
    default:
      return "n/a";
  }
}

function getCameraNames(
  snapshot: DebugSnapshot,
  streamInfo: StreamInfoMessage | null,
): string[] {
  if (streamInfo?.cameras && streamInfo.cameras.length > 0) {
    return streamInfo.cameras.map((camera) => camera.name);
  }

  return Object.keys(snapshot.gauges)
    .filter((name) => name.startsWith("ws.frames_received_total."))
    .map((name) => name.substring("ws.frames_received_total.".length))
    .sort();
}
