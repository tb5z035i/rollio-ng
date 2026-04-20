import React from "react";
import { Box, Text } from "ink";

type HealthStatus = "normal" | "degraded" | "failure";
type EpisodeState = "idle" | "recording" | "pending";

interface StatusBarProps {
  mode: string;
  state: EpisodeState;
  episodeCount: number;
  elapsedMs: number;
  connected: boolean;
  health: HealthStatus;
  width: number;
}

const HEALTH_COLORS: Record<HealthStatus, string> = {
  normal: "green",
  degraded: "yellow",
  failure: "red",
};

const HEALTH_LABELS: Record<HealthStatus, string> = {
  normal: "[Normal]",
  degraded: "[Degraded]",
  failure: "[Failure]",
};

function formatEpisodeState(state: EpisodeState): string {
  switch (state) {
    case "idle":
      return "Idle";
    case "recording":
      return "Recording";
    case "pending":
      return "Pending";
  }
}

export function formatElapsedMs(elapsedMs: number): string {
  const totalSeconds = Math.max(0, Math.floor(elapsedMs / 1000));
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  return `${minutes}:${seconds.toString().padStart(2, "0")}`;
}

/** Status bar text excluding key hints and debug/renderer toggles —
 *  those moved to a dedicated `KeyHintsBar` row above the status bar.
 *  Keeps the bar focused on always-visible session metadata: mode,
 *  episode state/elapsed, episode count, and websocket health. */
export function buildStatusBarLeft(props: {
  mode: string;
  state: EpisodeState;
  episodeCount: number;
  elapsedMs: number;
  connected: boolean;
}): string {
  const connStatus = props.connected ? "Connected" : "Disconnected";
  const stateLabel =
    props.state === "recording"
      ? `${formatEpisodeState(props.state)} ${formatElapsedMs(props.elapsedMs)}`
      : formatEpisodeState(props.state);
  return ` ${props.mode} | ${stateLabel} | Ep: ${props.episodeCount} | WS: ${connStatus}`;
}

export function StatusBar({
  mode,
  state,
  episodeCount,
  elapsedMs,
  connected,
  health,
  width,
}: StatusBarProps) {
  const left = buildStatusBarLeft({
    mode,
    state,
    episodeCount,
    elapsedMs,
    connected,
  });
  const right = ` ${HEALTH_LABELS[health]} `;
  const padding = Math.max(0, width - left.length - right.length);

  return (
    <Box width={width}>
      <Text>
        {left}
        {" ".repeat(padding)}
      </Text>
      <Text color={HEALTH_COLORS[health]}>{right}</Text>
    </Box>
  );
}
