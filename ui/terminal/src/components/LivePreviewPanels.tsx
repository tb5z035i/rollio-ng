import React, { useEffect, useMemo, useRef } from "react";
import { Box } from "ink";
import { encodeSetPreviewSize, type RobotStateMessage, type StreamInfoMessage } from "../lib/protocol.js";
import type { CameraFrame } from "../lib/websocket.js";
import { resolveCameraNames } from "../lib/camera-layout.js";
import { CameraRow, describeCameraPreviewRaster } from "./StreamPanel.js";
import { InfoPanel } from "./InfoPanel.js";
import { RobotStatePanel } from "./RobotStatePanel.js";
import type { AsciiCellGeometry, AsciiRendererId } from "../lib/renderers/index.js";

interface LivePreviewPanelsProps {
  frames: Map<string, CameraFrame>;
  robotStates: Map<string, RobotStateMessage>;
  streamInfo: StreamInfoMessage | null;
  connected: boolean;
  send: (msg: string) => void;
  width: number;
  availableRows: number;
  cellGeometry: AsciiCellGeometry;
  rendererId: AsciiRendererId;
  preferredCameraNames?: string[];
  hideEmptyRobotPanel?: boolean;
}

export function LivePreviewPanels({
  frames,
  robotStates,
  streamInfo,
  connected,
  send,
  width,
  availableRows,
  cellGeometry,
  rendererId,
  preferredCameraNames,
  hideEmptyRobotPanel = false,
}: LivePreviewPanelsProps) {
  const lastPreviewNegotiationKeyRef = useRef<string | null>(null);
  const isWide = width >= 120;
  const infoPanelWidth = isWide ? 25 : width;
  const contentWidth = isWide ? width - infoPanelWidth : width;
  const contentHeight = Math.max(1, availableRows);
  const activeFrameNames = Array.from(frames.keys());
  const configuredCameraNames = streamInfo?.cameras.map((camera) => camera.name) ?? [];
  const cameraNames = useMemo(() => {
    if (preferredCameraNames) {
      return mergePreferredCameraNames(preferredCameraNames, activeFrameNames);
    }
    return resolveCameraNames(configuredCameraNames, activeFrameNames);
  }, [activeFrameNames, configuredCameraNames, preferredCameraNames]);
  const robotPanelHeight = isWide
    ? Math.max(5, Math.floor(contentHeight * 0.3))
    : Math.max(5, Math.min(8, Math.floor(contentHeight * 0.3)));
  const infoPanelHeight = isWide
    ? 0
    : Math.min(5, Math.max(3, Math.floor(contentHeight * 0.15)));
  const cameraPanelHeight = Math.max(
    5,
    contentHeight - robotPanelHeight - (isWide ? 0 : infoPanelHeight),
  );
  const cameraPreviewRaster = useMemo(
    () =>
      describeCameraPreviewRaster(
        contentWidth,
        cameraPanelHeight,
        Math.max(cameraNames.length, 1),
        cellGeometry,
        rendererId,
      ),
    [cameraNames.length, cameraPanelHeight, cellGeometry, contentWidth, rendererId],
  );
  const cameraData = useMemo(
    () =>
      cameraNames.map((name) => ({
        name,
        frame: frames.get(name),
      })),
    [cameraNames, frames],
  );
  const cameraInfoByName = useMemo(
    () => new Map((streamInfo?.cameras ?? []).map((camera) => [camera.name, camera])),
    [streamInfo],
  );
  const infoPanelLines = useMemo(() => {
    if (!isWide) {
      return undefined;
    }

    const panelWidth = infoPanelWidth;
    const innerWidth = Math.max(0, panelWidth - 1);
    const lines: string[] = [];
    const pad = (value: string) => {
      const trimmed = value.substring(0, innerWidth);
      return trimmed + " ".repeat(Math.max(0, innerWidth - trimmed.length));
    };

    const headerText = "─ Info ";
    const headerPad = Math.max(0, panelWidth - headerText.length - 1);
    lines.push(`${headerText}${"─".repeat(headerPad)}┐`);
    lines.push(pad(" Devices") + "│");
    for (const name of cameraNames) {
      const frame = frames.get(name);
      const cameraInfo = cameraInfoByName.get(name);
      const resolution =
        cameraInfo?.source_width != null && cameraInfo.source_height != null
          ? `${cameraInfo.source_width}x${cameraInfo.source_height}`
          : frame
            ? `${frame.previewWidth}x${frame.previewHeight}`
            : "n/a";
      lines.push(pad(`  ${name}  ${resolution}`) + "│");
    }
    for (const [name, state] of robotStates) {
      lines.push(pad(`  ${name}  ${state.num_joints} DoF`) + "│");
    }
    lines.push(pad("") + "│");
    lines.push(pad(` WS: ${connected ? "Connected" : "Disconnected"}`) + "│");

    const totalRows = cameraPanelHeight + 2;
    while (lines.length < totalRows - 1) {
      lines.push(pad("") + "│");
    }
    lines.push(`${"─".repeat(panelWidth - 1)}┘`);
    return lines;
  }, [
    cameraInfoByName,
    cameraNames,
    cameraPanelHeight,
    connected,
    frames,
    infoPanelWidth,
    isWide,
    robotStates,
  ]);
  const robotEntries = Array.from(robotStates.entries());

  useEffect(() => {
    if (!connected) {
      lastPreviewNegotiationKeyRef.current = null;
      return;
    }

    const negotiationKey = [
      `${width}x${availableRows}`,
      `${cameraPreviewRaster.width}x${cameraPreviewRaster.height}`,
    ].join(":");
    if (lastPreviewNegotiationKeyRef.current === negotiationKey) {
      return;
    }

    const delayMs = lastPreviewNegotiationKeyRef.current === null ? 0 : 75;
    const timer = setTimeout(() => {
      send(
        encodeSetPreviewSize(
          cameraPreviewRaster.width,
          cameraPreviewRaster.height,
        ),
      );
      lastPreviewNegotiationKeyRef.current = negotiationKey;
    }, delayMs);

    return () => {
      clearTimeout(timer);
    };
  }, [
    availableRows,
    cameraPreviewRaster.height,
    cameraPreviewRaster.width,
    connected,
    send,
    width,
  ]);

  return (
    <Box flexDirection="column">
      {cameraData.length > 0 && (
        <CameraRow
          cameras={cameraData}
          previewRaster={cameraPreviewRaster}
          cellGeometry={cellGeometry}
          rendererId={rendererId}
          infoPanelLines={infoPanelLines}
          hasRightPanel={isWide}
        />
      )}

      <Box flexDirection="column">
        {robotEntries.length > 0 ? (
          robotEntries.map(([name, state]) => (
            <RobotStatePanel
              key={name}
              name={name}
              numJoints={state.num_joints}
              positions={state.positions}
              endEffectorStatus={state.end_effector_status}
              endEffectorFeedbackValid={state.end_effector_feedback_valid}
              panelWidth={contentWidth}
            />
          ))
        ) : hideEmptyRobotPanel ? null : (
          <RobotStatePanel
            name="robot_0"
            numJoints={0}
            positions={[]}
            endEffectorStatus={undefined}
            endEffectorFeedbackValid={undefined}
            panelWidth={contentWidth}
          />
        )}
      </Box>

      {!isWide && (
        <InfoPanel
          frames={frames}
          robotStates={robotStates}
          streamInfo={streamInfo}
          connected={connected}
          orientation="horizontal"
          panelWidth={width}
        />
      )}
    </Box>
  );
}

function mergePreferredCameraNames(
  preferredCameraNames: readonly string[],
  activeFrameNames: readonly string[],
): string[] {
  const names = [...preferredCameraNames];
  for (const name of activeFrameNames) {
    if (!names.includes(name)) {
      names.push(name);
    }
  }
  return names;
}
