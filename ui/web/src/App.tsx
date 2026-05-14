import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { CameraGrid } from "./components/CameraGrid";
import { DebugPanel } from "./components/DebugPanel";
import { InfoPanel } from "./components/InfoPanel";
import { RobotStatePanel } from "./components/RobotStatePanel";
import { StatusBar } from "./components/StatusBar";
import { TitleBar } from "./components/TitleBar";
import { resolveCameraNames } from "./lib/camera-layout";
import { actionForInput } from "./lib/controls";
import {
  nowMs,
  recordTiming,
  setGauge,
  snapshotDebugMetrics,
  type DebugSnapshot,
} from "./lib/debug-metrics";
import {
  buildPreviewNegotiationKey,
  isWideLayout,
  negotiatePreviewDimensions,
  type PreviewDimensions,
} from "./lib/layout";
import { encodeEpisodeCommand, encodeSetPreviewSize } from "./lib/protocol";
import type { UiRuntimeConfig } from "./lib/runtime-config";
import {
  useControlSocket,
  usePreviewSocket,
  type UseControlSocketOptions,
  type UsePreviewSocketOptions,
} from "./lib/websocket";

type AppProps = {
  runtimeConfig: UiRuntimeConfig;
  controlSocketOptions?: UseControlSocketOptions;
  previewSocketOptions?: UsePreviewSocketOptions;
};

type ViewportSize = {
  width: number;
  height: number;
};

function useViewportSize(): ViewportSize {
  const [size, setSize] = useState<ViewportSize>(() => ({
    width: window.innerWidth,
    height: window.innerHeight,
  }));

  useEffect(() => {
    const onResize = () => {
      setSize({
        width: window.innerWidth,
        height: window.innerHeight,
      });
    };

    window.addEventListener("resize", onResize);
    return () => {
      window.removeEventListener("resize", onResize);
    };
  }, []);

  return size;
}

function shouldIgnoreKeydown(event: KeyboardEvent): boolean {
  if (event.ctrlKey || event.metaKey || event.altKey || event.repeat) {
    return true;
  }

  const target = event.target;
  if (!(target instanceof HTMLElement)) {
    return false;
  }

  const tagName = target.tagName.toLowerCase();
  return (
    tagName === "input" ||
    tagName === "textarea" ||
    tagName === "select" ||
    target.isContentEditable
  );
}

function fallbackPreviewTileSize(
  viewport: ViewportSize,
  cameraCount: number,
  wideLayout: boolean,
): PreviewDimensions {
  const safeCameraCount = Math.max(1, cameraCount);
  const horizontalPadding = wideLayout ? 420 : 64;
  const verticalBudget = wideLayout ? viewport.height * 0.28 : viewport.height * 0.22;
  return {
    width: Math.max(160, Math.floor((viewport.width - horizontalPadding) / safeCameraCount)),
    height: Math.max(120, Math.floor(verticalBudget)),
  };
}

