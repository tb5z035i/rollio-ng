import type { CSSProperties } from "react";
import { CameraGrid } from "../../components/CameraGrid";
import { RobotStatePanel } from "../../components/RobotStatePanel";
import type { SetupStateMessage } from "../../lib/protocol";
import { usePreviewSocket } from "../../lib/websocket";
import { palette, panelStyle, panelTitleStyle } from "../styles";

interface PreviewStepProps {
  setupState: SetupStateMessage;
  previewWebsocketUrl: string;
}

export function PreviewStep({ previewWebsocketUrl }: PreviewStepProps) {
  // Preview step is the only place outside of identify where the
  // visualizer runs (see `should_run_preview_runtime` in
  // controller/src/setup/overview.rs), so we keep the socket open
  // here unconditionally.
  const preview = usePreviewSocket(previewWebsocketUrl, { enabled: true });
  const cameras = Array.from(preview.frames.entries()).map(([name, frame]) => ({
    name,
    frame,
  }));
  const robotChannels = Array.from(preview.robotChannels.values());

  return (
    <div style={layoutStyle}>
      <section style={panelStyle}>
        <h2 style={panelTitleStyle}>Live preview</h2>
        <p style={subtitleStyle}>
          The controller is running the encoder + visualizer pipeline
          against your current config. Confirm each camera and arm look
          right, then save to write the project TOML.
        </p>
        {cameras.length === 0 ? (
          <p style={emptyStyle}>
            Waiting for the first frame… (the preview pipeline starts
            ~1s after this step opens.)
          </p>
        ) : (
          <div style={cameraWrapperStyle}>
            <CameraGrid cameras={cameras} />
          </div>
        )}
      </section>

      {robotChannels.length > 0 ? (
        <section style={panelStyle}>
          <h2 style={panelTitleStyle}>Robot state</h2>
          <div style={robotListStyle}>
            {robotChannels.map((channel) => (
              <RobotStatePanel key={channel.name} channel={channel} />
            ))}
          </div>
        </section>
      ) : null}
    </div>
  );
}

const layoutStyle: CSSProperties = {
  display: "grid",
  gridTemplateColumns: "minmax(0, 2fr) minmax(0, 1fr)",
  gap: "1rem",
  alignItems: "start",
};

const subtitleStyle: CSSProperties = {
  margin: "0.25rem 0 1rem",
  fontSize: "0.8rem",
  color: palette.textMuted,
  maxWidth: "40rem",
};

const emptyStyle: CSSProperties = {
  margin: "1rem 0",
  fontStyle: "italic",
  color: palette.textMuted,
};

const cameraWrapperStyle: CSSProperties = {
  background: "#000",
  borderRadius: "0.375rem",
  overflow: "hidden",
};

const robotListStyle: CSSProperties = {
  display: "flex",
  flexDirection: "column",
  gap: "0.75rem",
};
