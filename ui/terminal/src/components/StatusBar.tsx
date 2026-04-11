import React from "react";
import { Box, Text } from "ink";
import type { EpisodeKeyBindings } from "../runtime-config.js";

type HealthStatus = "normal" | "degraded" | "failure";
type EpisodeState = "idle" | "recording" | "pending";

interface StatusBarProps {
  mode: string;
  state: EpisodeState;
  episodeCount: number;
  elapsedMs: number;
  episodeKeyBindings: EpisodeKeyBindings;
  connected: boolean;
  health: HealthStatus;
  width: number;
  debugEnabled?: boolean;
  rendererLabel?: string;
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

export function buildStatusBarLeft(props: {
  mode: string;
  state: EpisodeState;
  episodeCount: number;
  elapsedMs: number;
  episodeKeyBindings: EpisodeKeyBindings;
  connected: boolean;
  debugEnabled?: boolean;
  rendererLabel?: string;
}): string {
  const connStatus = props.connected ? "Connected" : "Disconnected";
  const debugStatus = props.debugEnabled ? "On" : "Off";
  const rendererStatus = props.rendererLabel
    ? ` | r:Render ${props.rendererLabel}`
    : "";
  const stateLabel =
    props.state === "recording"
      ? `${formatEpisodeState(props.state)} ${formatElapsedMs(props.elapsedMs)}`
      : formatEpisodeState(props.state);
  const controlHint =
    props.state === "idle"
      ? `${props.episodeKeyBindings.startKey}:Start`
      : props.state === "recording"
        ? `${props.episodeKeyBindings.stopKey}:Stop`
        : `${props.episodeKeyBindings.keepKey}:Keep ${props.episodeKeyBindings.discardKey}:Discard`;
  return (
    ` ${props.mode} | ${stateLabel} | Ep: ${props.episodeCount} | WS: ${connStatus}` +
    ` | ${controlHint} | d:Debug ${debugStatus}${rendererStatus}`
  );
}

export function StatusBar({
  mode,
  state,
  episodeCount,
  elapsedMs,
  episodeKeyBindings,
  connected,
  health,
  width,
  debugEnabled = false,
  rendererLabel,
}: StatusBarProps) {
  const left = buildStatusBarLeft({
    mode,
    state,
    episodeCount,
    elapsedMs,
    episodeKeyBindings,
    connected,
    debugEnabled,
    rendererLabel,
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