export default function App({
  runtimeConfig,
  controlSocketOptions,
  previewSocketOptions,
}: AppProps) {
  const renderStartMs = nowMs();
  const viewport = useViewportSize();
  const wideLayout = isWideLayout(viewport.width);
  const {
    episodeStatus,
    connected: controlConnected,
    send: sendControl,
  } = useControlSocket(runtimeConfig.controlWebsocketUrl, controlSocketOptions);
  const {
    frames,
    robotChannels,
    streamInfo,
    connected: previewConnected,
    send: sendPreview,
  } = usePreviewSocket(runtimeConfig.previewWebsocketUrl, previewSocketOptions);
  // Episode status drives the visible "connected" indicator since that's the
  // socket the user actually depends on for control. Preview-only outages
  // surface as missing frames.
  const connected = controlConnected;
  const [showDebug, setShowDebug] = useState(false);
  const [previewTileSize, setPreviewTileSize] = useState<PreviewDimensions | null>(null);
  const lastPreviewNegotiationKeyRef = useRef<string | null>(null);
  const [debugSnapshot, setDebugSnapshot] = useState<DebugSnapshot>(() =>
    snapshotDebugMetrics(),
  );

  const cameraNames = Array.from(frames.keys());
  const configuredCameraNames = streamInfo?.cameras?.map((camera) => camera.name) ?? [];
  const resolvedCameraNames = useMemo(
    () => resolveCameraNames(configuredCameraNames, cameraNames),
    [cameraNames, configuredCameraNames],
  );
  const cameraData = useMemo(
    () => resolvedCameraNames.map((name) => ({ name, frame: frames.get(name) })),
    [frames, resolvedCameraNames],
  );
  const robotEntries = Array.from(robotChannels.values()).sort((a, b) =>
    a.name.localeCompare(b.name),
  );
  const effectiveEpisodeStatus = episodeStatus ?? {
    type: "episode_status" as const,
    state: "idle" as const,
    episode_count: 0,
    elapsed_ms: 0,
  };
  const health = connected ? ("normal" as const) : ("degraded" as const);
  const requestedPreviewTileSize =
    previewTileSize ??
    fallbackPreviewTileSize(viewport, resolvedCameraNames.length, wideLayout);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (shouldIgnoreKeydown(event) || event.key.length !== 1) {
        return;
      }

      const action = actionForInput(event.key, runtimeConfig.episodeKeyBindings);
      if (action == null) {
        return;
      }

      event.preventDefault();
      if (action === "toggle_debug") {
        setShowDebug((previous) => !previous);
        return;
      }

      sendControl(encodeEpisodeCommand(action));
      setGauge("ui.last_episode_command", action);
    };

    window.addEventListener("keydown", onKeyDown);
    return () => {
      window.removeEventListener("keydown", onKeyDown);
    };
  }, [runtimeConfig.episodeKeyBindings, sendControl]);

  const handlePreviewSizeChange = useCallback((size: PreviewDimensions) => {
    setPreviewTileSize(size);
  }, []);

  // When ANY active camera reports `scaling_locked` on stream_info,
  // the preview encoder cannot accept resize requests for that stream
  // (passthrough mode pins output dims to source dims). Avoid sending
  // `set_preview_size` against any stream — the visualizer would log
  // a noisy rejection. The UI still tracks the latest negotiated tile
  // size locally so layout/rendering proceed normally.
  const scalingLocked =
    streamInfo?.cameras?.some((camera) => camera.scaling_locked === true) ?? false;

  useEffect(() => {
    if (!previewConnected) {
      lastPreviewNegotiationKeyRef.current = null;
      return;
    }
    if (scalingLocked) {
      lastPreviewNegotiationKeyRef.current = null;
      setGauge("ui.preview_request", "locked");
      setGauge("ui.preview_target", "locked");
      return;
    }

    const negotiatedSize = negotiatePreviewDimensions(
      requestedPreviewTileSize,
      window.devicePixelRatio || 1,
    );
    const negotiationKey = buildPreviewNegotiationKey(
      viewport.width,
      viewport.height,
      negotiatedSize,
    );
    if (lastPreviewNegotiationKeyRef.current === negotiationKey) {
      return;
    }

    const sendResize = () => {
      sendPreview(encodeSetPreviewSize(negotiatedSize.width, negotiatedSize.height));
      lastPreviewNegotiationKeyRef.current = negotiationKey;
      setGauge("ui.preview_request", `${negotiatedSize.width}x${negotiatedSize.height}`);
      setGauge("ui.preview_target", `${negotiatedSize.width}x${negotiatedSize.height}`);
    };

    const delayMs = lastPreviewNegotiationKeyRef.current === null ? 0 : 75;
    if (delayMs === 0) {
      sendResize();
      return;
    }

    const timer = window.setTimeout(sendResize, delayMs);

    return () => {
      window.clearTimeout(timer);
    };
  }, [
    previewConnected,
    requestedPreviewTileSize,
    scalingLocked,
    sendPreview,
    viewport.height,
    viewport.width,
  ]);

  useEffect(() => {
    setGauge(
      "ui.layout",
      `${viewport.width}x${viewport.height} ${wideLayout ? "wide" : "narrow"}`,
    );
    setGauge("ui.robot_count", robotChannels.size);
    setGauge("ui.debug_enabled", showDebug ? "On" : "Off");
    setGauge("ui.episode_state", effectiveEpisodeStatus.state);
    setGauge("ui.episode_count", effectiveEpisodeStatus.episode_count);
    setGauge("ui.episode_elapsed_ms", effectiveEpisodeStatus.elapsed_ms);
    setGauge("ui.stream_info_available", streamInfo ? "Ready" : "Waiting");
    setGauge(
      "ui.preview_active",
      streamInfo
        ? `${streamInfo.active_preview_width}x${streamInfo.active_preview_height}`
        : "Waiting",
    );
  }, [
    effectiveEpisodeStatus.elapsed_ms,
    effectiveEpisodeStatus.episode_count,
    effectiveEpisodeStatus.state,
    robotChannels.size,
    showDebug,
    streamInfo,
    viewport.height,
    viewport.width,
    wideLayout,
  ]);

  useEffect(() => {
    if (!showDebug) {
      return;
    }
    setDebugSnapshot(snapshotDebugMetrics());
    const interval = window.setInterval(() => {
      setDebugSnapshot(snapshotDebugMetrics());
    }, 250);
    return () => {
      window.clearInterval(interval);
    };
  }, [showDebug]);

  const renderDurationMs = nowMs() - renderStartMs;
  useEffect(() => {
    recordTiming("app.render", renderDurationMs);
  });

  return (
    <div className="app-shell">
      <TitleBar mode="Collect" />

      <main className="app-main">
        <section
          className={`camera-layout ${wideLayout ? "camera-layout--wide" : "camera-layout--narrow"}`}
        >
          <div className="camera-layout__primary">
            <CameraGrid cameras={cameraData} onPreviewSizeChange={handlePreviewSizeChange} />
          </div>
          {wideLayout ? (
            <div className="camera-layout__sidebar">
              <InfoPanel
                connected={connected}
                frames={frames}
                orientation="vertical"
                robotChannels={robotChannels}
                streamInfo={streamInfo}
              />
            </div>
          ) : null}
        </section>

        <section className="robot-panels">
          {robotEntries.length > 0 ? (
            robotEntries.map((channel) => (
              <RobotStatePanel key={channel.name} channel={channel} />
            ))
          ) : (
            <RobotStatePanel
              channel={{
                name: "robot_0",
                states: {},
                lastTimestampMs: 0,
              }}
            />
          )}
        </section>

        {!wideLayout ? (
          <InfoPanel
            connected={connected}
            frames={frames}
            orientation="horizontal"
            robotChannels={robotChannels}
            streamInfo={streamInfo}
          />
        ) : null}

        {showDebug ? (
          <DebugPanel snapshot={debugSnapshot} streamInfo={streamInfo} />
        ) : null}
      </main>

      <StatusBar
        connected={connected}
        debugEnabled={showDebug}
        elapsedMs={effectiveEpisodeStatus.elapsed_ms}
        episodeCount={effectiveEpisodeStatus.episode_count}
        episodeKeyBindings={runtimeConfig.episodeKeyBindings}
        health={health}
        mode="Collect"
        state={effectiveEpisodeStatus.state}
      />
    </div>
  );
}
