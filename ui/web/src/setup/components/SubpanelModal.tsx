import { useEffect, useState, type CSSProperties } from "react";
import {
  encodeSetupCommand,
  type SetupAvailableDevice,
  type SetupCommandAction,
  type SetupDeviceChannelV2,
} from "../../lib/protocol";
import {
  PREVIEW_DEFAULTS,
  PREVIEW_OUTPUT_MODE_OPTS,
  RECORD_BACKEND_OPTS,
  RECORD_BIT_DEPTH_OPTS,
  RECORD_CHROMA_OPTS,
  RECORD_COLOR_SPACE_OPTS,
  RECORD_DEFAULTS,
  RECORD_DEPTH_CODEC_OPTS,
  RECORD_PRESET_OPTS,
  RECORD_VIDEO_CODEC_OPTS,
  ROBOT_MODE_OPTS,
  fmtBool,
  fmtOpt,
} from "../options";
import {
  buttonStyle,
  ghostButtonStyle,
  inputStyle,
  modalBackdropStyle,
  modalCardStyle,
  palette,
} from "../styles";

interface SubpanelModalProps {
  device: SetupAvailableDevice;
  send: (msg: string) => void;
  onClose: () => void;
}

export function SubpanelModal({ device, send, onClose }: SubpanelModalProps) {
  const channel = device.current.channels[0];
  if (!channel) {
    return null;
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
      <div style={modalCardStyle}>
        <header style={headerStyle}>
          <div>
            <h2 style={titleStyle}>
              {device.current.name}{" "}
              <span style={mutedStyle}>({channel.channel_type})</span>
            </h2>
            <p style={subtitleStyle}>
              {channel.kind === "camera" ? "Camera channel" : "Robot channel"} —{" "}
              {device.display_name}
            </p>
          </div>
          <button type="button" style={ghostButtonStyle} onClick={onClose}>
            Close
          </button>
        </header>

        <FieldGroup title="Channel">
          <TextField
            label="Name"
            value={channel.name ?? device.current.name}
            placeholder={channel.channel_type}
            onCommit={(value) =>
              send(
                encodeSetupCommand("setup_set_device_name", {
                  name: device.current.name,
                  value,
                }),
              )
            }
            hint="unique non-empty string"
          />
          <ReadonlyField
            label="Display label"
            value={channel.channel_label ?? device.display_name}
            hint="from driver query --json"
          />
          <ReadonlyField label="Kind" value={channel.kind} hint="camera|robot" />
          <CycleField
            label="Preview enabled"
            value={fmtBool(channel.preview_enabled)}
            action="setup_subpanel_toggle_preview_enabled"
            channelName={channel.name ?? device.current.name}
            send={send}
            hint="on|off"
          />
          <CycleField
            label="Record enabled"
            value={fmtBool(channel.record_enabled)}
            action="setup_subpanel_toggle_record_enabled"
            channelName={channel.name ?? device.current.name}
            send={send}
            hint="on|off"
          />
        </FieldGroup>

        {channel.kind === "camera" ? (
          <CameraSubpanelGroups
            device={device}
            channel={channel}
            send={send}
          />
        ) : null}
        {channel.kind === "robot" ? (
          <RobotSubpanelGroups
            device={device}
            channel={channel}
            send={send}
          />
        ) : null}
      </div>
    </div>
  );
}

