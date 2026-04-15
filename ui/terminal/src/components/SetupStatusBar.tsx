import React from "react";
import { Box, Text } from "ink";

type SetupHealth = "normal" | "degraded";
type SetupStatus = "editing" | "saved" | "cancelled";

interface SetupStatusBarProps {
  stepIndex: number;
  totalSteps: number;
  connected: boolean;
  outputPath: string;
  width: number;
  status: SetupStatus;
  stepHint: string;
  message?: string;
}

const HEALTH_COLORS: Record<SetupHealth, string> = {
  normal: "green",
  degraded: "yellow",
};

const HEALTH_LABELS: Record<SetupHealth, string> = {
  normal: "[Ready]",
  degraded: "[Waiting]",
};

export function buildSetupStatusBarLeft(props: {
  stepIndex: number;
  totalSteps: number;
  connected: boolean;
  outputPath: string;
  status: SetupStatus;
  stepHint: string;
  message?: string;
}): string {
  const connection = props.connected ? "Connected" : "Connecting";
  const output = props.outputPath.length > 28
    ? `...${props.outputPath.slice(-25)}`
    : props.outputPath;
  const summary = props.message ?? props.stepHint;
  return (
    ` Setup | ${props.stepIndex}/${props.totalSteps} | WS: ${connection}` +
    ` | ${summary} | File: ${output}`
  );
}

export function SetupStatusBar({
  stepIndex,
  totalSteps,
  connected,
  outputPath,
  width,
  status,
  stepHint,
  message,
}: SetupStatusBarProps) {
  const health: SetupHealth = connected ? "normal" : "degraded";
  const left = buildSetupStatusBarLeft({
    stepIndex,
    totalSteps,
    connected,
    outputPath,
    status,
    stepHint,
    message,
  });
  const right =
    status === "saved"
      ? " [Saved] "
      : status === "cancelled"
        ? " [Cancelled] "
        : ` ${HEALTH_LABELS[health]} `;
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
