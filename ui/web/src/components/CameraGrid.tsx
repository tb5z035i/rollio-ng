import { useEffect, useRef } from "react";
import { MAX_PREVIEW_CAMERAS } from "../lib/camera-layout";
import { incrementGauge, nowMs, recordTiming, setGauge } from "../lib/debug-metrics";
import type { PreviewDimensions } from "../lib/layout";
import { codecName } from "../lib/protocol";
import type { CameraFrame } from "../lib/websocket";

interface CameraGridProps {
  cameras: Array<{
    name: string;
    frame: CameraFrame | undefined;
    /**
     * True when the active encoder reports SCALING_LOCKED on its
     * stream_info — the preview cannot be rescaled (passthrough
     * mode). We render a small lock badge over the tile so users
     * understand why resize gestures aren't taking effect.
     */
    scalingLocked?: boolean;
    previewIssue?: string;
    previewIssueTitle?: string;
  }>;
  onPreviewSizeChange?: (size: PreviewDimensions) => void;
}

function metaLine(frame: CameraFrame): string {
  if (frame.kind === "jpeg") {
    return `${frame.previewWidth}x${frame.previewHeight} | jpeg ${frame.jpegBytes} bytes`;
  }
  return `${frame.width}x${frame.height} | ${codecName(frame.codecId)} ${frame.payloadBytes} bytes`;
}

interface VideoCanvasTileProps {
  name: string;
  frame: Extract<CameraFrame, { kind: "video" }>;
}

function VideoCanvasTile({ name, frame }: VideoCanvasTileProps) {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) {
      return;
    }
    if (canvas.width !== frame.width) {
      canvas.width = frame.width;
    }
    if (canvas.height !== frame.height) {
      canvas.height = frame.height;
    }
    const ctx = canvas.getContext("2d");
    if (!ctx) {
      return;
    }
    try {
      // The websocket layer owns `frame.videoFrame.close()`; we read
      // it synchronously here and never retain a reference past this
      // useEffect.
      ctx.drawImage(frame.videoFrame, 0, 0, frame.width, frame.height);
    } catch (error) {
      console.warn(
        `[camera-grid] drawImage failed for ${name}: ${String(error)}`,
      );
    }
  }, [frame.sequence, frame.width, frame.height, frame.videoFrame, name]);

  return (
    <canvas
      ref={canvasRef}
      className="camera-tile__canvas"
      width={frame.width}
      height={frame.height}
      data-testid={`camera-canvas-${name}`}
      aria-label={`${name} preview`}
    />
  );
}

export function CameraGrid({
  cameras,
  onPreviewSizeChange,
}: CameraGridProps) {
  const mediaMeasureRef = useRef<HTMLDivElement | null>(null);
  const lastPresentedSequenceRef = useRef<Map<string, number>>(new Map());

  useEffect(() => {
    const measure = () => {
      const element = mediaMeasureRef.current;
      if (!element || !onPreviewSizeChange) {
        return;
      }

      onPreviewSizeChange({
        width: element.clientWidth,
        height: element.clientHeight,
      });
    };

    measure();
    if (typeof ResizeObserver === "undefined") {
      return;
    }

    const element = mediaMeasureRef.current;
    if (!element) {
      return;
    }

    const observer = new ResizeObserver(() => {
      measure();
    });
    observer.observe(element);
    return () => {
      observer.disconnect();
    };
  }, [cameras.length, onPreviewSizeChange]);

  useEffect(() => {
    const commitStartMs = nowMs();
    for (const camera of cameras) {
      const frame = camera.frame;
      if (!frame) {
        continue;
      }

      const lastSequence = lastPresentedSequenceRef.current.get(camera.name);
      if (lastSequence === frame.sequence) {
        continue;
      }

      lastPresentedSequenceRef.current.set(camera.name, frame.sequence);
      incrementGauge("ui.frames_presented_total");
      incrementGauge(`ui.frames_presented_total.${camera.name}`);
      if (frame.kind === "jpeg") {
        setGauge(
          `ui.display_latency_ms.${camera.name}`,
          Math.max(0, Date.now() - frame.timestampNs / 1_000_000),
        );
        setGauge(
          `ui.preview_resolution.${camera.name}`,
          `${frame.previewWidth}x${frame.previewHeight}`,
        );
        setGauge(`ui.jpeg_bytes.${camera.name}`, frame.jpegBytes);
        setGauge(`ui.frame_index.${camera.name}`, frame.frameIndex);
      } else {
        // `timestampUs` is the codec's PTS (monotonic-from-recording),
        // not unix epoch — comparing it to Date.now() gives a ~1.7e9-ms
        // garbage value. `sourceTimestampUs` carries the camera's
        // wall-clock capture time, which is the right side of the
        // capture-to-display latency calculation.
        setGauge(
          `ui.display_latency_ms.${camera.name}`,
          Math.max(0, Date.now() - frame.sourceTimestampUs / 1_000),
        );
        setGauge(
          `ui.preview_resolution.${camera.name}`,
          `${frame.width}x${frame.height}`,
        );
        setGauge(
          `ui.encoded_payload_bytes.${camera.name}`,
          frame.payloadBytes,
        );
      }
    }
    setGauge("ui.camera_count", cameras.length);
    recordTiming("ui.camera_commit", nowMs() - commitStartMs);
  }, [cameras]);

  // Tiles wrap onto additional rows once a row already holds
  // `MAX_PREVIEW_CAMERAS` of them, so each tile keeps a healthy 16:10-ish
  // box even when the project ships with more cameras than fit on one
  // row (e.g. realsense color + depth + 2 wrist cams = 4, with cap=3,
  // produces 2 rows: [3 tiles, 1 tile]). Earlier behaviour silently
  // hid every tile past the cap; the operator was left wondering why a
  // configured stream looked offline.
  const columnCount = Math.max(1, Math.min(cameras.length, MAX_PREVIEW_CAMERAS));
  return (
    <div
      className="camera-grid"
      style={{ gridTemplateColumns: `repeat(${columnCount}, minmax(0, 1fr))` }}
    >
      {cameras.map((camera, index) => (
        <section className="panel camera-tile" key={camera.name}>
          <header className="panel__header">{camera.name}</header>
          <div
            className="camera-tile__media"
            ref={index === 0 ? mediaMeasureRef : undefined}
          >
            {camera.frame ? (
              camera.frame.kind === "jpeg" ? (
                <img
                  alt={`${camera.name} preview`}
                  className="camera-tile__image"
                  src={camera.frame.objectUrl}
                />
              ) : (
                <VideoCanvasTile name={camera.name} frame={camera.frame} />
              )
            ) : (
              <div
                className="camera-tile__placeholder"
                data-testid={`camera-placeholder-${camera.name}`}
                title={camera.previewIssueTitle}
              >
                {camera.previewIssue ?? "No signal"}
              </div>
            )}
            {camera.scalingLocked ? (
              <div
                className="camera-tile__lock"
                title={
                  "Encoder is in passthrough mode: output dims are pinned " +
                  "to source dims, so the preview size can't be changed " +
                  "from the UI."
                }
                data-testid={`camera-lock-${camera.name}`}
                role="img"
                aria-label={`${camera.name} preview size is locked to source dimensions`}
              >
                <span aria-hidden="true" className="camera-tile__lock-glyph">
                  &#x1F512;
                </span>
                <span className="camera-tile__lock-text">locked</span>
              </div>
            ) : null}
          </div>
          <div className="camera-tile__meta">
            {camera.frame ? metaLine(camera.frame) : "Waiting for frames"}
          </div>
        </section>
      ))}
    </div>
  );
}