function CameraSubpanelGroups({
  device,
  channel,
  send,
}: {
  device: SetupAvailableDevice;
  channel: SetupDeviceChannelV2;
  send: (msg: string) => void;
}) {
  const channelName = channel.name ?? device.current.name;
  const record = channel.record ?? {};
  const preview = channel.preview_config ?? {};
  const profile = channel.profile;
  return (
    <>
      <FieldGroup title="Profile">
        <CycleField
          label="Resolution"
          value={
            profile
              ? `${profile.width}x${profile.height} @ ${profile.fps}fps ${profile.pixel_format}`
              : "(none)"
          }
          action="setup_subpanel_cycle_primary"
          channelName={channelName}
          send={send}
          hint="driver-advertised profiles"
        />
        <ReadonlyField
          label="Native pixel format"
          value={profile?.native_pixel_format ?? "(driver picks)"}
          hint="v4l2 fourcc"
        />
      </FieldGroup>

      <FieldGroup title="Record encoder">
        <RecordCycleField
          label="Video codec"
          field="video_codec"
          value={fmtOpt(record.video_codec, RECORD_DEFAULTS.video_codec)}
          options={RECORD_VIDEO_CODEC_OPTS}
          channelName={channelName}
          send={send}
        />
        <ReadonlyField
          label="Depth codec"
          value={fmtOpt(record.depth_codec, RECORD_DEFAULTS.depth_codec)}
          hint={RECORD_DEPTH_CODEC_OPTS.join("|") + " (only)"}
        />
        <RecordCycleField
          label="Video backend"
          field="video_backend"
          value={fmtOpt(
            record.video_backend ?? record.backend,
            RECORD_DEFAULTS.video_backend,
          )}
          options={RECORD_BACKEND_OPTS}
          channelName={channelName}
          send={send}
        />
        <RecordCycleField
          label="Depth backend"
          field="depth_backend"
          value={fmtOpt(
            record.depth_backend ?? record.backend,
            RECORD_DEFAULTS.depth_backend,
          )}
          options={RECORD_BACKEND_OPTS}
          channelName={channelName}
          send={send}
        />
        <RecordTextField
          label="CRF"
          field="crf"
          value={record.crf != null ? String(record.crf) : ""}
          channelName={channelName}
          send={send}
          hint="0..=51"
        />
        <RecordCycleField
          label="Preset"
          field="preset"
          value={fmtOpt(record.preset, "default")}
          options={RECORD_PRESET_OPTS}
          channelName={channelName}
          send={send}
        />
        <RecordTextField
          label="Tune"
          field="tune"
          value={record.tune ?? ""}
          channelName={channelName}
          send={send}
          hint="x264/x265 tune string"
        />
        <RecordCycleField
          label="Bit depth"
          field="bit_depth"
          value={fmtOpt(record.bit_depth, RECORD_DEFAULTS.bit_depth)}
          options={RECORD_BIT_DEPTH_OPTS.map(String)}
          channelName={channelName}
          send={send}
        />
        <RecordCycleField
          label="Chroma subsampling"
          field="chroma_subsampling"
          value={fmtOpt(
            record.chroma_subsampling,
            RECORD_DEFAULTS.chroma_subsampling,
          )}
          options={RECORD_CHROMA_OPTS}
          channelName={channelName}
          send={send}
        />
        <RecordCycleField
          label="Color space"
          field="color_space"
          value={fmtOpt(record.color_space, RECORD_DEFAULTS.color_space)}
          options={RECORD_COLOR_SPACE_OPTS}
          channelName={channelName}
          send={send}
        />
        <RecordTextField
          label="Queue size"
          field="queue_size"
          value={record.queue_size != null ? String(record.queue_size) : ""}
          channelName={channelName}
          send={send}
          hint=">0"
        />
      </FieldGroup>

      <FieldGroup title="Preview encoder">
        <PreviewCycleField
          label="Output mode"
          field="output_mode"
          value={fmtOpt(preview.output_mode, PREVIEW_DEFAULTS.output_mode)}
          options={PREVIEW_OUTPUT_MODE_OPTS}
          channelName={channelName}
          send={send}
        />
        <PreviewCycleField
          label="Color codec"
          field="color_codec"
          value={fmtOpt(preview.color_codec, PREVIEW_DEFAULTS.color_codec)}
          options={RECORD_VIDEO_CODEC_OPTS}
          channelName={channelName}
          send={send}
        />
        <ReadonlyField
          label="Depth codec"
          value={fmtOpt(preview.depth_codec, PREVIEW_DEFAULTS.depth_codec)}
          hint="rvl (only)"
        />
        <PreviewCycleField
          label="Backend"
          field="backend"
          value={fmtOpt(preview.backend, PREVIEW_DEFAULTS.backend)}
          options={RECORD_BACKEND_OPTS}
          channelName={channelName}
          send={send}
        />
        <PreviewTextField
          label="Width"
          field="width"
          value={preview.width != null ? String(preview.width) : ""}
          channelName={channelName}
          send={send}
          hint=">0, h264 needs >=160 multiple of 16"
        />
        <PreviewTextField
          label="Height"
          field="height"
          value={preview.height != null ? String(preview.height) : ""}
          channelName={channelName}
          send={send}
          hint=">0, h264 needs >=160 multiple of 16"
        />
        <PreviewTextField
          label="FPS"
          field="fps"
          value={preview.fps != null ? String(preview.fps) : ""}
          channelName={channelName}
          send={send}
          hint="1..=1000"
        />
        <PreviewTextField
          label="GOP seconds"
          field="gop_seconds"
          value={
            preview.gop_seconds != null ? String(preview.gop_seconds) : ""
          }
          channelName={channelName}
          send={send}
          hint=">0"
        />
        <PreviewTextField
          label="CRF"
          field="crf"
          value={preview.crf != null ? String(preview.crf) : ""}
          channelName={channelName}
          send={send}
          hint="0..=51"
        />
        <PreviewTextField
          label="JPEG quality"
          field="jpeg_quality"
          value={
            preview.jpeg_quality != null ? String(preview.jpeg_quality) : ""
          }
          channelName={channelName}
          send={send}
          hint="1..=100"
        />
      </FieldGroup>
    </>
  );
}

