import React from "react";
import { Box, Text } from "ink";

interface RobotStatePanelProps {
  name: string;
  numJoints: number;
  positions: number[];
  panelWidth: number;
}

const PI = Math.PI;

export function RobotStatePanel({
  name,
  numJoints,
  positions,
  panelWidth,
}: RobotStatePanelProps) {
  const headerText = `─ ${name} (${numJoints} DoF) `;
  const headerPad = Math.max(0, panelWidth - headerText.length - 2);
  const topBorder = `┌${headerText}${"─".repeat(headerPad)}┐`;
  const bottomBorder = `└${"─".repeat(panelWidth - 2)}┘`;

  if (numJoints === 0 || positions.length === 0) {
    const msg = "Waiting for data...";
    const innerW = panelWidth - 2;
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

  // Layout: 2 columns if width allows, otherwise single column
  const useTwoColumns = panelWidth > 60;
  const cols = useTwoColumns ? 2 : 1;
  const colWidth = Math.floor((panelWidth - 2) / cols);

  const jointLines: string[] = [];

  for (let row = 0; row < Math.ceil(numJoints / cols); row++) {
    let line = "";
    for (let col = 0; col < cols; col++) {
      const j = row + col * Math.ceil(numJoints / cols);
      if (j >= numJoints) {
        line += " ".repeat(colWidth);
        continue;
      }

      const pos = positions[j] ?? 0;
      // Normalize from [-PI, PI] to [0, 1]
      const normalized = Math.max(0, Math.min(1, (pos + PI) / (2 * PI)));

      const label = `J${j} `;
      const value = ` ${pos >= 0 ? " " : ""}${pos.toFixed(2)}`;
      const barSpace = Math.max(1, colWidth - label.length - value.length - 2);
      const filled = Math.round(normalized * barSpace);
      const empty = barSpace - filled;

      const bar = "█".repeat(filled) + "░".repeat(empty);
      const cell = `${label}${bar}${value}`;
      // Pad to column width
      line += cell + " ".repeat(Math.max(0, colWidth - cell.length));
    }

    // Trim to inner width and wrap with borders
    const inner = line.substring(0, panelWidth - 2);
    const pad = Math.max(0, panelWidth - 2 - inner.length);
    jointLines.push(`│${inner}${" ".repeat(pad)}│`);
  }

  return (
    <Box flexDirection="column" width={panelWidth}>
      <Text dimColor>{topBorder}</Text>
      {jointLines.map((line, i) => (
        <Text key={i}>{line}</Text>
      ))}
      <Text dimColor>{bottomBorder}</Text>
    </Box>
  );
}
