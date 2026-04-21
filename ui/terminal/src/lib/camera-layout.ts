/// Maximum number of camera tiles rendered side-by-side in **one** preview
/// row. Mirrors `rollio_types::config::MAX_PREVIEW_CAMERAS`.
///
/// Tiles past this count wrap onto additional rows in `LivePreviewPanels`
/// rather than being silently dropped — operators see every configured
/// stream while each tile keeps the 16:10 box.
export const MAX_PREVIEW_CAMERAS = 3;

/**
 * Resolve which camera channels to display, in stable order. Configured
 * channels appear first; any active-but-unconfigured channels are
 * appended (so a new stream still appears immediately).
 *
 * No truncation: the live preview wraps overflow into additional rows
 * (see `LivePreviewPanels`).
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
