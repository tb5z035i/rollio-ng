/// Maximum number of camera tiles rendered side-by-side in **one** preview
/// row. Mirrors `rollio_types::config::MAX_PREVIEW_CAMERAS`.
///
/// Configuring more cameras than this no longer hides any tile — the grid
/// wraps onto additional rows (see {@link CameraGrid}). The constant only
/// caps the per-row column count so each tile keeps the requested 16:10
/// box without becoming unreadable.
export const MAX_PREVIEW_CAMERAS = 3;

/**
 * Resolve which camera channels to display, in stable order. Configured
 * channels appear first; any active-but-unconfigured channels are appended
 * so a freshly-discovered stream still appears.
 *
 * No truncation: callers are responsible for laying tiles out (the web /
 * terminal UIs wrap onto extra rows once a row already holds
 * {@link MAX_PREVIEW_CAMERAS} tiles).
 */
export function resolveCameraNames(
  configuredCameraNames: readonly string[],
  activeFrameNames: readonly string[],
): string[] {
  if (configuredCameraNames.length > 0) {
    const merged = [...configuredCameraNames];
    for (const name of activeFrameNames) {
      if (!merged.includes(name)) {
        merged.push(name);
      }
    }
    return merged;
  }

  if (activeFrameNames.length > 0) {
    return [...activeFrameNames];
  }

  return ["camera_0", "camera_1"];
}
