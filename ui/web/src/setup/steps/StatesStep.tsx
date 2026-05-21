import type { CSSProperties } from "react";
import {
  encodeSetupCommand,
  type SetupAvailableDevice,
  type SetupStateMessage,
} from "../../lib/protocol";
import { palette, panelStyle, panelTitleStyle } from "../styles";

interface StatesStepProps {
  setupState: SetupStateMessage;
  send: (msg: string) => void;
}

interface RobotChannelView {
  device: SetupAvailableDevice;
  channelName: string;
  channelType: string;
  supportedStates: string[];
  publishStates: string[];
  recordedStates: string[];
}

export function StatesStep({ setupState, send }: StatesStepProps) {
  const channels = buildRobotChannelViews(setupState);

  return (
    <section style={panelStyle}>
      <h2 style={panelTitleStyle}>State kinds</h2>
      <p style={subtitleStyle}>
        For each robot channel, pick which state kinds the wizard
        publishes over iceoryx2 and which ones the assembler records
        into the dataset. Recording requires publishing — the
        controller validates this on every save.
      </p>
      {channels.length === 0 ? (
        <p style={emptyStyle}>
          No enabled robot channels yet — enable a robot on the
          Devices step first.
        </p>
      ) : (
        <div style={listStyle}>
          {channels.map((view) => (
            <ChannelStateTable
              key={`${view.device.name}/${view.channelType}`}
              view={view}
              send={send}
            />
          ))}
        </div>
      )}
    </section>
  );
}

function ChannelStateTable({
  view,
  send,
}: {
  view: RobotChannelView;
  send: (msg: string) => void;
}) {
  return (
    <div style={channelBoxStyle}>
      <header style={channelHeaderStyle}>
        <div>
          <div style={channelTitleStyle}>{view.channelName}</div>
          <div style={channelSubtitleStyle}>
            {view.device.display_name} · {view.channelType}
          </div>
        </div>
      </header>
      <table style={tableStyle}>
        <thead>
          <tr>
            <th style={tableHeaderCell}>State kind</th>
            <th style={tableHeaderCell}>Publish</th>
            <th style={tableHeaderCell}>Record</th>
          </tr>
        </thead>
        <tbody>
          {view.supportedStates.map((kind) => (
            <tr key={kind}>
              <td style={tableBodyCell}>
                <code style={codeStyle}>{kind}</code>
              </td>
              <td style={tableBodyCell}>
                <input
                  type="checkbox"
                  checked={view.publishStates.includes(kind)}
                  onChange={() =>
                    send(
                      encodeSetupCommand("setup_toggle_publish_state", {
                        name: view.channelName,
                        value: kind,
                      }),
                    )
                  }
                />
              </td>
              <td style={tableBodyCell}>
                <input
                  type="checkbox"
                  checked={view.recordedStates.includes(kind)}
                  disabled={!view.publishStates.includes(kind)}
                  title={
                    view.publishStates.includes(kind)
                      ? undefined
                      : "Enable publish first — recording requires publishing"
                  }
                  onChange={() =>
                    send(
                      encodeSetupCommand("setup_toggle_recorded_state", {
                        name: view.channelName,
                        value: kind,
                      }),
                    )
                  }
                />
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function buildRobotChannelViews(
  setupState: SetupStateMessage,
): RobotChannelView[] {
  const out: RobotChannelView[] = [];
  for (const available of setupState.available_devices) {
    if (available.device_type !== "robot") continue;
    const currentChannel = available.current.channels[0];
    if (!currentChannel || currentChannel.enabled === false) continue;
    out.push({
      device: available,
      channelName: currentChannel.name ?? available.current.name,
      channelType: currentChannel.channel_type,
      supportedStates: available.supported_states ?? [],
      publishStates: currentChannel.publish_states ?? [],
      recordedStates: currentChannel.recorded_states ?? [],
    });
  }
  return out;
}

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

const listStyle: CSSProperties = {
  display: "flex",
  flexDirection: "column",
  gap: "1rem",
};

const channelBoxStyle: CSSProperties = {
  border: `1px solid ${palette.border}`,
  borderRadius: "0.5rem",
  overflow: "hidden",
};

const channelHeaderStyle: CSSProperties = {
  display: "flex",
  justifyContent: "space-between",
  alignItems: "center",
  padding: "0.75rem 1rem",
  background: palette.surfaceMuted,
};

const channelTitleStyle: CSSProperties = {
  fontWeight: 600,
};

const channelSubtitleStyle: CSSProperties = {
  fontSize: "0.75rem",
  color: palette.textMuted,
};

const tableStyle: CSSProperties = {
  width: "100%",
  borderCollapse: "collapse",
};

const tableHeaderCell: CSSProperties = {
  textAlign: "left",
  padding: "0.5rem 1rem",
  fontWeight: 500,
  color: palette.textMuted,
  fontSize: "0.75rem",
  textTransform: "uppercase",
  letterSpacing: "0.04em",
  borderTop: `1px solid ${palette.border}`,
  borderBottom: `1px solid ${palette.border}`,
};

const tableBodyCell: CSSProperties = {
  padding: "0.5rem 1rem",
  borderBottom: `1px solid ${palette.border}`,
};

const codeStyle: CSSProperties = {
  fontFamily: "ui-monospace, SFMono-Regular, monospace",
  fontSize: "0.8rem",
  color: palette.text,
};
