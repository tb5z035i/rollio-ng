import React from "react";
import { Box, Text } from "ink";
import type { EndEffectorStatus } from "../lib/protocol.js";

export interface RobotStatePanelProps {
  name: string;
  numJoints: number;
  positions: number[];
  panelWidth: number;
  endEffectorStatus?: EndEffectorStatus;
  endEffectorFeedbackValid?: boolean;
}

const PI = Math.PI;
const END_EFFECTOR_MIN = 0.0;
const END_EFFECTOR_MAX = 0.07;

export function buildRobotPanelLines({
  name,
  numJoints,
  positions,
  panelWidth,
  endEffectorStatus,
  endEffectorFeedbackValid,
}: RobotStatePanelProps): string[] {
  const headerText = `─ ${name} (${numJoints} DoF) `;
  const headerPad = Math.max(0, panelWidth - headerText.length - 2);
  const topBorder = `┌${headerText}${"─".repeat(headerPad)}┐`;
  const bottomBorder = `└${"─".repeat(panelWidth - 2)}┘`;
  const lines = [topBorder];

  if (numJoints === 0 || positions.length === 0) {
    lines.push(
      formatCenteredLine(
        panelWidth,
        endEffectorStatus
          ? `${formatEndEffectorStatusText(endEffectorStatus, endEffectorFeedbackValid)} | Waiting for feedback`
          : "Waiting for data...",
      ),
    );
    lines.push(bottomBorder);
    return lines;
  }

  if (endEffectorStatus) {
    lines.push(
      formatPaddedLine(
        panelWidth,
        formatEndEffectorStatusText(endEffectorStatus, endEffectorFeedbackValid),
      ),
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
      const normalized = normalizePositionForDisplay(pos, Boolean(endEffectorStatus));

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
    jointLines.push(formatPaddedLine(panelWidth, line));
  }

  lines.push(...jointLines);
  lines.push(bottomBorder);
  return lines;
}

export function RobotStatePanel(props: RobotStatePanelProps) {
  const lines = buildRobotPanelLines(props);
  return (
    <Box flexDirection="column" width={props.panelWidth}>
      {lines.map((line, index) => (
        <Text key={index} dimColor={index === 0 || index === lines.length - 1}>
          {line}
        </Text>
      ))}
    </Box>
  );
}

function formatEndEffectorStatusText(
  status: EndEffectorStatus,
  feedbackValid?: boolean,
): string {
  const feedbackLabel =
    feedbackValid === undefined ? "" : ` | Feedback: ${feedbackValid ? "ok" : "stale"}`;
  return `Status: ${status[0].toUpperCase()}${status.slice(1)}${feedbackLabel}`;
}

function formatPaddedLine(panelWidth: number, content: string): string {
  const inner = content.substring(0, panelWidth - 2);
  const pad = Math.max(0, panelWidth - 2 - inner.length);
  return `│${inner}${" ".repeat(pad)}│`;
}

function formatCenteredLine(panelWidth: number, content: string): string {
  const inner = content.substring(0, panelWidth - 2);
  const pad = Math.max(0, panelWidth - 2 - inner.length);
  const left = Math.floor(pad / 2);
  const right = pad - left;
  return `│${" ".repeat(left)}${inner}${" ".repeat(right)}│`;
}

function normalizePositionForDisplay(
  position: number,
  isEndEffector: boolean,
): number {
  if (isEndEffector) {
    const span = END_EFFECTOR_MAX - END_EFFECTOR_MIN;
    if (span <= 0) return 0;
    return Math.max(
      0,
      Math.min(1, (position - END_EFFECTOR_MIN) / span),
    );
  }

  // Generic arm joints are still visualized as angles in [-PI, PI].
  return Math.max(0, Math.min(1, (position + PI) / (2 * PI)));
}
