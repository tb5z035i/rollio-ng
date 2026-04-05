import React from "react";
import { Box, Text } from "ink";
import type { CameraFrame } from "../lib/websocket.js";
import type { RobotStateMessage } from "../lib/protocol.js";

interface InfoPanelProps {
  frames: Map<string, CameraFrame>;
  robotStates: Map<string, RobotStateMessage>;
  connected: boolean;
  orientation: "vertical" | "horizontal";
  panelWidth: number;
}

export function InfoPanel({
  frames,
  robotStates,
  connected,
  orientation,
  panelWidth,
}: InfoPanelProps) {
  const headerText = "─ Info ";
  const headerPad = Math.max(0, panelWidth - headerText.length - 2);
  const topBorder = `┌${headerText}${"─".repeat(headerPad)}┐`;
  const bottomBorder = `└${"─".repeat(panelWidth - 2)}┘`;
  const innerW = panelWidth - 2;

  const hasData = frames.size > 0 || robotStates.size > 0;

  if (!hasData) {
    const msg = "No devices connected";
    const pad = Math.max(0, innerW - msg.length);
    const left = Math.floor(pad / 2);
    const right = pad - left;

    return (
      <Box flexDirection="column" width={panelWidth}>
        <Text dimColor>{topBorder}</Text>
        <Text dimColor>{`│${" ".repeat(left)}${msg}${" ".repeat(right)}│`}</Text>
        <Text dimColor>{bottomBorder}</Text>
      </Box>
    );
  }

  const lines: string[] = [];

  if (orientation === "vertical") {
    // Vertical: detailed list
    lines.push(padLine(" Devices", innerW));

    for (const [name, frame] of frames) {
      lines.push(padLine(`  ${name}  ${frame.width}x${frame.height}`, innerW));
    }

    for (const [name, state] of robotStates) {
      lines.push(padLine(`  ${name}  ${state.num_joints} DoF`, innerW));
    }

    lines.push(padLine("", innerW));
    lines.push(
      padLine(` WS: ${connected ? "Connected" : "Disconnected"}`, innerW),
    );
  } else {
    // Horizontal: compact 2-line strip
    const camParts: string[] = [];
    for (const [name, frame] of frames) {
      camParts.push(`${name}: ${frame.width}x${frame.height}`);
    }

    const robotParts: string[] = [];
    for (const [name, state] of robotStates) {
      robotParts.push(`${name}: ${state.num_joints} DoF`);
    }

    const line1 = ` ${camParts.join(" | ")}`;
    const line2 = ` ${robotParts.join(" | ")}`;

    lines.push(padLine(line1, innerW));
    lines.push(padLine(line2, innerW));
  }

  return (
    <Box flexDirection="column" width={panelWidth}>
      <Text dimColor>{topBorder}</Text>
      {lines.map((line, i) => (
        <Text key={i}>{`│${line}│`}</Text>
      ))}
      <Text dimColor>{bottomBorder}</Text>
    </Box>
  );
}

function padLine(text: string, width: number): string {
  const trimmed = text.substring(0, width);
  return trimmed + " ".repeat(Math.max(0, width - trimmed.length));
}
