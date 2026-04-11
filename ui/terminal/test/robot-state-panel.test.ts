import assert from "node:assert/strict";
import test from "node:test";
import { buildRobotPanelLines } from "../src/components/RobotStatePanel.js";

test("buildRobotPanelLines includes end-effector status for standalone EEFs", () => {
  const lines = buildRobotPanelLines({
    name: "eef_g2",
    numJoints: 1,
    positions: [0.042],
    panelWidth: 48,
    endEffectorStatus: "enabled",
    endEffectorFeedbackValid: true,
  });

  assert.match(lines.join("\n"), /Status: Enabled \| Feedback: ok/);
  assert.match(lines.join("\n"), /J0/);
  assert.match(lines.join("\n"), /0\.04/);
});

test("buildRobotPanelLines falls back to waiting message for regular robots", () => {
  const lines = buildRobotPanelLines({
    name: "leader_arm",
    numJoints: 0,
    positions: [],
    panelWidth: 48,
  });

  assert.match(lines.join("\n"), /Waiting for data/);
});

test("buildRobotPanelLines scales end-effector bars over 0.00 to 0.07", () => {
  const closed = buildRobotPanelLines({
    name: "eef_g2",
    numJoints: 1,
    positions: [0.0],
    panelWidth: 48,
    endEffectorStatus: "enabled",
    endEffectorFeedbackValid: true,
  }).join("\n");
  const open = buildRobotPanelLines({
    name: "eef_g2",
    numJoints: 1,
    positions: [0.07],
    panelWidth: 48,
    endEffectorStatus: "enabled",
    endEffectorFeedbackValid: true,
  }).join("\n");

  assert.match(closed, /0\.00/);
  assert.match(open, /0\.07/);
  assert.ok(
    (open.match(/█/g) ?? []).length > (closed.match(/█/g) ?? []).length,
    "open end effector should render a fuller bar than the closed position",
  );
});
