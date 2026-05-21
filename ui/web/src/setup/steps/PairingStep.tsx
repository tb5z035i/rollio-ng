import { useMemo, useState, type CSSProperties } from "react";
import {
  encodeSetupCommand,
  type MappingPolicy,
  type SetupAvailableDevice,
  type SetupChannelPairing,
  type SetupStateMessage,
} from "../../lib/protocol";
import { MAPPING_POLICY_OPTS } from "../options";
import {
  buttonStyle,
  dangerButtonStyle,
  ghostButtonStyle,
  inputStyle,
  modalBackdropStyle,
  modalCardStyle,
  palette,
  panelStyle,
  panelTitleStyle,
  primaryButtonStyle,
} from "../styles";

interface PairingStepProps {
  setupState: SetupStateMessage;
  send: (msg: string) => void;
}

interface EndpointOption {
  deviceName: string;
  channelType: string;
  label: string;
}

export function PairingStep({ setupState, send }: PairingStepProps) {
  const [creating, setCreating] = useState(false);
  const robotEndpoints = useMemo(
    () => enumerateRobotEndpoints(setupState.available_devices),
    [setupState.available_devices],
  );

  return (
    <section style={panelStyle}>
      <header style={panelHeaderStyle}>
        <div>
          <h2 style={panelTitleStyle}>Teleop pairings</h2>
          <p style={subtitleStyle}>
            Pair a leader channel (e.g., an operator's arm) with a
            follower channel (the robot to drive). The controller
            enforces driver compatibility — invalid combinations are
            rejected with a footer message.
          </p>
        </div>
        <button
          type="button"
          style={primaryButtonStyle}
          disabled={robotEndpoints.length < 2}
          onClick={() => setCreating(true)}
        >
          + New pair
        </button>
      </header>

      {setupState.config.pairings.length === 0 ? (
        <p style={emptyStyle}>
          No pairings yet. Add at least one robot pair to record
          teleop sessions.
        </p>
      ) : (
        <ul style={listStyle}>
          {setupState.config.pairings.map((pair, index) => (
            <PairingRow
              key={`${index}-${pair.leader_device}-${pair.follower_device}`}
              index={index}
              pair={pair}
              endpointOptions={robotEndpoints}
              send={send}
            />
          ))}
        </ul>
      )}

      {creating ? (
        <NewPairingModal
          endpointOptions={robotEndpoints}
          send={send}
          onClose={() => setCreating(false)}
        />
      ) : null}
    </section>
  );
}

function PairingRow({
  index,
  pair,
  endpointOptions,
  send,
}: {
  index: number;
  pair: SetupChannelPairing;
  endpointOptions: EndpointOption[];
  send: (msg: string) => void;
}) {
  const leaderValue = `${pair.leader_device}|${pair.leader_channel_type}`;
  const followerValue = `${pair.follower_device}|${pair.follower_channel_type}`;
  const isParallel = pair.mapping === "parallel";
  return (
    <li style={rowStyle}>
      <div style={rowHeaderStyle}>
        <div style={pairTitleStyle}>
          Pair #{index + 1}
          <PolicyBadge mapping={pair.mapping} />
        </div>
        <div style={rowActionsStyle}>
          <button
            type="button"
            style={buttonStyle}
            onClick={() =>
              send(
                encodeSetupCommand("setup_cycle_pair_mapping", {
                  index,
                  delta: 1,
                }),
              )
            }
            title="Cycle policy: direct-joint → cartesian → parallel"
          >
            Cycle policy
          </button>
          <button
            type="button"
            style={dangerButtonStyle}
            onClick={() =>
              send(encodeSetupCommand("setup_remove_pairing", { index }))
            }
          >
            Remove
          </button>
        </div>
      </div>
      <div style={pairBodyStyle}>
        <FieldRow label="Leader" hint={`${pair.leader_state}`}>
          <select
            style={{ ...inputStyle, flex: 1 }}
            value={leaderValue}
            onChange={(event) =>
              send(
                encodeSetupCommand("setup_set_pairing_leader", {
                  index,
                  value: event.target.value,
                }),
              )
            }
          >
            {endpointOptions.map((opt) => (
              <option
                key={`leader-${opt.deviceName}-${opt.channelType}`}
                value={`${opt.deviceName}|${opt.channelType}`}
              >
                {opt.label}
              </option>
            ))}
          </select>
        </FieldRow>
        <FieldRow label="Follower" hint={`${pair.follower_command}`}>
          <select
            style={{ ...inputStyle, flex: 1 }}
            value={followerValue}
            onChange={(event) =>
              send(
                encodeSetupCommand("setup_set_pairing_follower", {
                  index,
                  value: event.target.value,
                }),
              )
            }
          >
            {endpointOptions.map((opt) => (
              <option
                key={`follower-${opt.deviceName}-${opt.channelType}`}
                value={`${opt.deviceName}|${opt.channelType}`}
              >
                {opt.label}
              </option>
            ))}
          </select>
        </FieldRow>
        {isParallel ? (
          <FieldRow label="Ratio" hint="finite non-zero number">
            <RatioInput pair={pair} index={index} send={send} />
          </FieldRow>
        ) : null}
        <FieldRow label="Joint map" hint="leader → follower joint indices">
          <code style={codeStyle}>
            [{pair.joint_index_map.join(", ")}] ·{" "}
            scales=[{pair.joint_scales.join(", ")}]
          </code>
        </FieldRow>
      </div>
    </li>
  );
}

function RatioInput({
  pair,
  index,
  send,
}: {
  pair: SetupChannelPairing;
  index: number;
  send: (msg: string) => void;
}) {
  const current = pair.joint_scales[0]?.toString() ?? "1";
  const [draft, setDraft] = useState(current);
  // Re-sync when controller updates the ratio externally.
  if (current !== lastSeenRatio.get(index)) {
    lastSeenRatio.set(index, current);
    if (draft !== current) setDraft(current);
  }
  return (
    <input
      style={{ ...inputStyle, width: "8rem" }}
      type="text"
      value={draft}
      onChange={(event) => setDraft(event.target.value)}
      onBlur={() => {
        if (draft !== current) {
          send(
            encodeSetupCommand("setup_set_pairing_ratio", {
              index,
              value: draft,
            }),
          );
        }
      }}
      onKeyDown={(event) => {
        if (event.key === "Enter") {
          if (draft !== current) {
            send(
              encodeSetupCommand("setup_set_pairing_ratio", {
                index,
                value: draft,
              }),
            );
          }
          (event.target as HTMLInputElement).blur();
        } else if (event.key === "Escape") {
          setDraft(current);
          (event.target as HTMLInputElement).blur();
        }
      }}
    />
  );
}

const lastSeenRatio = new Map<number, string>();

function NewPairingModal({
  endpointOptions,
  send,
  onClose,
}: {
  endpointOptions: EndpointOption[];
  send: (msg: string) => void;
  onClose: () => void;
}) {
  const [policy, setPolicy] = useState<MappingPolicy>("direct-joint");
  const [leader, setLeader] = useState(
    endpointOptions[0]
      ? `${endpointOptions[0].deviceName}|${endpointOptions[0].channelType}`
      : "",
  );
  const [follower, setFollower] = useState(
    endpointOptions[1]
      ? `${endpointOptions[1].deviceName}|${endpointOptions[1].channelType}`
      : "",
  );
  const [ratio, setRatio] = useState("1.0");

  const canSubmit = leader !== "" && follower !== "" && leader !== follower;

  function submit() {
    let value = `${policy};${leader};${follower}`;
    if (policy === "parallel") {
      value += `;ratio=${ratio}`;
    }
    send(encodeSetupCommand("setup_create_pairing", { value }));
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
      <div style={{ ...modalCardStyle, width: "min(540px, 92vw)" }}>
        <h2 style={modalTitleStyle}>New pairing</h2>
        <div style={modalFieldStackStyle}>
          <FieldRow label="Policy" hint={MAPPING_POLICY_OPTS.join("|")}>
            <select
              style={{ ...inputStyle, flex: 1 }}
              value={policy}
              onChange={(event) =>
                setPolicy(event.target.value as MappingPolicy)
              }
            >
              {MAPPING_POLICY_OPTS.map((p) => (
                <option key={p} value={p}>
                  {p}
                </option>
              ))}
            </select>
          </FieldRow>
          <FieldRow label="Leader">
            <select
              style={{ ...inputStyle, flex: 1 }}
              value={leader}
              onChange={(event) => setLeader(event.target.value)}
            >
              {endpointOptions.map((opt) => (
                <option
                  key={`new-leader-${opt.deviceName}-${opt.channelType}`}
                  value={`${opt.deviceName}|${opt.channelType}`}
                >
                  {opt.label}
                </option>
              ))}
            </select>
          </FieldRow>
          <FieldRow label="Follower">
            <select
              style={{ ...inputStyle, flex: 1 }}
              value={follower}
              onChange={(event) => setFollower(event.target.value)}
            >
              {endpointOptions.map((opt) => (
                <option
                  key={`new-follower-${opt.deviceName}-${opt.channelType}`}
                  value={`${opt.deviceName}|${opt.channelType}`}
                >
                  {opt.label}
                </option>
              ))}
            </select>
          </FieldRow>
          {policy === "parallel" ? (
            <FieldRow label="Ratio" hint="finite non-zero number">
              <input
                style={{ ...inputStyle, flex: 1 }}
                type="text"
                value={ratio}
                onChange={(event) => setRatio(event.target.value)}
              />
            </FieldRow>
          ) : null}
        </div>
        <div style={modalFooterStyle}>
          <button type="button" style={ghostButtonStyle} onClick={onClose}>
            Cancel
          </button>
          <button
            type="button"
            style={{
              ...primaryButtonStyle,
              opacity: canSubmit ? 1 : 0.5,
            }}
            disabled={!canSubmit}
            onClick={submit}
          >
            Create
          </button>
        </div>
      </div>
    </div>
  );
}

function PolicyBadge({ mapping }: { mapping: MappingPolicy }) {
  return (
    <span style={policyBadgeStyle(mapping)}>{mapping}</span>
  );
}

function FieldRow({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string;
  children: React.ReactNode;
}) {
  return (
    <div style={fieldRowStyle}>
      <div style={labelStyle}>
        <span>{label}</span>
        {hint ? <span style={hintStyle}>{hint}</span> : null}
      </div>
      <div style={controlStyle}>{children}</div>
    </div>
  );
}

function enumerateRobotEndpoints(
  devices: SetupAvailableDevice[],
): EndpointOption[] {
  const out: EndpointOption[] = [];
  for (const device of devices) {
    if (device.device_type !== "robot") continue;
    for (const channel of device.current.channels) {
      if (channel.kind !== "robot" || channel.enabled === false) continue;
      const channelName = channel.name ?? device.current.name;
      out.push({
        deviceName: device.current.name,
        channelType: channel.channel_type,
        label: `${channelName} (${channel.channel_type}) — ${device.driver}`,
      });
    }
  }
  return out;
}

const panelHeaderStyle: CSSProperties = {
  display: "flex",
  justifyContent: "space-between",
  alignItems: "flex-start",
  gap: "1rem",
  marginBottom: "1rem",
};

const subtitleStyle: CSSProperties = {
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

const listStyle: CSSProperties = {
  margin: 0,
  padding: 0,
  listStyle: "none",
  display: "flex",
  flexDirection: "column",
  gap: "0.75rem",
};

const rowStyle: CSSProperties = {
  border: `1px solid ${palette.border}`,
  borderRadius: "0.5rem",
  padding: "0.75rem 1rem",
};

const rowHeaderStyle: CSSProperties = {
  display: "flex",
  justifyContent: "space-between",
  alignItems: "center",
  marginBottom: "0.5rem",
};

const pairTitleStyle: CSSProperties = {
  display: "flex",
  alignItems: "center",
  gap: "0.5rem",
  fontWeight: 600,
};

const rowActionsStyle: CSSProperties = {
  display: "flex",
  gap: "0.5rem",
};

const pairBodyStyle: CSSProperties = {
  display: "flex",
  flexDirection: "column",
  gap: "0.5rem",
};

const fieldRowStyle: CSSProperties = {
  display: "grid",
  gridTemplateColumns: "minmax(7rem, 10rem) 1fr",
  alignItems: "center",
  gap: "0.75rem",
};

const labelStyle: CSSProperties = {
  display: "flex",
  flexDirection: "column",
  gap: "0.125rem",
};

const hintStyle: CSSProperties = {
  fontSize: "0.7rem",
  color: palette.textMuted,
};

const controlStyle: CSSProperties = {
  display: "flex",
  alignItems: "center",
  gap: "0.5rem",
};

const codeStyle: CSSProperties = {
  fontFamily: "ui-monospace, SFMono-Regular, monospace",
  fontSize: "0.8rem",
  color: palette.textMuted,
};

const modalTitleStyle: CSSProperties = {
  margin: "0 0 1rem 0",
  fontSize: "1.125rem",
  fontWeight: 600,
};

const modalFieldStackStyle: CSSProperties = {
  display: "flex",
  flexDirection: "column",
  gap: "0.75rem",
};

const modalFooterStyle: CSSProperties = {
  marginTop: "1rem",
  display: "flex",
  justifyContent: "flex-end",
  gap: "0.5rem",
};

function policyBadgeStyle(mapping: MappingPolicy): CSSProperties {
  const color =
    mapping === "direct-joint"
      ? palette.accent
      : mapping === "cartesian"
        ? palette.ok
        : palette.warning;
  return {
    fontSize: "0.7rem",
    fontWeight: 500,
    padding: "0.125rem 0.5rem",
    borderRadius: "9999px",
    background: "rgba(255,255,255,0.05)",
    border: `1px solid ${color}`,
    color,
  };
}
