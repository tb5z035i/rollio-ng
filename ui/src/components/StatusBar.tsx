import React from "react";
import { Box, Text } from "ink";

type HealthStatus = "normal" | "degraded" | "failure";

interface StatusBarProps {
  mode: string;
  state: string;
  episodeCount: number;
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

export function StatusBar({
  mode,
  state,
  episodeCount,
  connected,
  health,
  width,
}: StatusBarProps) {
  const connStatus = connected ? "Connected" : "Disconnected";
  const left = ` ${mode} | ${state} | Ep: ${episodeCount} | WS: ${connStatus}`;
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
