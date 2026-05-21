import { useMemo, useState, type CSSProperties } from "react";
import { CameraGrid } from "../../components/CameraGrid";
import { RobotStatePanel } from "../../components/RobotStatePanel";
import {
  encodeSetupCommand,
  type SetupAvailableDevice,
  type SetupStateMessage,
} from "../../lib/protocol";
import {
  usePreviewSocket,
  type PreviewSocketState,
} from "../../lib/websocket";
import { AddDevicePicker } from "../components/AddDevicePicker";
import { SubpanelModal } from "../components/SubpanelModal";
import {
  buttonStyle,
  ghostButtonStyle,
  inputStyle,
  palette,
  panelStyle,
  panelTitleStyle,
  primaryButtonStyle,
  tableBodyCell,
  tableHeaderCell,
  tableStyle,
} from "../styles";

interface DevicesStepProps {
  setupState: SetupStateMessage;
  send: (msg: string) => void;
  previewWebsocketUrl: string;
}

export function DevicesStep({
  setupState,
  send,
  previewWebsocketUrl,
}: DevicesStepProps) {
  const [addPickerOpen, setAddPickerOpen] = useState(false);
  const subpanelTarget = setupState.subpanel_target ?? null;

  // Open the preview socket only while identify is active. The
  // gateway returns a clean close when its preview upstream is
  // absent (see web-gateway/src/main.rs:165), but skipping the
  // connection entirely keeps the gateway logs quieter and the
  // browser dev tools cleaner.
  const previewActive = setupState.identify_device != null;
  const preview = usePreviewSocket(previewWebsocketUrl, {
    enabled: previewActive,
  });

  const subpanelDevice = useMemo(() => {
    if (!subpanelTarget) return null;
    return findDeviceByName(setupState.available_devices, subpanelTarget);
  }, [subpanelTarget, setupState.available_devices]);

  return (
    <div style={layoutStyle(previewActive)}>
      <section style={panelStyle}>
        <header style={panelHeaderStyle}>
          <div>
            <h2 style={panelTitleStyle}>Devices</h2>
            <p style={panelSubtitleStyle}>
              Toggle which devices participate in this project. Use
              identify to confirm which physical camera or arm a row
              points to.
            </p>
          </div>
          <button
            type="button"
            style={primaryButtonStyle}
            onClick={() => setAddPickerOpen(true)}
          >
            + Add device
          </button>
        </header>

        {setupState.available_devices.length === 0 ? (
          <p style={emptyStyle}>
            No devices discovered. Add a pseudo camera or robot to get
            started.
          </p>
        ) : (
          <table style={tableStyle}>
            <thead>
              <tr>
                <th style={tableHeaderCell}>On</th>
                <th style={tableHeaderCell}>Name</th>
                <th style={tableHeaderCell}>Kind</th>
                <th style={tableHeaderCell}>Profile / Mode</th>
                <th style={tableHeaderCell}>Identify</th>
                <th style={tableHeaderCell}></th>
              </tr>
            </thead>
            <tbody>
              {setupState.available_devices.map((device) => (
                <DeviceRow
                  key={device.name}
                  device={device}
                  identifying={setupState.identify_device === device.name}
                  send={send}
                />
              ))}
            </tbody>
          </table>
        )}
      </section>

      {previewActive ? (
        <IdentifyPreviewPanel
          identifyDeviceName={setupState.identify_device!}
          preview={preview}
        />
      ) : null}

      {subpanelDevice ? (
        <SubpanelModal
          device={subpanelDevice}
          send={send}
          onClose={() => send(encodeSetupCommand("setup_close_subpanel"))}
        />
      ) : null}

      {addPickerOpen ? (
        <AddDevicePicker send={send} onClose={() => setAddPickerOpen(false)} />
      ) : null}
    </div>
  );
}

interface DeviceRowProps {
  device: SetupAvailableDevice;
  identifying: boolean;
  send: (msg: string) => void;
}

