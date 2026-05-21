import type { CSSProperties } from "react";
import { encodeSetupCommand } from "../../lib/protocol";
import {
  buttonStyle,
  ghostButtonStyle,
  modalBackdropStyle,
  modalCardStyle,
  palette,
} from "../styles";

interface AddDevicePickerProps {
  send: (msg: string) => void;
  onClose: () => void;
}

/**
 * Three-button modal mirroring the TUI's add-device picker.
 *
 * Pseudo cameras / robots inject synthetic `rollio-device-pseudo`
 * channels (useful for testing without hardware). "Command device"
 * is a stand-in for a robot that only accepts commands (no
 * publishable state). All three close the modal once the
 * controller acknowledges by mutating the config.
 */
export function AddDevicePicker({ send, onClose }: AddDevicePickerProps) {
  function dispatch(action: "setup_add_pseudo_camera" | "setup_add_pseudo_robot" | "setup_add_command_device") {
    send(encodeSetupCommand(action));
    onClose();
  }
  return (
    <div
      style={modalBackdropStyle}
      role="dialog"
      aria-modal="true"
      onClick={(event) => {
        if (event.target === event.currentTarget) {
          onClose();
        }
      }}
    >
      <div style={{ ...modalCardStyle, width: "min(420px, 92vw)" }}>
        <h2 style={titleStyle}>Add a device</h2>
        <p style={subtitleStyle}>
          Inject a synthetic device for testing or wire up a
          command-only follower.
        </p>
        <div style={listStyle}>
          <PickerButton
            primary="Pseudo camera"
            secondary="Synthetic JPEG stream for testing without hardware"
            onClick={() => dispatch("setup_add_pseudo_camera")}
          />
          <PickerButton
            primary="Pseudo robot"
            secondary="Synthetic arm publishing all state kinds"
            onClick={() => dispatch("setup_add_pseudo_robot")}
          />
          <PickerButton
            primary="Command-only device"
            secondary="Follower that accepts commands but emits no state"
            onClick={() => dispatch("setup_add_command_device")}
          />
        </div>
        <div style={footerStyle}>
          <button type="button" style={ghostButtonStyle} onClick={onClose}>
            Cancel
          </button>
        </div>
      </div>
    </div>
  );
}

function PickerButton({
  primary,
  secondary,
  onClick,
}: {
  primary: string;
  secondary: string;
  onClick: () => void;
}) {
  return (
    <button type="button" style={pickerButtonStyle} onClick={onClick}>
      <span style={pickerPrimaryStyle}>{primary}</span>
      <span style={pickerSecondaryStyle}>{secondary}</span>
    </button>
  );
}

const titleStyle: CSSProperties = {
  margin: 0,
  fontSize: "1.125rem",
  fontWeight: 600,
};

const subtitleStyle: CSSProperties = {
  margin: "0.25rem 0 1rem",
  fontSize: "0.75rem",
  color: palette.textMuted,
};

const listStyle: CSSProperties = {
  display: "flex",
  flexDirection: "column",
  gap: "0.5rem",
};

const pickerButtonStyle: CSSProperties = {
  ...buttonStyle,
  display: "flex",
  flexDirection: "column",
  alignItems: "flex-start",
  textAlign: "left",
  padding: "0.75rem",
  gap: "0.125rem",
};

const pickerPrimaryStyle: CSSProperties = {
  fontWeight: 600,
};

const pickerSecondaryStyle: CSSProperties = {
  fontSize: "0.75rem",
  color: palette.textMuted,
};

const footerStyle: CSSProperties = {
  marginTop: "1rem",
  display: "flex",
  justifyContent: "flex-end",
};
