import React, { useState, useEffect, useMemo } from "react";
import { Box, useStdout } from "ink";
import { useWebSocket } from "./lib/websocket.js";
import { TitleBar } from "./components/TitleBar.js";
import { StatusBar } from "./components/StatusBar.js";
import { CameraRow } from "./components/StreamPanel.js";
import { RobotStatePanel } from "./components/RobotStatePanel.js";
import { InfoPanel } from "./components/InfoPanel.js";

/** Custom hook to track terminal dimensions with live resize updates. */
function useTerminalSize() {
  const { stdout } = useStdout();
  const [size, setSize] = useState({
    columns: stdout.columns || 80,
    rows: stdout.rows || 24,
  });

  useEffect(() => {
    const onResize = () => {
      setSize({
        columns: stdout.columns || 80,
        rows: stdout.rows || 24,
      });
    };

    stdout.on("resize", onResize);
    return () => {
      stdout.off("resize", onResize);
    };
  }, [stdout]);

  return size;
}

export function App() {
  const { columns, rows } = useTerminalSize();
  const { frames, robotStates, connected } = useWebSocket("ws://localhost:9090");

  // Derive health status
  const health = connected ? ("normal" as const) : ("degraded" as const);

  // Layout constants
  const isWide = columns >= 120;
  const infoPanelWidth = isWide ? 25 : columns;
  const contentWidth = isWide ? columns - infoPanelWidth : columns;
  const contentHeight = Math.max(1, rows - 2); // minus title + status bars

  // Camera panel sizing
  const cameraNames = Array.from(frames.keys());
  const camKeys = cameraNames.length > 0 ? cameraNames : ["camera_0", "camera_1"];

  // Allocate vertical space
  const robotPanelHeight = isWide
    ? Math.max(5, Math.floor(contentHeight * 0.3))
    : Math.max(5, Math.min(8, Math.floor(contentHeight * 0.3)));
  const infoPanelHeightH = isWide
    ? 0
    : Math.min(5, Math.max(3, Math.floor(contentHeight * 0.15)));
  const cameraPanelHeight = Math.max(
    5,
    contentHeight - robotPanelHeight - (isWide ? 0 : infoPanelHeightH),
  );

  // Build camera data for CameraRow
  const cameraData = useMemo(
    () =>
      camKeys.map((name) => ({
        name,
        frame: frames.get(name),
      })),
    [camKeys, frames],
  );

  // Build info panel lines for wide mode (merged into camera row)
  const infoPanelLines = useMemo(() => {
    if (!isWide) return undefined;

    const w = infoPanelWidth;
    const lines: string[] = [];
    const pad = (s: string) => {
      const trimmed = s.substring(0, w);
      return trimmed + " ".repeat(Math.max(0, w - trimmed.length));
    };

    // Top border
    const headerText = "─ Info ";
    const headerPad = Math.max(0, w - headerText.length - 1);
    lines.push(`${headerText}${"─".repeat(headerPad)}┐`);

    // Content
    lines.push(pad(" Devices") + "│");
    for (const [name, frame] of frames) {
      lines.push(pad(`  ${name}  ${frame.width}x${frame.height}`) + "│");
    }
    for (const [name, state] of robotStates) {
      lines.push(pad(`  ${name}  ${state.num_joints} DoF`) + "│");
    }
    lines.push(pad("") + "│");
    lines.push(pad(` WS: ${connected ? "Connected" : "Disconnected"}`) + "│");

    // Pad remaining rows
    const totalRows = cameraPanelHeight + 2; // +2 for borders
    while (lines.length < totalRows - 1) {
      lines.push(pad("") + "│");
    }
    // Bottom border
    lines.push(`${"─".repeat(w - 1)}┘`);

    return lines;
  }, [isWide, infoPanelWidth, frames, robotStates, connected, cameraPanelHeight]);

  // Build robot panel data
  const robotEntries = Array.from(robotStates.entries());

  return (
    <Box flexDirection="column" width={columns} height={rows}>
      {/* Title Bar */}
      <TitleBar mode="Collect" width={columns} />

      {/* Camera row (pre-composed ANSI lines, bypasses Ink width measurement) */}
      <CameraRow
        cameras={cameraData}
        totalWidth={contentWidth}
        panelHeight={cameraPanelHeight}
        infoPanelLines={infoPanelLines}
      />

      {/* Robot state panels */}
      <Box flexDirection="column">
        {robotEntries.length > 0 ? (
          robotEntries.map(([name, state]) => (
            <RobotStatePanel
              key={name}
              name={name}
              numJoints={state.num_joints}
              positions={state.positions}
              panelWidth={contentWidth}
            />
          ))
        ) : (
          <RobotStatePanel
            name="robot_0"
            numJoints={0}
            positions={[]}
            panelWidth={contentWidth}
          />
        )}
      </Box>

      {/* Info panel (horizontal mode, narrow terminals only) */}
      {!isWide && (
        <InfoPanel
          frames={frames}
          robotStates={robotStates}
          connected={connected}
          orientation="horizontal"
          panelWidth={columns}
        />
      )}

      {/* Status Bar */}
      <StatusBar
        mode="Collect"
        state="Idle"
        episodeCount={0}
        connected={connected}
        health={health}
        width={columns}
      />
    </Box>
  );
}