function RobotSubpanelGroups({
  device,
  channel,
  send,
}: {
  device: SetupAvailableDevice;
  channel: SetupDeviceChannelV2;
  send: (msg: string) => void;
}) {
  const channelName = channel.name ?? device.current.name;
  return (
    <>
      <FieldGroup title="Robot">
        <CycleField
          label="Mode"
          value={channel.mode ?? "?"}
          action="setup_subpanel_cycle_primary"
          channelName={channelName}
          send={send}
          hint={ROBOT_MODE_OPTS.join("|")}
        />
        <ReadonlyField
          label="DoF"
          value={fmtOpt(channel.dof)}
          hint="1..=15"
        />
        <TextField
          label="Control frequency (Hz)"
          value={
            channel.control_frequency_hz != null
              ? String(channel.control_frequency_hz)
              : ""
          }
          placeholder="60.0"
          onCommit={(value) =>
            send(
              encodeSetupCommand("setup_subpanel_set_control_frequency_hz", {
                name: channelName,
                value,
              }),
            )
          }
          hint=">0"
        />
      </FieldGroup>
      <FieldGroup title="States">
        <ReadonlyField
          label="publish_states"
          value={(channel.publish_states ?? []).join(", ") || "(none)"}
          hint="edit via States step"
        />
        <ReadonlyField
          label="recorded_states"
          value={(channel.recorded_states ?? []).join(", ") || "(none)"}
          hint="edit via States step"
        />
      </FieldGroup>
    </>
  );
}

// ---------------------------------------------------------------------------
// Field primitives — small focused row renderers used by every group above.
// ---------------------------------------------------------------------------

function FieldGroup({
  title,
  children,
}: {
  title: string;
  children: React.ReactNode;
}) {
  return (
    <section style={groupStyle}>
      <h3 style={groupTitleStyle}>{title}</h3>
      <div style={fieldListStyle}>{children}</div>
    </section>
  );
}

function ReadonlyField({
  label,
  value,
  hint,
}: {
  label: string;
  value: string;
  hint?: string;
}) {
  return (
    <FieldRow label={label} hint={hint}>
      <span style={readonlyValueStyle}>{value}</span>
    </FieldRow>
  );
}

interface CycleProps {
  label: string;
  value: string;
  channelName: string;
  send: (msg: string) => void;
  hint?: string;
}

function CycleField({
  label,
  value,
  action,
  channelName,
  send,
  hint,
}: CycleProps & { action: SetupCommandAction }) {
  return (
    <FieldRow label={label} hint={hint}>
      <button
        type="button"
        style={buttonStyle}
        onClick={() =>
          send(encodeSetupCommand(action, { name: channelName, delta: -1 }))
        }
      >
        ‹
      </button>
      <span style={cycleValueStyle}>{value}</span>
      <button
        type="button"
        style={buttonStyle}
        onClick={() =>
          send(encodeSetupCommand(action, { name: channelName, delta: 1 }))
        }
      >
        ›
      </button>
    </FieldRow>
  );
}

function RecordCycleField(props: {
  label: string;
  field: string;
  value: string;
  options: readonly string[];
  channelName: string;
  send: (msg: string) => void;
}) {
  return (
    <SubpanelCycleField {...props} kind="record" />
  );
}

function PreviewCycleField(props: {
  label: string;
  field: string;
  value: string;
  options: readonly string[];
  channelName: string;
  send: (msg: string) => void;
}) {
  return (
    <SubpanelCycleField {...props} kind="preview" />
  );
}

function SubpanelCycleField({
  label,
  field,
  value,
  options,
  channelName,
  send,
  kind,
}: {
  label: string;
  field: string;
  value: string;
  options: readonly string[];
  channelName: string;
  send: (msg: string) => void;
  kind: "record" | "preview";
}) {
  const action: SetupCommandAction =
    kind === "record"
      ? "setup_subpanel_cycle_record_field"
      : "setup_subpanel_cycle_preview_field";
  return (
    <FieldRow label={label} hint={options.join("|")}>
      <button
        type="button"
        style={buttonStyle}
        onClick={() =>
          send(
            encodeSetupCommand(action, {
              name: channelName,
              field,
              delta: -1,
            }),
          )
        }
      >
        ‹
      </button>
      <span style={cycleValueStyle}>{value}</span>
      <button
        type="button"
        style={buttonStyle}
        onClick={() =>
          send(
            encodeSetupCommand(action, {
              name: channelName,
              field,
              delta: 1,
            }),
          )
        }
      >
        ›
      </button>
    </FieldRow>
  );
}

