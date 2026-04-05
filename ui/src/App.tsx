import React, { useState, useEffect } from "react";
import { Box, useStdout } from "ink";
import { useWebSocket } from "./lib/websocket.js";
import { TitleBar } from "./components/TitleBar.js";
import { StatusBar } from "./components/StatusBar.js";
import { StreamPanel } from "./components/StreamPanel.js";
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
  const hasData = frames.size > 0 || robotStates.size > 0;
  const health = connected
    ? hasData
      ? ("normal" as const)
      : ("normal" as const)
    : ("degraded" as const);

  // Layout constants
  const isWide = columns >= 120;
  const infoPanelWidth = isWide ? 25 : columns;
  const contentWidth = isWide ? columns - infoPanelWidth : columns;
  const contentHeight = Math.max(1, rows - 2); // minus title + status bars

  // Camera panel sizing
  const cameraNames = Array.from(frames.keys());
  const numCameras = Math.max(cameraNames.length, 1); // at least 1 placeholder
  const cameraWidth = Math.max(10, Math.floor(contentWidth / numCameras));

  // Allocate vertical space
  const robotPanelHeight = isWide
    ? Math.max(5, Math.floor(contentHeight * 0.3))
    : Math.max(5, Math.min(8, Math.floor(contentHeight * 0.3)));
  const infoPanelHeightH = isWide ? 0 : Math.min(5, Math.max(3, Math.floor(contentHeight * 0.15)));
  const cameraPanelHeight = Math.max(
    5,
    contentHeight - robotPanelHeight - (isWide ? 0 : infoPanelHeightH),
  );

  // Build camera panel keys (use known names or placeholders)
  const camKeys =
    cameraNames.length > 0
      ? cameraNames
      : ["camera_0", "camera_1"];

  // Build robot panel data
  const robotEntries = Array.from(robotStates.entries());

  return (
    <Box flexDirection="column" width={columns} height={rows}>
      {/* Title Bar */}
      <TitleBar mode="Collect" width={columns} />

      {/* Content Area */}
      <Box flexDirection="row" height={contentHeight}>
        {/* Main content (cameras + robots) */}
        <Box flexDirection="column" width={contentWidth}>
          {/* Camera panels row */}
          <Box flexDirection="row" height={cameraPanelHeight}>
            {camKeys.map((name) => {
              const frame = frames.get(name);
              return (
                <StreamPanel
                  key={name}
                  name={name}
                  jpegData={frame?.jpegData ?? null}
                  panelWidth={cameraWidth}
                  panelHeight={cameraPanelHeight}
                />
              );
            })}
          </Box>

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

          {/* Info panel (horizontal mode, narrow terminals) */}
          {!isWide && (
            <InfoPanel
              frames={frames}
              robotStates={robotStates}
              connected={connected}
              orientation="horizontal"
              panelWidth={columns}
            />
          )}
        </Box>

        {/* Info panel (vertical mode, wide terminals) */}
        {isWide && (
          <InfoPanel
            frames={frames}
            robotStates={robotStates}
            connected={connected}
            orientation="vertical"
            panelWidth={infoPanelWidth}
          />
        )}
      </Box>

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
