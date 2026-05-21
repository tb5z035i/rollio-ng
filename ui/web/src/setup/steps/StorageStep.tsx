import { useEffect, useState, type CSSProperties } from "react";
import {
  encodeSetupCommand,
  type SetupCommandAction,
  type SetupStateMessage,
} from "../../lib/protocol";
import { COLLECTION_MODE_OPTS, EPISODE_FORMAT_OPTS, STORAGE_BACKEND_OPTS } from "../options";
import {
  buttonStyle,
  inputStyle,
  palette,
  panelStyle,
  panelTitleStyle,
} from "../styles";

interface StorageStepProps {
  setupState: SetupStateMessage;
  send: (msg: string) => void;
}

export function StorageStep({ setupState, send }: StorageStepProps) {
  const { config } = setupState;
  const ui = config.ui;
  const controller = config.controller;
  const visualizer = config.visualizer;
  const assembler = config.assembler;
  const monitor = config.monitor;

  return (
    <div style={layoutStyle}>
      <SettingsGroup title="Project">
        <TextSetting
          label="Name"
          value={config.project_name}
          action="setup_set_project_name"
          send={send}
        />
        <CycleSetting
          label="Collection mode"
          value={config.mode}
          options={COLLECTION_MODE_OPTS}
          action="setup_cycle_collection_mode"
          send={send}
        />
      </SettingsGroup>

      <SettingsGroup title="Episode">
        <CycleSetting
          label="Format"
          value={config.episode.format}
          options={EPISODE_FORMAT_OPTS}
          action="setup_cycle_episode_format"
          send={send}
        />
        <TextSetting
          label="FPS"
          value={String(config.episode.fps)}
          action="setup_set_episode_fps"
          send={send}
          hint="1..=1000"
          inputType="number"
        />
        <TextSetting
          label="Chunk size"
          value={config.episode.chunk_size != null ? String(config.episode.chunk_size) : ""}
          action="setup_set_episode_chunk_size"
          send={send}
          hint=">0"
          inputType="number"
        />
      </SettingsGroup>

      <SettingsGroup title="Storage">
        <CycleSetting
          label="Backend"
          value={config.storage.backend}
          options={STORAGE_BACKEND_OPTS}
          action="setup_cycle_storage_backend"
          send={send}
        />
        <TextSetting
          label="Output path"
          value={config.storage.output_path}
          action="setup_set_storage_output_path"
          send={send}
        />
        <TextSetting
          label="Endpoint"
          value={config.storage.endpoint ?? ""}
          action="setup_set_storage_endpoint"
          send={send}
          hint="https://... (used by http/dataloop backends)"
        />
        {config.storage.backend === "dataloop" ? (
          <>
            <TextSetting
              label="Dataloop project id"
              value={config.storage.dataloop_project_id ?? ""}
              action="setup_set_dataloop_project_id"
              send={send}
            />
            <TextSetting
              label="Dataloop API token"
              value={config.storage.dataloop_token ?? ""}
              action="setup_set_dataloop_token"
              send={send}
              hint="kept in the saved config — store securely"
              inputType="password"
            />
          </>
        ) : null}
        <TextSetting
          label="Queue size"
          value={config.storage.queue_size != null ? String(config.storage.queue_size) : ""}
          action="setup_set_storage_queue_size"
          send={send}
          hint=">0"
          inputType="number"
        />
      </SettingsGroup>

      <SettingsGroup title="UI gateway">
        <TextSetting
          label="HTTP host"
          value={ui?.http_host ?? "0.0.0.0"}
          action="setup_set_ui_http_host"
          send={send}
          hint="0.0.0.0 to listen on all interfaces"
        />
        <TextSetting
          label="HTTP port"
          value={ui?.http_port != null ? String(ui.http_port) : ""}
          action="setup_set_ui_http_port"
          send={send}
          inputType="number"
          hint="1..=65535"
        />
        <TextSetting
          label="Start key"
          value={ui?.start_key ?? "s"}
          action="setup_set_ui_start_key"
          send={send}
          hint="single character"
        />
        <TextSetting
          label="Stop key"
          value={ui?.stop_key ?? "e"}
          action="setup_set_ui_stop_key"
          send={send}
          hint="single character"
        />
        <TextSetting
          label="Keep key"
          value={ui?.keep_key ?? "k"}
          action="setup_set_ui_keep_key"
          send={send}
          hint="single character"
        />
        <TextSetting
          label="Discard key"
          value={ui?.discard_key ?? "x"}
          action="setup_set_ui_discard_key"
          send={send}
          hint="single character"
        />
      </SettingsGroup>

      <SettingsGroup title="Controller">
        <TextSetting
          label="Shutdown timeout (ms)"
          value={controller?.shutdown_timeout_ms != null ? String(controller.shutdown_timeout_ms) : ""}
          action="setup_set_controller_shutdown_timeout_ms"
          send={send}
          inputType="number"
        />
        <TextSetting
          label="Child poll interval (ms)"
          value={controller?.child_poll_interval_ms != null ? String(controller.child_poll_interval_ms) : ""}
          action="setup_set_controller_child_poll_interval_ms"
          send={send}
          inputType="number"
        />
      </SettingsGroup>

      <SettingsGroup title="Visualizer">
        <TextSetting
          label="Port"
          value={visualizer?.port != null ? String(visualizer.port) : ""}
          action="setup_set_visualizer_port"
          send={send}
          inputType="number"
          hint="1..=65535"
        />
      </SettingsGroup>

      <SettingsGroup title="Assembler">
        <TextSetting
          label="Missing-EOS timeout (ms)"
          value={assembler?.missing_eos_timeout_ms != null ? String(assembler.missing_eos_timeout_ms) : ""}
          action="setup_set_assembler_missing_eos_timeout_ms"
          send={send}
          inputType="number"
        />
        <TextSetting
          label="Staging directory"
          value={assembler?.staging_dir ?? ""}
          action="setup_set_assembler_staging_dir"
          send={send}
        />
        <TextSetting
          label="Staging slots"
          value={assembler?.staging_slots != null ? String(assembler.staging_slots) : ""}
          action="setup_set_assembler_staging_slots"
          send={send}
          inputType="number"
        />
      </SettingsGroup>

      <SettingsGroup title="Monitor">
        <TextSetting
          label="Metrics frequency (Hz)"
          value={monitor?.metrics_frequency_hz != null ? String(monitor.metrics_frequency_hz) : ""}
          action="setup_set_monitor_metrics_frequency_hz"
          send={send}
          inputType="number"
        />
      </SettingsGroup>
    </div>
  );
}

