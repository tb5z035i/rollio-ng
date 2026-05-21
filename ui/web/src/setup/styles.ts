/**
 * Inline-style atoms shared across the setup wizard.
 *
 * Kept here (not in `index.css`) so each `*.tsx` file declares only
 * what it uses. The wizard is a self-contained route — we don't want
 * its dark theme leaking into the collect view's stylesheet.
 */
import type { CSSProperties } from "react";

export const palette = {
  bg: "#0b0d10",
  surface: "#18181b",
  surfaceMuted: "#1f1f23",
  border: "#27272a",
  borderStrong: "#3f3f46",
  text: "#e4e4e7",
  textMuted: "#a1a1aa",
  accent: "#3b82f6",
  accentMuted: "#1e3a8a",
  warning: "#facc15",
  danger: "#ef4444",
  ok: "#22c55e",
} as const;

export const shellStyle: CSSProperties = {
  display: "flex",
  flexDirection: "column",
  minHeight: "100vh",
  background: palette.bg,
  color: palette.text,
  fontFamily:
    "ui-sans-serif, system-ui, -apple-system, 'Segoe UI', Roboto, sans-serif",
  fontSize: "14px",
};

export const stepperRowStyle: CSSProperties = {
  display: "flex",
  gap: "0.25rem",
  alignItems: "center",
  padding: "0.75rem 1.25rem",
  background: palette.surface,
  borderBottom: `1px solid ${palette.border}`,
};

export const stepBodyStyle: CSSProperties = {
  flex: 1,
  display: "flex",
  flexDirection: "column",
  padding: "1.25rem",
  gap: "1rem",
  overflow: "auto",
};

export const statusBarStyle: CSSProperties = {
  display: "flex",
  alignItems: "center",
  justifyContent: "space-between",
  gap: "1rem",
  padding: "0.5rem 1.25rem",
  background: palette.surface,
  borderTop: `1px solid ${palette.border}`,
  fontSize: "0.875rem",
  color: palette.textMuted,
};

export const panelStyle: CSSProperties = {
  background: palette.surface,
  border: `1px solid ${palette.border}`,
  borderRadius: "0.5rem",
  padding: "1rem",
};

export const panelTitleStyle: CSSProperties = {
  margin: "0 0 0.75rem 0",
  fontSize: "1rem",
  fontWeight: 600,
  color: palette.text,
};

export const tableStyle: CSSProperties = {
  width: "100%",
  borderCollapse: "collapse",
};

export const tableHeaderCell: CSSProperties = {
  textAlign: "left",
  padding: "0.5rem 0.75rem",
  fontWeight: 500,
  color: palette.textMuted,
  borderBottom: `1px solid ${palette.border}`,
};

export const tableBodyCell: CSSProperties = {
  padding: "0.5rem 0.75rem",
  borderBottom: `1px solid ${palette.border}`,
  verticalAlign: "middle",
};

export const buttonStyle: CSSProperties = {
  background: palette.surfaceMuted,
  border: `1px solid ${palette.borderStrong}`,
  color: palette.text,
  padding: "0.375rem 0.75rem",
  fontSize: "0.875rem",
  borderRadius: "0.375rem",
  cursor: "pointer",
  fontFamily: "inherit",
};

export const primaryButtonStyle: CSSProperties = {
  ...buttonStyle,
  background: palette.accent,
  borderColor: palette.accent,
  color: "#fff",
  fontWeight: 600,
};

export const dangerButtonStyle: CSSProperties = {
  ...buttonStyle,
  background: "transparent",
  borderColor: palette.danger,
  color: palette.danger,
};

export const ghostButtonStyle: CSSProperties = {
  ...buttonStyle,
  background: "transparent",
  borderColor: palette.border,
  color: palette.textMuted,
};

export const inputStyle: CSSProperties = {
  background: palette.surfaceMuted,
  border: `1px solid ${palette.borderStrong}`,
  color: palette.text,
  padding: "0.25rem 0.5rem",
  fontSize: "0.875rem",
  borderRadius: "0.25rem",
  fontFamily: "inherit",
  minWidth: 0,
};

export const modalBackdropStyle: CSSProperties = {
  position: "fixed",
  inset: 0,
  background: "rgba(0,0,0,0.65)",
  display: "flex",
  alignItems: "center",
  justifyContent: "center",
  zIndex: 100,
};

export const modalCardStyle: CSSProperties = {
  background: palette.bg,
  border: `1px solid ${palette.borderStrong}`,
  borderRadius: "0.5rem",
  padding: "1.25rem",
  width: "min(720px, 92vw)",
  maxHeight: "88vh",
  overflow: "auto",
  boxShadow: "0 16px 48px rgba(0,0,0,0.5)",
};

export const warningBannerStyle: CSSProperties = {
  background: "rgba(250, 204, 21, 0.08)",
  border: `1px solid ${palette.warning}`,
  color: palette.warning,
  padding: "0.5rem 0.75rem",
  borderRadius: "0.375rem",
  fontSize: "0.875rem",
};
