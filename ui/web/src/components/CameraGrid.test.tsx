import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { CameraGrid } from "./CameraGrid";

describe("CameraGrid scaling_locked badge", () => {
  it("renders no lock badge when scalingLocked is false", () => {
    const { queryByTestId } = render(
      <CameraGrid
        cameras={[
          { name: "camera/color", frame: undefined, scalingLocked: false },
        ]}
      />,
    );
    expect(queryByTestId("camera-lock-camera/color")).toBeNull();
  });

  it("renders no lock badge when scalingLocked is omitted (default behavior)", () => {
    const { queryByTestId } = render(
      <CameraGrid
        cameras={[{ name: "camera/color", frame: undefined }]}
      />,
    );
    expect(queryByTestId("camera-lock-camera/color")).toBeNull();
  });

  it("renders the lock badge with explanatory tooltip when scalingLocked is true", () => {
    const { getByTestId } = render(
      <CameraGrid
        cameras={[
          { name: "camera/color", frame: undefined, scalingLocked: true },
        ]}
      />,
    );
    const badge = getByTestId("camera-lock-camera/color");
    expect(badge).not.toBeNull();
    // Tooltip explains the cause so a user hovering it understands why
    // their resize gestures aren't taking effect.
    expect(badge.getAttribute("title")).toMatch(/passthrough/i);
    expect(badge.textContent).toMatch(/locked/i);
  });

  it("renders one badge per locked camera, independent of unlocked siblings", () => {
    const { queryByTestId } = render(
      <CameraGrid
        cameras={[
          { name: "cam/a", frame: undefined, scalingLocked: true },
          { name: "cam/b", frame: undefined, scalingLocked: false },
          { name: "cam/c", frame: undefined, scalingLocked: true },
        ]}
      />,
    );
    expect(queryByTestId("camera-lock-cam/a")).not.toBeNull();
    expect(queryByTestId("camera-lock-cam/b")).toBeNull();
    expect(queryByTestId("camera-lock-cam/c")).not.toBeNull();
  });

  it("renders a preview issue instead of the generic no-signal placeholder", () => {
    const { getByTestId } = render(
      <CameraGrid
        cameras={[
          {
            name: "camera/color",
            frame: undefined,
            previewIssue: "WebCodecs unavailable",
            previewIssueTitle: "Open the UI through localhost.",
          },
        ]}
      />,
    );

    const placeholder = getByTestId("camera-placeholder-camera/color");
    expect(placeholder.textContent).toBe("WebCodecs unavailable");
    expect(placeholder.getAttribute("title")).toBe(
      "Open the UI through localhost.",
    );
  });
});
