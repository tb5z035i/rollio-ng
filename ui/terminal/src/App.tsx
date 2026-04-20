import React, { useState, useEffect } from "react";
import { Box, useInput, useStdin, useStdout } from "ink";
import { useControlSocket, usePreviewSocket } from "./lib/websocket.js";
import { encodeEpisodeCommand } from "./lib/protocol.js";
import { getTerminalMetrics } from "./lib/terminal-geometry.js";
import { TitleBar } from "./components/TitleBar.js";
import { StatusBar } from "./components/StatusBar.js";
import { KeyHintsBar, type KeyHint } from "./components/KeyHintsBar.js";
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
  controlWebsocketUrl: string;
  previewWebsocketUrl: string;
  initialAsciiRendererId: AsciiRendererId;
  episodeKeyBindings: EpisodeKeyBindings;
};

export function App({
  controlWebsocketUrl,
  previewWebsocketUrl,
  initialAsciiRendererId,
  episodeKeyBindings,
}: AppProps) {
  const renderStartMs = nowMs();
  const { columns, rows, cellGeometry } = useTerminalMetrics();
  const { isRawModeSupported } = useStdin();
  const supportsInteractiveInput = isRawModeSupported === true;
  const {
    connected: controlConnected,
    send: sendControl,
    episodeStatus,
  } = useControlSocket(controlWebsocketUrl);
  const {
    connected: previewConnected,
    send: sendPreview,
    frames,
    robotChannels,
    streamInfo,
  } = usePreviewSocket(previewWebsocketUrl);
  // Surface "fully connected" only when both planes are up. The episode
  // command path lives on the control socket; previews lighting up second
  // shouldn't degrade the indicator alone.
  const connected = controlConnected;
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
        // Episode commands are control plane traffic — route via the
        // control socket, not the preview socket.
        sendControl(encodeEpisodeCommand(action));
        setGauge("ui.last_episode_command", action);
      }
    },
    { isActive: supportsInteractiveInput },
  );

  // Layout constants
  const debugPanelHeight = showDebug ? DEBUG_PANEL_HEIGHT : 0;
  // Title bar (1) + KeyHintsBar (1) + StatusBar (1) = 3 rows reserved for
  // chrome around the live preview area. Plus the debug panel when on.
  const contentHeight = Math.max(1, rows - 3 - debugPanelHeight);
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
    setGauge("ui.robot_count", robotChannels.size);
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
    robotChannels.size,
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

  const collectKeyHints = buildCollectKeyHints({
    episodeState: effectiveEpisodeStatus.state,
    episodeKeyBindings,
    showDebug,
    rendererLabel,
    hasCameraFrames: frames.size > 0,
  });

  return (
    <Box flexDirection="column" width={columns} height={rows}>
      {/* Title Bar */}
      <TitleBar mode="Collect" width={columns} />

      <LivePreviewPanels
        frames={frames}
        robotChannels={robotChannels}
        streamInfo={streamInfo}
        connected={previewConnected}
        send={sendPreview}
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

      <KeyHintsBar hints={collectKeyHints} width={columns} />
      {/* Status Bar */}
      <StatusBar
        mode="Collect"
        state={effectiveEpisodeStatus.state}
        episodeCount={effectiveEpisodeStatus.episode_count}
        elapsedMs={effectiveEpisodeStatus.elapsed_ms}
        connected={connected}
        health={health}
        width={columns}
      />
    </Box>
  );
}

type BuildCollectKeyHintsArgs = {
  episodeState: "idle" | "recording" | "pending";
  episodeKeyBindings: EpisodeKeyBindings;
  showDebug: boolean;
  rendererLabel: string;
  hasCameraFrames: boolean;
};

/** Per-episode-state key hint set for collect mode. The state-specific
 *  controls always lead so the next-step action sits where the operator
 *  expects it; debug + renderer toggles trail. `r:Renderer` is gated on
 *  `hasCameraFrames` to mirror the wizard rule. */
function buildCollectKeyHints({
  episodeState,
  episodeKeyBindings,
  showDebug,
  rendererLabel,
  hasCameraFrames,
}: BuildCollectKeyHintsArgs): KeyHint[] {
  const stateHints: KeyHint[] = (() => {
    switch (episodeState) {
      case "idle":
        return [{ key: episodeKeyBindings.startKey, label: "Start" }];
      case "recording":
        return [{ key: episodeKeyBindings.stopKey, label: "Stop" }];
      case "pending":
        return [
          { key: episodeKeyBindings.keepKey, label: "Keep" },
          { key: episodeKeyBindings.discardKey, label: "Discard" },
        ];
    }
  })();
  const tail: KeyHint[] = [
    { key: "d", label: `Debug [${showDebug ? "On" : "Off"}]` },
  ];
  if (hasCameraFrames) {
    tail.push({ key: "r", label: `Renderer [${rendererLabel}]` });
  }
  return [...stateHints, ...tail];
}
