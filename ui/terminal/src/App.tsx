import React, { useState, useEffect, useMemo, useRef } from "react";
import { Box, useInput, useStdin, useStdout } from "ink";
import { useWebSocket } from "./lib/websocket.js";
import { encodeEpisodeCommand, encodeSetPreviewSize } from "./lib/protocol.js";
import { resolveCameraNames } from "./lib/camera-layout.js";
import { getTerminalMetrics } from "./lib/terminal-geometry.js";
import { TitleBar } from "./components/TitleBar.js";
import { StatusBar } from "./components/StatusBar.js";
import {
  CameraRow,
  describeCameraPreviewRaster,
} from "./components/StreamPanel.js";
import {
  getAsciiRendererLabel,
  nextAsciiRendererId,
  type AsciiRendererId,
} from "./lib/renderers/index.js";
import { RobotStatePanel } from "./components/RobotStatePanel.js";
import { InfoPanel } from "./components/InfoPanel.js";
import { DebugPanel, DEBUG_PANEL_HEIGHT } from "./components/DebugPanel.js";
import {
  nowMs,
  recordTiming,
  setGauge,
  snapshotDebugMetrics,
  type DebugSnapshot,
} from "./lib/debug-metrics.js";
import { actionForInput } from "./lib/controls.js";
import type { EpisodeKeyBindings } from "./runtime-config.js";

function useTerminalMetrics() {
  const { stdout } = useStdout();
  const [metrics, setMetrics] = useState(() => getTerminalMetrics(stdout));

  useEffect(() => {
    const onResize = () => {
      setMetrics(getTerminalMetrics(stdout));
    };

    onResize();
    stdout.on("resize", onResize);
    return () => {
      stdout.off("resize", onResize);
    };
  }, [stdout]);

  return metrics;
}

type AppProps = {
  websocketUrl: string;
  initialAsciiRendererId: AsciiRendererId;
  episodeKeyBindings: EpisodeKeyBindings;
};