function RecordTextField(props: {
  label: string;
  field: string;
  value: string;
  channelName: string;
  send: (msg: string) => void;
  hint?: string;
}) {
  return <SubpanelTextField {...props} kind="record" />;
}

function PreviewTextField(props: {
  label: string;
  field: string;
  value: string;
  channelName: string;
  send: (msg: string) => void;
  hint?: string;
}) {
  return <SubpanelTextField {...props} kind="preview" />;
}

function SubpanelTextField({
  label,
  field,
  value,
  channelName,
  send,
  hint,
  kind,
}: {
  label: string;
  field: string;
  value: string;
  channelName: string;
  send: (msg: string) => void;
  hint?: string;
  kind: "record" | "preview";
}) {
  const action: SetupCommandAction =
    kind === "record"
      ? "setup_subpanel_set_record_field"
      : "setup_subpanel_set_preview_field";
  return (
    <TextField
      label={label}
      value={value}
      hint={hint}
      onCommit={(next) =>
        send(
          encodeSetupCommand(action, {
            name: channelName,
            field,
            value: next,
          }),
        )
      }
    />
  );
}

function TextField({
  label,
  value,
  placeholder,
  onCommit,
  hint,
}: {
  label: string;
  value: string;
  placeholder?: string;
  onCommit: (value: string) => void;
  hint?: string;
}) {
  const [draft, setDraft] = useState(value);
  // Re-sync the input when the controller-side value changes
  // underneath us (e.g., the cycle of a different field updated this
  // one too).
  useEffect(() => {
    setDraft(value);
  }, [value]);
  return (
    <FieldRow label={label} hint={hint}>
      <input
        style={{ ...inputStyle, flex: 1 }}
        type="text"
        value={draft}
        placeholder={placeholder}
        onChange={(event) => setDraft(event.target.value)}
        onBlur={() => {
          if (draft !== value) onCommit(draft);
        }}
        onKeyDown={(event) => {
          if (event.key === "Enter") {
            if (draft !== value) onCommit(draft);
            (event.target as HTMLInputElement).blur();
          } else if (event.key === "Escape") {
            setDraft(value);
            (event.target as HTMLInputElement).blur();
          }
        }}
      />
    </FieldRow>
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
      <div style={fieldLabelStyle}>
        <span>{label}</span>
        {hint ? <span style={fieldHintStyle}>{hint}</span> : null}
      </div>
      <div style={fieldControlStyle}>{children}</div>
    </div>
  );
}

const headerStyle: CSSProperties = {
  display: "flex",
  justifyContent: "space-between",
  alignItems: "flex-start",
  marginBottom: "1rem",
  gap: "1rem",
};

const titleStyle: CSSProperties = {
  margin: 0,
  fontSize: "1.125rem",
  fontWeight: 600,
};

const subtitleStyle: CSSProperties = {
  margin: "0.25rem 0 0 0",
  fontSize: "0.75rem",
  color: palette.textMuted,
};

const mutedStyle: CSSProperties = {
  color: palette.textMuted,
  fontWeight: 400,
};

const groupStyle: CSSProperties = {
  marginBottom: "1rem",
};

const groupTitleStyle: CSSProperties = {
  margin: "0 0 0.5rem 0",
  fontSize: "0.75rem",
  fontWeight: 600,
  letterSpacing: "0.04em",
  textTransform: "uppercase",
  color: palette.textMuted,
};

const fieldListStyle: CSSProperties = {
  display: "flex",
  flexDirection: "column",
  border: `1px solid ${palette.border}`,
  borderRadius: "0.375rem",
  overflow: "hidden",
};

const fieldRowStyle: CSSProperties = {
  display: "grid",
  gridTemplateColumns: "minmax(8rem, 14rem) 1fr",
  alignItems: "center",
  gap: "0.75rem",
  padding: "0.5rem 0.75rem",
  borderBottom: `1px solid ${palette.border}`,
  background: palette.surface,
};

const fieldLabelStyle: CSSProperties = {
  display: "flex",
  flexDirection: "column",
  gap: "0.125rem",
};

const fieldHintStyle: CSSProperties = {
  fontSize: "0.7rem",
  color: palette.textMuted,
};

const fieldControlStyle: CSSProperties = {
  display: "flex",
  alignItems: "center",
  gap: "0.5rem",
};

const readonlyValueStyle: CSSProperties = {
  color: palette.textMuted,
  fontFamily: "ui-monospace, SFMono-Regular, monospace",
  fontSize: "0.875rem",
};

const cycleValueStyle: CSSProperties = {
  flex: 1,
  textAlign: "center",
  fontFamily: "ui-monospace, SFMono-Regular, monospace",
  fontSize: "0.875rem",
};
