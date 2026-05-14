import type { CameraFrame, AggregatedRobotChannel } from "../lib/websocket";
import { codecName, type StreamInfoMessage } from "../lib/protocol";

interface InfoPanelProps {
  frames: Map<string, CameraFrame>;
  robotChannels: Map<string, AggregatedRobotChannel>;
  streamInfo?: StreamInfoMessage | null;
  connected: boolean;
  orientation: "vertical" | "horizontal";
}

export function InfoPanel({
  frames,
  robotChannels,
  streamInfo = null,
  connected,
  orientation,
}: InfoPanelProps) {
  const cameraNames =
    streamInfo?.cameras?.map((camera) => camera.name) ?? Array.from(frames.keys());
  const robotNames = streamInfo?.robots ?? Array.from(robotChannels.keys());
  const hasData =
    cameraNames.length > 0 ||
    robotNames.length > 0 ||
    frames.size > 0 ||
    robotChannels.size > 0;

  if (!hasData) {
    return (
      <section className="panel">
        <header className="panel__header">Info</header>
        <div className="panel__empty">No devices connected</div>
      </section>
    );
  }

  if (orientation === "horizontal") {
    const cameraLine = cameraNames
      .map((name) => `${name}: ${cameraResolution(name, frames.get(name), streamInfo)}`)
      .join(" | ");
    const robotLine = robotNames
      .map((name) => `${name}: ${robotDof(robotChannels.get(name))} DoF`)
      .join(" | ");

    return (
      <section className="panel">
        <header className="panel__header">Info</header>
        <div className="info-panel info-panel--horizontal">
          <div className="info-panel__line">{cameraLine || "No cameras"}</div>
          <div className="info-panel__line">
            {robotLine || "No robots"} | WS: {connected ? "Connected" : "Disconnected"}
          </div>
        </div>
      </section>
    );
  }

  return (
    <section className="panel">
      <header className="panel__header">Info</header>
      <div className="info-panel">
        <div className="info-panel__section">
          <div className="info-panel__heading">Devices</div>
          {cameraNames.map((name) => (
            <div className="info-panel__row" key={name}>
              <span>{name}</span>
              <span>{cameraResolution(name, frames.get(name), streamInfo)}</span>
            </div>
          ))}
          {robotNames.map((name) => (
            <div className="info-panel__row" key={name}>
              <span>{name}</span>
              <span>{robotDof(robotChannels.get(name))} DoF</span>
            </div>
          ))}
        </div>
        <div className="info-panel__section">
          <div className="info-panel__row">
            <span>WS</span>
            <span>{connected ? "Connected" : "Disconnected"}</span>
          </div>
          {streamInfo ? (
            <div className="info-panel__row">
              <span>Preview</span>
              <span>
                {previewCodecLabel(streamInfo, frames)} · {previewResolutionLabel(streamInfo)}
              </span>
            </div>
          ) : null}
        </div>
      </div>
    </section>
  );
}

function previewResolutionLabel(streamInfo: StreamInfoMessage): string {
  const fixedCount = streamInfo.cameras.filter(
    (camera) => camera.preview_resizable === false,
  ).length;
  const dynamicLabel = `${streamInfo.active_preview_width}x${streamInfo.active_preview_height}`;
  return fixedCount > 0 ? `${dynamicLabel} + ${fixedCount} native` : dynamicLabel;
}

function previewCodecLabel(
  streamInfo: StreamInfoMessage,
  frames: Map<string, CameraFrame>,
): string {
  if (streamInfo.preview_output_mode === "jpeg") {
    return "jpeg";
  }
  for (const frame of frames.values()) {
    if (frame.kind === "video") {
      return codecName(frame.codecId);
    }
  }
  return "encoded";
}

function cameraResolution(
  name: string,
  frame: CameraFrame | undefined,
  streamInfo: StreamInfoMessage | null,
): string {
  const camera = streamInfo?.cameras?.find((entry) => entry.name === name);
  if (camera?.source_width != null && camera.source_height != null) {
    return `${camera.source_width}x${camera.source_height}`;
  }
  if (frame) {
    if (frame.kind === "jpeg") {
      return `${frame.previewWidth}x${frame.previewHeight}`;
    }
    return `${frame.width}x${frame.height}`;
  }
  return "n/a";
}

function robotDof(channel: AggregatedRobotChannel | undefined): number {
  if (!channel) return 0;
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
