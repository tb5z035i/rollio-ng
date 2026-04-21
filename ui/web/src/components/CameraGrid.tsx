import { useEffect, useRef } from "react";
import { MAX_PREVIEW_CAMERAS } from "../lib/camera-layout";
import { incrementGauge, nowMs, recordTiming, setGauge } from "../lib/debug-metrics";
import type { PreviewDimensions } from "../lib/layout";
import type { CameraFrame } from "../lib/websocket";

interface CameraGridProps {
  cameras: Array<{ name: string; frame: CameraFrame | undefined }>;
  onPreviewSizeChange?: (size: PreviewDimensions) => void;
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
              <img
                alt={`${camera.name} preview`}
                className="camera-tile__image"
                src={camera.frame.objectUrl}
              />
            ) : (
              <div className="camera-tile__placeholder">No signal</div>
            )}
          </div>
          <div className="camera-tile__meta">
            {camera.frame
              ? `${camera.frame.previewWidth}x${camera.frame.previewHeight} | ${camera.frame.jpegBytes} bytes`
              : "Waiting for frames"}
          </div>
        </section>
      ))}
    </div>
  );
}
