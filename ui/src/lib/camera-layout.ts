export function resolveCameraNames(
  configuredCameraNames: readonly string[],
  activeFrameNames: readonly string[],
): string[] {
  if (configuredCameraNames.length > 0) {
    const names = [...configuredCameraNames];
    for (const name of activeFrameNames) {
      if (!names.includes(name)) {
        names.push(name);
      }
    }
    return names;
  }

  if (activeFrameNames.length > 0) {
    return [...activeFrameNames];
  }

  return ["camera_0", "camera_1"];
}