export function App({
  websocketUrl,
  initialAsciiRendererId,
  episodeKeyBindings,
}: AppProps) {
  const renderStartMs = nowMs();
  const { columns, rows, cellGeometry } = useTerminalMetrics();
  const { isRawModeSupported } = useStdin();
  const supportsInteractiveInput = isRawModeSupported === true;
  const { frames, robotStates, streamInfo, episodeStatus, connected, send } = useWebSocket(
    websocketUrl,
  );
  const [showDebug, setShowDebug] = useState(false);
  const [cameraRendererId, setCameraRendererId] = useState<AsciiRendererId>(
    initialAsciiRendererId,
  );
  const lastPreviewNegotiationKeyRef = useRef<string | null>(null);
  const [debugSnapshot, setDebugSnapshot] = useState<DebugSnapshot>(() =>
    snapshotDebugMetrics(),
  );

  // Derive health status
  const health = connected ? ("normal" as const) : ("degraded" as const);

  useInput(
    (input, key) => {
      if (key.ctrl || key.meta) {
        return;
      }

      const action = actionForInput(input, episodeKeyBindings);
      if (action === "toggle_debug") {
        setShowDebug((prev) => !prev);
      } else if (action === "cycle_renderer") {
        setCameraRendererId((previous) => nextAsciiRendererId(previous));
      } else if (action != null) {
        send(encodeEpisodeCommand(action));
        setGauge("ui.last_episode_command", action);
      }
    },
    { isActive: supportsInteractiveInput },
  );

  // Layout constants
  const isWide = columns >= 120;
  const infoPanelWidth = isWide ? 25 : columns;
  const debugPanelHeight = showDebug ? DEBUG_PANEL_HEIGHT : 0;
  const contentWidth = isWide ? columns - infoPanelWidth : columns;
  const contentHeight = Math.max(1, rows - 2 - debugPanelHeight); // minus title + status bars

  // Camera panel sizing
  const cameraNames = Array.from(frames.keys());
  const configuredCameraNames = streamInfo?.cameras.map((camera) => camera.name) ?? [];
  const camKeys = resolveCameraNames(configuredCameraNames, cameraNames);

  // Allocate vertical space
  const robotPanelHeight = isWide
    ? Math.max(5, Math.floor(contentHeight * 0.3))
    : Math.max(5, Math.min(8, Math.floor(contentHeight * 0.3)));
  const infoPanelHeightH = isWide
    ? 0
    : Math.min(5, Math.max(3, Math.floor(contentHeight * 0.15)));
  const cameraPanelHeight = Math.max(
    5,
    contentHeight - robotPanelHeight - (isWide ? 0 : infoPanelHeightH),
  );
  const cameraPreviewRaster = useMemo(
    () =>
      describeCameraPreviewRaster(
        contentWidth,
        cameraPanelHeight,
        camKeys.length,
        cellGeometry,
        cameraRendererId,
      ),
    [
      camKeys.length,
      cameraRendererId,
      cellGeometry,
      cameraPanelHeight,
      contentWidth,
    ],
  );
  const rendererLabel = getAsciiRendererLabel(cameraRendererId);
  const cameraInfoByName = useMemo(
    () => new Map((streamInfo?.cameras ?? []).map((camera) => [camera.name, camera])),
    [streamInfo],
  );

  // Build camera data for CameraRow
  const cameraData = useMemo(
    () =>
      camKeys.map((name) => ({
        name,
        frame: frames.get(name),
      })),
    [camKeys, frames],
  );

  // Build info panel lines for wide mode (merged into camera row)
  const infoPanelLines = useMemo(() => {
    if (!isWide) return undefined;

    const w = infoPanelWidth;
    const innerW = Math.max(0, w - 1);
    const lines: string[] = [];
    const pad = (s: string) => {
      const trimmed = s.substring(0, innerW);
      return trimmed + " ".repeat(Math.max(0, innerW - trimmed.length));
    };

    // Top border (connects to camera panel's ┬ on the left)
    const headerText = "─ Info ";
    const headerPad = Math.max(0, w - headerText.length - 1);
    lines.push(`${headerText}${"─".repeat(headerPad)}┐`);  // camera ┬ + this = ┬─ Info ──┐

    // Content
    lines.push(pad(" Devices") + "│");
    for (const name of camKeys) {
      const frame = frames.get(name);
      const cameraInfo = cameraInfoByName.get(name);
      const resolution =
        cameraInfo?.source_width != null && cameraInfo.source_height != null
          ? `${cameraInfo.source_width}x${cameraInfo.source_height}`
          : frame
            ? `${frame.previewWidth}x${frame.previewHeight}`
            : "n/a";
      lines.push(pad(`  ${name}  ${resolution}`) + "│");
    }
    for (const [name, state] of robotStates) {
      lines.push(pad(`  ${name}  ${state.num_joints} DoF`) + "│");
    }
    lines.push(pad("") + "│");
    lines.push(pad(` WS: ${connected ? "Connected" : "Disconnected"}`) + "│");

    // Pad remaining rows
    const totalRows = cameraPanelHeight + 2; // +2 for borders
    while (lines.length < totalRows - 1) {
      lines.push(pad("") + "│");
    }
    // Bottom border
    lines.push(`${"─".repeat(w - 1)}┘`);

    return lines;
  }, [
    isWide,
    infoPanelWidth,
    camKeys,
    frames,
    robotStates,
    connected,
    cameraPanelHeight,
    cameraInfoByName,
  ]);

  // Build robot panel data
  const robotEntries = Array.from(robotStates.entries());
  const effectiveEpisodeStatus = episodeStatus ?? {
    type: "episode_status" as const,
    state: "idle" as const,
    episode_count: 0,
    elapsed_ms: 0,
  };

  useEffect(() => {
    if (!connected) {
      lastPreviewNegotiationKeyRef.current = null;
      return;
    }

    const negotiationKey = [
      `${columns}x${rows}`,
      `${cameraPreviewRaster.width}x${cameraPreviewRaster.height}`,
    ].join(":");
    if (lastPreviewNegotiationKeyRef.current === negotiationKey) {
      return;
    }

    const delayMs = lastPreviewNegotiationKeyRef.current === null ? 0 : 75;
    const timer = setTimeout(() => {
      send(encodeSetPreviewSize(cameraPreviewRaster.width, cameraPreviewRaster.height));
      lastPreviewNegotiationKeyRef.current = negotiationKey;
      setGauge(
        "ui.preview_request",
        `${cameraPreviewRaster.width}x${cameraPreviewRaster.height}`,
      );
    }, delayMs);

    return () => {
      clearTimeout(timer);
    };
  }, [
    columns,
    rows,
    cameraPreviewRaster.height,
    cameraPreviewRaster.width,
    connected,
    send,
  ]);

  useEffect(() => {
    setGauge("ui.layout", `${columns}x${rows} ${isWide ? "wide" : "narrow"}`);
    setGauge("ui.camera_count", frames.size);
    setGauge("ui.robot_count", robotStates.size);
    setGauge("ui.debug_enabled", showDebug ? "On" : "Off");
    setGauge("ui.camera_renderer", cameraRendererId);
    setGauge("ui.camera_renderer_label", rendererLabel);
    setGauge("ui.episode_state", effectiveEpisodeStatus.state);
    setGauge("ui.episode_count", effectiveEpisodeStatus.episode_count);
    setGauge("ui.episode_elapsed_ms", effectiveEpisodeStatus.elapsed_ms);
    setGauge(
      "ui.stream_info_available",
      streamInfo ? "Ready" : "Waiting",
    );
    setGauge(
      "ui.preview_target",
      `${cameraPreviewRaster.width}x${cameraPreviewRaster.height}`,
    );
    setGauge(
      "ui.preview_active",
      streamInfo
        ? `${streamInfo.active_preview_width}x${streamInfo.active_preview_height}`
        : "Waiting",
    );
    setGauge(
      "ui.cell_geometry",
      `${cellGeometry.pixelWidth.toFixed(2)}x${cellGeometry.pixelHeight.toFixed(2)}`,
    );
  }, [
    cellGeometry,
    columns,
    rows,
    isWide,
    frames.size,
    robotStates.size,
    showDebug,
    cameraRendererId,
    rendererLabel,
    effectiveEpisodeStatus.elapsed_ms,
    effectiveEpisodeStatus.episode_count,
    effectiveEpisodeStatus.state,
    streamInfo,
    cameraPreviewRaster.height,
    cameraPreviewRaster.width,
  ]);

  useEffect(() => {
    if (!showDebug) return;
    setDebugSnapshot(snapshotDebugMetrics());
    const interval = setInterval(() => {
      setDebugSnapshot(snapshotDebugMetrics());
    }, 250);
    return () => {
      clearInterval(interval);
    };
  }, [showDebug]);

  const renderDurationMs = nowMs() - renderStartMs;

  useEffect(() => {
    recordTiming("app.render", renderDurationMs);
  });

  return (
    <Box flexDirection="column" width={columns} height={rows}>
      {/* Title Bar */}
      <TitleBar mode="Collect" width={columns} />

      {/* Camera row (pre-composed ANSI lines, bypasses Ink width measurement) */}
      <CameraRow
        cameras={cameraData}
        previewRaster={cameraPreviewRaster}
        cellGeometry={cellGeometry}
        rendererId={cameraRendererId}
        infoPanelLines={infoPanelLines}
        hasRightPanel={isWide}
      />

      {/* Robot state panels */}
      <Box flexDirection="column">
        {robotEntries.length > 0 ? (
          robotEntries.map(([name, state]) => (
            <RobotStatePanel
              key={name}
              name={name}
              numJoints={state.num_joints}
              positions={state.positions}
              endEffectorStatus={state.end_effector_status}
              endEffectorFeedbackValid={state.end_effector_feedback_valid}
              panelWidth={contentWidth}
            />
          ))
        ) : (
          <RobotStatePanel
            name="robot_0"
            numJoints={0}
            positions={[]}
            endEffectorStatus={undefined}
            endEffectorFeedbackValid={undefined}
            panelWidth={contentWidth}
          />
        )}
      </Box>

      {/* Info panel (horizontal mode, narrow terminals only) */}
      {!isWide && (
        <InfoPanel
          frames={frames}
          robotStates={robotStates}
          streamInfo={streamInfo}
          connected={connected}
          orientation="horizontal"
          panelWidth={columns}
        />
      )}

      {showDebug && (
        <DebugPanel
          width={columns}
          snapshot={debugSnapshot}
          streamInfo={streamInfo}
        />
      )}

      {/* Status Bar */}
      <StatusBar
        mode="Collect"
        state={effectiveEpisodeStatus.state}
        episodeCount={effectiveEpisodeStatus.episode_count}
        elapsedMs={effectiveEpisodeStatus.elapsed_ms}
        episodeKeyBindings={episodeKeyBindings}
        connected={connected}
        health={health}
        width={columns}
        debugEnabled={showDebug}
        rendererLabel={rendererLabel}
      />
    </Box>
  );
}