function SettingsGroup({
  title,
  children,
}: {
  title: string;
  children: React.ReactNode;
}) {
  return (
    <section style={panelStyle}>
      <h3 style={panelTitleStyle}>{title}</h3>
      <div style={fieldStackStyle}>{children}</div>
    </section>
  );
}

function CycleSetting({
  label,
  value,
  options,
  action,
  send,
  hint,
}: {
  label: string;
  value: string;
  options: readonly string[];
  action: SetupCommandAction;
  send: (msg: string) => void;
  hint?: string;
}) {
  return (
    <FieldRow label={label} hint={hint ?? options.join("|")}>
      <button
        type="button"
        style={buttonStyle}
        onClick={() => send(encodeSetupCommand(action, { delta: -1 }))}
      >
        ‹
      </button>
      <span style={cycleValueStyle}>{value}</span>
      <button
        type="button"
        style={buttonStyle}
        onClick={() => send(encodeSetupCommand(action, { delta: 1 }))}
      >
        ›
      </button>
    </FieldRow>
  );
}

function TextSetting({
  label,
  value,
  action,
  send,
  hint,
  inputType,
}: {
  label: string;
  value: string;
  action: SetupCommandAction;
  send: (msg: string) => void;
  hint?: string;
  inputType?: "text" | "number" | "password";
}) {
  const [draft, setDraft] = useState(value);
  useEffect(() => {
    setDraft(value);
  }, [value]);
  return (
    <FieldRow label={label} hint={hint}>
      <input
        type={inputType ?? "text"}
        style={{ ...inputStyle, flex: 1 }}
        value={draft}
        onChange={(event) => setDraft(event.target.value)}
        onBlur={() => {
          if (draft !== value) {
            send(encodeSetupCommand(action, { value: draft }));
          }
        }}
        onKeyDown={(event) => {
          if (event.key === "Enter") {
            if (draft !== value) {
              send(encodeSetupCommand(action, { value: draft }));
            }
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
      <div style={labelStyle}>
        <span>{label}</span>
        {hint ? <span style={hintStyle}>{hint}</span> : null}
      </div>
      <div style={controlStyle}>{children}</div>
    </div>
  );
}

const layoutStyle: CSSProperties = {
  display: "grid",
  gridTemplateColumns: "repeat(auto-fit, minmax(20rem, 1fr))",
  gap: "1rem",
};

const fieldStackStyle: CSSProperties = {
  display: "flex",
  flexDirection: "column",
  gap: "0.5rem",
};

const fieldRowStyle: CSSProperties = {
  display: "grid",
  gridTemplateColumns: "minmax(8rem, 14rem) 1fr",
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

const cycleValueStyle: CSSProperties = {
  flex: 1,
  textAlign: "center",
  fontFamily: "ui-monospace, SFMono-Regular, monospace",
  fontSize: "0.875rem",
};
