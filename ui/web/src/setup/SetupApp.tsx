import { useEffect, useMemo, type CSSProperties } from "react";
import type { UiRuntimeConfig } from "../lib/runtime-config";
import {
  encodeSetupCommand,
  type SetupStep,
  type SetupStateMessage,
} from "../lib/protocol";
import {
  useControlSocket,
  type UseControlSocketOptions,
} from "../lib/websocket";
import { Stepper } from "./components/Stepper";
import { SetupStatusBar } from "./components/SetupStatusBar";
import { DevicesStep } from "./steps/DevicesStep";
import { PairingStep } from "./steps/PairingStep";
import { PreviewStep } from "./steps/PreviewStep";
import { StatesStep } from "./steps/StatesStep";
import { StorageStep } from "./steps/StorageStep";
import {
  palette,
  shellStyle,
  stepBodyStyle,
  warningBannerStyle,
} from "./styles";

export interface SetupAppProps {
  runtimeConfig: UiRuntimeConfig;
  /** Test hook: forwarded into `useControlSocket` so tests can swap
   *  the WebSocket factory without intercepting the global. Mirrors
   *  the `controlSocketOptions` prop on the collect-mode `<App>`. */
  controlSocketOptions?: UseControlSocketOptions;
}

const FULL_STEP_ORDER: SetupStep[] = [
  "devices",
  "states",
  "pairing",
  "storage",
  "preview",
];

export default function SetupApp({
  runtimeConfig,
  controlSocketOptions,
}: SetupAppProps) {
  const { setupState, connected, send } = useControlSocket(
    runtimeConfig.controlWebsocketUrl,
    controlSocketOptions,
  );

  // Re-poll setup state on connect and every second after. The
  // controller already publishes on every state mutation, but the
  // periodic refresh recovers from any iceoryx2 quota overflow
  // (matches the Ink TUI's behavior).
  useEffect(() => {
    if (!connected) return;
    send(encodeSetupCommand("setup_get_state"));
    const interval = window.setInterval(() => {
      send(encodeSetupCommand("setup_get_state"));
    }, 1000);
    return () => window.clearInterval(interval);
  }, [connected, send]);

  // The controller hides Pairing when collection mode is Intervention.
  // Use the observed total_steps to derive which steps are visible.
  const visibleSteps = useMemo<SetupStep[]>(() => {
    if (!setupState) return FULL_STEP_ORDER;
    if (setupState.total_steps >= FULL_STEP_ORDER.length) return FULL_STEP_ORDER;
    return FULL_STEP_ORDER.filter((step) => step !== "pairing");
  }, [setupState]);

  if (!setupState) {
    return (
      <div style={shellStyle}>
        <header style={connectingHeaderStyle}>
          <h1 style={titleStyle}>Rollio setup</h1>
          <span style={connectingMetaStyle}>
            {connected ? "Waiting for state…" : "Connecting to controller…"}
          </span>
        </header>
      </div>
    );
  }

  return (
    <div style={shellStyle}>
      <Stepper
        currentStep={setupState.step}
        totalSteps={setupState.total_steps}
        stepIndex={setupState.step_index}
        steps={visibleSteps}
        send={send}
        connected={connected}
      />

      <main style={stepBodyStyle}>
        <Warnings warnings={setupState.warnings} />
        <StepBody
          setupState={setupState}
          send={send}
          previewWebsocketUrl={runtimeConfig.previewWebsocketUrl}
        />
      </main>

      <SetupStatusBar
        setupState={setupState}
        send={send}
        connected={connected}
      />
    </div>
  );
}

function StepBody({
  setupState,
  send,
  previewWebsocketUrl,
}: {
  setupState: SetupStateMessage;
  send: (msg: string) => void;
  previewWebsocketUrl: string;
}) {
  switch (setupState.step) {
    case "devices":
      return (
        <DevicesStep
          setupState={setupState}
          send={send}
          previewWebsocketUrl={previewWebsocketUrl}
        />
      );
    case "states":
      return <StatesStep setupState={setupState} send={send} />;
    case "pairing":
      return <PairingStep setupState={setupState} send={send} />;
    case "storage":
      return <StorageStep setupState={setupState} send={send} />;
    case "preview":
      return (
        <PreviewStep
          setupState={setupState}
          previewWebsocketUrl={previewWebsocketUrl}
        />
      );
  }
}

function Warnings({ warnings }: { warnings: string[] }) {
  if (warnings.length === 0) return null;
  return (
    <div style={warningBannerStyle}>
      <strong>Warnings</strong>
      <ul style={warningListStyle}>
        {warnings.map((warning, index) => (
          <li key={index}>{warning}</li>
        ))}
      </ul>
    </div>
  );
}

const connectingHeaderStyle: CSSProperties = {
  display: "flex",
  alignItems: "center",
  justifyContent: "space-between",
  padding: "1.5rem",
  borderBottom: `1px solid ${palette.border}`,
};

const titleStyle: CSSProperties = {
  margin: 0,
  fontSize: "1.5rem",
  fontWeight: 600,
};

const connectingMetaStyle: CSSProperties = {
  color: palette.textMuted,
};

const warningListStyle: CSSProperties = {
  margin: "0.5rem 0 0 1.25rem",
  padding: 0,
};