function DeviceRow({ device, identifying, send }: DeviceRowProps) {
  const channel = device.current.channels[0];
  const enabled = channel?.enabled !== false;
  const isCamera = device.device_type === "camera";

  return (
    <tr style={{ opacity: enabled ? 1 : 0.5 }}>
      <td style={tableBodyCell}>
        <input
          type="checkbox"
          checked={enabled}
          onChange={() =>
            send(
              encodeSetupCommand("setup_toggle_device", {
                name: device.name,
              }),
            )
          }
        />
      </td>
      <td style={tableBodyCell}>
        <DeviceNameEditor device={device} send={send} />
        <div style={driverHintStyle}>{device.driver}</div>
      </td>
      <td style={tableBodyCell}>
        <span style={kindBadgeStyle(device.device_type)}>
          {device.device_type}
        </span>
      </td>
      <td style={tableBodyCell}>
        {isCamera ? (
          <CycleControl
            label={profileLabel(device)}
            onPrev={() =>
              send(
                encodeSetupCommand("setup_cycle_camera_profile", {
                  name: device.name,
                  delta: -1,
                }),
              )
            }
            onNext={() =>
              send(
                encodeSetupCommand("setup_cycle_camera_profile", {
                  name: device.name,
                  delta: 1,
                }),
              )
            }
            disabled={!enabled}
          />
        ) : (
          <CycleControl
            label={channel?.mode ?? "?"}
            onPrev={() =>
              send(
                encodeSetupCommand("setup_cycle_robot_mode", {
                  name: device.name,
                  delta: -1,
                }),
              )
            }
            onNext={() =>
              send(
                encodeSetupCommand("setup_cycle_robot_mode", {
                  name: device.name,
                  delta: 1,
                }),
              )
            }
            disabled={!enabled}
          />
        )}
      </td>
      <td style={tableBodyCell}>
        <input
          type="checkbox"
          checked={identifying}
          disabled={!enabled}
          onChange={() =>
            send(
              encodeSetupCommand("setup_toggle_identify", {
                name: device.name,
              }),
            )
          }
        />
      </td>
      <td style={tableBodyCell}>
        <button
          type="button"
          style={ghostButtonStyle}
          disabled={!enabled}
          onClick={() =>
            send(
              encodeSetupCommand("setup_open_subpanel", {
                name: device.name,
              }),
            )
          }
        >
          Configure…
        </button>
      </td>
    </tr>
  );
}

function DeviceNameEditor({
  device,
  send,
}: {
  device: SetupAvailableDevice;
  send: (msg: string) => void;
}) {
  const channel = device.current.channels[0];
  const currentName = channel?.name ?? device.current.name;
  const [draft, setDraft] = useState(currentName);
  // Re-sync when controller-side mutations land.
  if (currentName !== lastSeenName.get(device.name)) {
    lastSeenName.set(device.name, currentName);
    if (draft !== currentName) {
      setDraft(currentName);
    }
  }
  return (
    <input
      style={{
        ...inputStyle,
        width: "100%",
        fontWeight: 500,
      }}
      type="text"
      value={draft}
      onChange={(event) => setDraft(event.target.value)}
      onBlur={() => {
        if (draft !== currentName && draft.trim() !== "") {
          send(
            encodeSetupCommand("setup_set_device_name", {
              name: device.name,
              value: draft.trim(),
            }),
          );
        } else if (draft.trim() === "") {
          setDraft(currentName);
        }
      }}
      onKeyDown={(event) => {
        if (event.key === "Enter") {
          (event.target as HTMLInputElement).blur();
        } else if (event.key === "Escape") {
          setDraft(currentName);
          (event.target as HTMLInputElement).blur();
        }
      }}
    />
  );
}

// Module-level cache: track the last name we observed per device so the
// DeviceNameEditor input doesn't fight the controller-side echo.
const lastSeenName = new Map<string, string>();

function CycleControl({
  label,
  onPrev,
  onNext,
  disabled,
}: {
  label: string;
  onPrev: () => void;
  onNext: () => void;
  disabled?: boolean;
}) {
  return (
    <div style={cycleControlStyle}>
      <button
        type="button"
        style={buttonStyle}
        onClick={onPrev}
        disabled={disabled}
      >
        ‹
      </button>
      <span style={cycleLabelStyle}>{label}</span>
      <button
        type="button"
        style={buttonStyle}
        onClick={onNext}
        disabled={disabled}
      >
        ›
      </button>
    </div>
  );
}

