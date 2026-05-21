import type { CSSProperties } from "react";
import { encodeSetupCommand, type SetupStep } from "../../lib/protocol";
import { palette, stepperRowStyle } from "../styles";

interface StepperProps {
  currentStep: SetupStep;
  totalSteps: number;
  stepIndex: number;
  /** Ordered list of step ids the wizard currently exposes. The
   *  controller suppresses Pairing in Intervention mode (so the list
   *  is 4 steps, not 5) — we honor whatever the snapshot reports
   *  rather than hard-coding the full set. */
  steps: SetupStep[];
  send: (msg: string) => void;
  connected: boolean;
}

const STEP_LABELS: Record<SetupStep, string> = {
  devices: "Devices",
  states: "States",
  pairing: "Pairing",
  storage: "Storage",
  preview: "Preview",
};

export function Stepper({
  currentStep,
  totalSteps,
  stepIndex,
  steps,
  send,
  connected,
}: StepperProps) {
  return (
    <div style={stepperRowStyle}>
      <div style={titleStyle}>Rollio setup</div>
      <div style={{ flex: 1, display: "flex", gap: "0.25rem" }}>
        {steps.map((step, index) => {
          const isCurrent = step === currentStep;
          return (
            <button
              key={step}
              type="button"
              disabled={!connected}
              onClick={() =>
                send(encodeSetupCommand("setup_jump_step", { value: step }))
              }
              style={chipStyle(isCurrent)}
            >
              <span style={chipIndexStyle(isCurrent)}>{index + 1}</span>
              <span>{STEP_LABELS[step]}</span>
            </button>
          );
        })}
      </div>
      <div style={metaStyle}>
        Step {stepIndex + 1} / {totalSteps}
      </div>
    </div>
  );
}

const titleStyle: CSSProperties = {
  fontWeight: 600,
  fontSize: "1rem",
  marginRight: "1rem",
};

const metaStyle: CSSProperties = {
  marginLeft: "1rem",
  fontSize: "0.75rem",
  color: palette.textMuted,
};

function chipStyle(active: boolean): CSSProperties {
  return {
    display: "flex",
    alignItems: "center",
    gap: "0.5rem",
    padding: "0.375rem 0.75rem",
    border: `1px solid ${active ? palette.accent : palette.border}`,
    background: active ? palette.accentMuted : palette.surfaceMuted,
    color: active ? "#bfdbfe" : palette.textMuted,
    borderRadius: "9999px",
    cursor: "pointer",
    fontSize: "0.875rem",
    fontFamily: "inherit",
  };
}

function chipIndexStyle(active: boolean): CSSProperties {
  return {
    display: "inline-flex",
    alignItems: "center",
    justifyContent: "center",
    minWidth: "1.25rem",
    height: "1.25rem",
    borderRadius: "9999px",
    background: active ? palette.accent : palette.border,
    color: "#fff",
    fontSize: "0.7rem",
    fontWeight: 600,
  };
}
