import React, { useEffect, useMemo, useRef } from "react";
import { Box } from "ink";
import { encodeSetPreviewSize, type StreamInfoMessage } from "../lib/protocol.js";
import type { AggregatedRobotChannel, CameraFrame } from "../lib/websocket.js";
import { MAX_PREVIEW_CAMERAS, resolveCameraNames } from "../lib/camera-layout.js";
import { CameraRow, describeCameraPreviewRaster } from "./StreamPanel.js";
import { InfoPanel } from "./InfoPanel.js";
import { RobotStatePanel } from "./RobotStatePanel.js";
import type { AsciiCellGeometry, AsciiRendererId } from "../lib/renderers/index.js";

interface LivePreviewPanelsProps {
  frames: Map<string, CameraFrame>;
  robotChannels: Map<string, AggregatedRobotChannel>;
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
  robotChannels,
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
  const totalCameraPanelHeight = Math.max(
    5,
    contentHeight - robotPanelHeight - (isWide ? 0 : infoPanelHeight),
  );
  // Wrap camera tiles into rows of at most `MAX_PREVIEW_CAMERAS` so the
  // operator sees every configured stream — overflow lands on a second
  // row instead of being silently dropped. The per-row height shrinks
  // proportionally so the cluster fits within the existing camera panel
  // height; the per-row width budget improves because each row only
  // needs to fit `min(N_remaining, MAX_PREVIEW_CAMERAS)` tiles.
  const cameraRowCount = Math.max(1, Math.ceil(cameraNames.length / MAX_PREVIEW_CAMERAS));
  const perRowCameraPanelHeight = Math.max(
    5,
    Math.floor(totalCameraPanelHeight / cameraRowCount),
  );
  const tilesPerRow = Math.min(
    Math.max(cameraNames.length, 1),
    MAX_PREVIEW_CAMERAS,
  );
  const cameraPreviewRaster = useMemo(
    () =>
      describeCameraPreviewRaster(
        contentWidth,
        perRowCameraPanelHeight,
        tilesPerRow,
        cellGeometry,
        rendererId,
      ),
    [cellGeometry, contentWidth, perRowCameraPanelHeight, rendererId, tilesPerRow],
  );
  const cameraData = useMemo(
    () =>
      cameraNames.map((name) => ({
        name,
        frame: frames.get(name),
      })),
    [cameraNames, frames],
  );
  const cameraRows = useMemo(() => {
    const rows: typeof cameraData[] = [];
    for (let i = 0; i < cameraData.length; i += MAX_PREVIEW_CAMERAS) {
      rows.push(cameraData.slice(i, i + MAX_PREVIEW_CAMERAS));
    }
    return rows;
  }, [cameraData]);
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
    for (const [name, channel] of robotChannels) {
      const dof = inferDofForInfoLine(channel);
      lines.push(pad(`  ${name}  ${dof} DoF`) + "│");
    }
    lines.push(pad("") + "│");
    lines.push(pad(` WS: ${connected ? "Connected" : "Disconnected"}`) + "│");

    const totalRows = totalCameraPanelHeight + 2;
    while (lines.length < totalRows - 1) {
      lines.push(pad("") + "│");
    }
    lines.push(`${"─".repeat(panelWidth - 1)}┘`);
    return lines;
  }, [
    cameraInfoByName,
    cameraNames,
    totalCameraPanelHeight,
    connected,
    frames,
    infoPanelWidth,
    isWide,
    robotChannels,
  ]);
  const robotEntries = Array.from(robotChannels.entries());

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
      {cameraRows.map((row, rowIndex) => (
        <CameraRow
          key={`row-${rowIndex}`}
          cameras={row}
          previewRaster={cameraPreviewRaster}
          cellGeometry={cellGeometry}
          rendererId={rendererId}
          infoPanelLines={rowIndex === 0 ? infoPanelLines : undefined}
          hasRightPanel={isWide && rowIndex === 0}
        />
      ))}

      <Box flexDirection="column">
        {robotEntries.length > 0 ? (
          robotEntries.map(([name, channel]) => (
            <RobotStatePanel
              key={name}
              channel={channel}
              panelWidth={contentWidth}
            />
          ))
        ) : hideEmptyRobotPanel ? null : (
          <RobotStatePanel
            channel={{
              name: "robot_0",
              states: {},
              lastTimestampMs: 0,
            }}
            panelWidth={contentWidth}
          />
        )}
      </Box>

      {!isWide && (
        <InfoPanel
          frames={frames}
          robotChannels={robotChannels}
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

function inferDofForInfoLine(channel: AggregatedRobotChannel): number {
  const sample =
    channel.states.joint_position ??
    channel.states.parallel_position ??
    channel.states.end_effector_pose;
  if (sample) {
    return sample.numJoints || sample.values.length;
  }
  for (const value of Object.values(channel.states)) {
    if (value) {
      return value.numJoints || value.values.length;
    }
  }
  return 0;
}
