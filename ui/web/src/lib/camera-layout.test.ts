import { describe, expect, it } from "vitest";
import { MAX_PREVIEW_CAMERAS, resolveCameraNames } from "./camera-layout";

describe("resolveCameraNames", () => {
  it("keeps configured streams visible", () => {
    expect(
      resolveCameraNames(
        ["camera_d435i_rgb", "camera_d435i_depth"],
        ["camera_d435i_rgb"],
      ),
    ).toEqual(["camera_d435i_rgb", "camera_d435i_depth"]);
  });

  it("appends unexpected active streams", () => {
    expect(
      resolveCameraNames(["camera_a"], ["camera_a", "camera_b"]),
    ).toEqual(["camera_a", "camera_b"]);
  });

  it(`returns every configured channel even when more than ${MAX_PREVIEW_CAMERAS} are configured (overflow wraps to a second row in the grid, not a silent drop)`, () => {
    const configured = Array.from(
      { length: MAX_PREVIEW_CAMERAS + 2 },
      (_, i) => `cam_${i}`,
    );
    const names = resolveCameraNames(configured, []);
    expect(names).toEqual(configured);
  });

  it("falls back to placeholders when nothing is configured or active", () => {
    expect(resolveCameraNames([], [])).toEqual(["camera_0", "camera_1"]);
  });
});
