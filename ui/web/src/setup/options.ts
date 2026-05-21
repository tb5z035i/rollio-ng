/**
 * Per-channel encoder defaults + cycle option lists.
 *
 * Mirrored from `controller/src/setup/subpanel.rs` and the TUI constants
 * in `ui/terminal/src/SetupApp.tsx`. The Rust dispatcher is the
 * authority — these strings are only used to render value placeholders
 * and "(option1 | option2)" range hints. Update both sides when adding
 * new variants.
 */

export const RECORD_DEFAULTS = {
  video_codec: "h264",
  depth_codec: "rvl",
  backend: "auto",
  video_backend: "auto",
  depth_backend: "auto",
  chroma_subsampling: "422",
  bit_depth: 8,
  color_space: "auto",
  queue_size: 32,
} as const;

export const PREVIEW_DEFAULTS = {
  output_mode: "encoded",
  color_codec: "h264",
  depth_codec: "rvl",
  backend: "auto",
  width: 320,
  height: 240,
  fps: 15,
  gop_seconds: 1,
  crf: 26,
  jpeg_quality: 50,
} as const;

export const RECORD_VIDEO_CODEC_OPTS = ["h264", "h265", "av1", "mjpg"] as const;
export const RECORD_DEPTH_CODEC_OPTS = ["rvl"] as const;
export const RECORD_BACKEND_OPTS = [
  "auto",
  "cpu",
  "nvidia",
  "vaapi",
  "passthrough",
  "horizon-x5",
] as const;
export const RECORD_CHROMA_OPTS = ["422", "420"] as const;
export const RECORD_BIT_DEPTH_OPTS = [8, 10] as const;
export const RECORD_COLOR_SPACE_OPTS = [
  "auto",
  "bt709-limited",
  "bt601-limited",
] as const;
export const RECORD_PRESET_OPTS = [
  "(default)",
  "ultrafast",
  "veryfast",
  "fast",
  "medium",
  "slow",
  "slower",
  "veryslow",
] as const;
export const PREVIEW_OUTPUT_MODE_OPTS = ["jpeg", "encoded"] as const;

export const ROBOT_MODE_OPTS = ["free-drive", "command-following"] as const;
export const COLLECTION_MODE_OPTS = ["teleop", "intervention"] as const;
export const EPISODE_FORMAT_OPTS = [
  "lerobot-v2.1",
  "lerobot-v3.0",
  "mcap",
] as const;
export const STORAGE_BACKEND_OPTS = ["local", "http", "dataloop"] as const;
export const MAPPING_POLICY_OPTS = ["direct-joint", "cartesian", "parallel"] as const;

/** Human-readable representation of an optional value with a fallback default. */
export function fmtOpt<T>(
  value: T | null | undefined,
  fallback?: T | string,
): string {
  if (value == null || value === "") {
    return fallback != null ? `(${fallback})` : "(unset)";
  }
  return String(value);
}

export function fmtBool(value: boolean | null | undefined): string {
  return value === false ? "off" : "on";
}
