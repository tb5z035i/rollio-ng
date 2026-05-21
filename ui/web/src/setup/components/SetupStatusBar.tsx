import type { CSSProperties } from "react";
import { encodeSetupCommand, type SetupStateMessage } from "../../lib/protocol";
import {
  dangerButtonStyle,
  palette,
  primaryButtonStyle,
  statusBarStyle,
} from "../styles";

interface SetupStatusBarProps {
  setupState: SetupStateMessage;
  send: (msg: string) => void;
  connected: boolean;
}

export function SetupStatusBar({
  setupState,
  send,
  connected,
}: SetupStatusBarProps) {
  const finalStep = setupState.step_index + 1 >= setupState.total_steps;
  return (
    <footer style={statusBarStyle}>
      <div style={leftStyle}>
        <ConnectionDot connected={connected} />
        <span style={pathStyle}>{setupState.output_path}</span>
        {setupState.message ? (
          <span style={messageStyle}>{setupState.message}</span>
        ) : null}
      </div>
      <div style={{ display: "flex", gap: "0.5rem" }}>
        <button
          type="button"
          style={{ ...primaryButtonStyle, opacity: connected ? 1 : 0.5 }}
          disabled={!connected}
          onClick={() => send(encodeSetupCommand("setup_save"))}
        >
          {finalStep ? "Save and exit" : "Save now"}
        </button>
        <button
          type="button"
          style={dangerButtonStyle}
          disabled={!connected}
          onClick={() => send(encodeSetupCommand("setup_cancel"))}
        >
          Cancel
        </button>
      </div>
    </footer>
  );
}

function ConnectionDot({ connected }: { connected: boolean }) {
  return (
    <span style={dotStyle(connected)}>
      {connected ? "● controller connected" : "○ controller disconnected"}
    </span>
  );
}

const leftStyle: CSSProperties = {
  display: "flex",
  alignItems: "center",
  gap: "1rem",
  flexWrap: "wrap",
};

const pathStyle: CSSProperties = {
  fontFamily: "ui-monospace, SFMono-Regular, monospace",
  fontSize: "0.75rem",
};

const messageStyle: CSSProperties = {
  color: palette.warning,
  maxWidth: "32rem",
  whiteSpace: "nowrap",
  overflow: "hidden",
  textOverflow: "ellipsis",
};

function dotStyle(connected: boolean): CSSProperties {
  return {
    color: connected ? palette.ok : palette.danger,
    fontSize: "0.75rem",
  };
}