function IdentifyPreviewPanel({
  identifyDeviceName,
  preview,
}: {
  identifyDeviceName: string;
  preview: PreviewSocketState;
}) {
  const cameras = Array.from(preview.frames.entries()).map(([name, frame]) => ({
    name,
    frame,
  }));
  const robotChannelsList = Array.from(preview.robotChannels.values());

  return (
    <aside style={panelStyle}>
      <h2 style={panelTitleStyle}>Identify preview</h2>
      <p style={panelSubtitleStyle}>
        Live feed from <code style={codeStyle}>{identifyDeviceName}</code>.
        Watch the camera move or the arm jog to confirm the row points
        at the physical device you expect.
      </p>
      {preview.connected ? null : (
        <p style={emptyStyle}>Waiting for the preview pipeline to come up…</p>
      )}
      {cameras.length > 0 ? (
        <div style={cameraGridWrapperStyle}>
          <CameraGrid cameras={cameras} />
        </div>
      ) : null}
      {robotChannelsList.map((channel) => (
        <RobotStatePanel key={channel.name} channel={channel} />
      ))}
    </aside>
  );
}

function profileLabel(device: SetupAvailableDevice): string {
  const channel = device.current.channels[0];
  if (!channel) return "(no channel)";
  const profile = channel.profile;
  if (!profile) return "(none)";
  return `${profile.width}x${profile.height} @ ${profile.fps}fps`;
}

function findDeviceByName(
  devices: SetupAvailableDevice[],
  name: string,
): SetupAvailableDevice | null {
  return devices.find((d) => d.name === name) ?? null;
}

const panelHeaderStyle: CSSProperties = {
  display: "flex",
  justifyContent: "space-between",
  alignItems: "flex-start",
  gap: "1rem",
  marginBottom: "1rem",
};

const panelSubtitleStyle: CSSProperties = {
  margin: "0.25rem 0 0",
  fontSize: "0.8rem",
  color: palette.textMuted,
  maxWidth: "40rem",
};

const emptyStyle: CSSProperties = {
  margin: "1rem 0",
  fontStyle: "italic",
  color: palette.textMuted,
};

const driverHintStyle: CSSProperties = {
  marginTop: "0.25rem",
  fontSize: "0.7rem",
  color: palette.textMuted,
};

function kindBadgeStyle(kind: "camera" | "robot"): CSSProperties {
  const isCamera = kind === "camera";
  return {
    display: "inline-block",
    padding: "0.125rem 0.5rem",
    borderRadius: "9999px",
    fontSize: "0.7rem",
    fontWeight: 600,
    background: isCamera ? "rgba(34,197,94,0.15)" : "rgba(59,130,246,0.15)",
    color: isCamera ? palette.ok : palette.accent,
    border: `1px solid ${isCamera ? palette.ok : palette.accent}`,
  };
}

const cycleControlStyle: CSSProperties = {
  display: "inline-flex",
  alignItems: "center",
  gap: "0.5rem",
};

const cycleLabelStyle: CSSProperties = {
  minWidth: "10rem",
  textAlign: "center",
  fontFamily: "ui-monospace, SFMono-Regular, monospace",
  fontSize: "0.8rem",
};

const codeStyle: CSSProperties = {
  background: palette.surfaceMuted,
  padding: "0 0.25rem",
  borderRadius: "0.25rem",
  fontSize: "0.8rem",
};

const cameraGridWrapperStyle: CSSProperties = {
  marginTop: "0.5rem",
  background: "#000",
  borderRadius: "0.375rem",
  overflow: "hidden",
  maxWidth: "100%",
};

function layoutStyle(twoColumn: boolean): CSSProperties {
  return {
    display: "grid",
    gridTemplateColumns: twoColumn ? "minmax(0, 1.5fr) minmax(0, 1fr)" : "1fr",
    gap: "1rem",
    alignItems: "start",
  };
}
