import React, { useState, useEffect } from "react";
import { Box, useInput, useStdin, useStdout } from "ink";
import { useWebSocket } from "./lib/websocket.js";
import { encodeEpisodeCommand } from "./lib/protocol.js";
import { getTerminalMetrics } from "./lib/terminal-geometry.js";
import { TitleBar } from "./components/TitleBar.js";
import { StatusBar } from "./components/StatusBar.js";
import { getAsciiRendererLabel, nextAsciiRendererId, type AsciiRendererId } from "./lib/renderers/index.js";
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
import { LivePreviewPanels } from "./components/LivePreviewPanels.js";

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
  const debugPanelHeight = showDebug ? DEBUG_PANEL_HEIGHT : 0;
  const contentHeight = Math.max(1, rows - 2 - debugPanelHeight); // minus title + status bars
  const rendererLabel = getAsciiRendererLabel(cameraRendererId);
  const effectiveEpisodeStatus = episodeStatus ?? {
    type: "episode_status" as const,
    state: "idle" as const,
    episode_count: 0,
    elapsed_ms: 0,
  };

  useEffect(() => {
    setGauge("ui.layout", `${columns}x${rows}`);
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
    frames.size,
    robotStates.size,
    showDebug,
    cameraRendererId,
    rendererLabel,
    effectiveEpisodeStatus.elapsed_ms,
    effectiveEpisodeStatus.episode_count,
    effectiveEpisodeStatus.state,
    streamInfo,
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

      <LivePreviewPanels
        frames={frames}
        robotStates={robotStates}
        streamInfo={streamInfo}
        connected={connected}
        send={send}
        width={columns}
        availableRows={contentHeight}
        cellGeometry={cellGeometry}
        rendererId={cameraRendererId}
      />

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
