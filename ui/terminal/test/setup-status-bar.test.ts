import assert from "node:assert/strict";
import test from "node:test";
import { buildSetupStatusBarLeft } from "../src/components/SetupStatusBar.js";

test("buildSetupStatusBarLeft includes step progress and output path", () => {
  const line = buildSetupStatusBarLeft({
    stepIndex: 3,
    totalSteps: 6,
    connected: true,
    outputPath: "/tmp/setup/config.toml",
    status: "editing",
    stepHint: "j/k:Focus h/l:Cycle",
  });

  assert.match(line, /3\/6/);
  assert.match(line, /Keys:/);
  assert.match(line, /WS: Connected/);
  assert.match(line, /j\/k:Focus h\/l:Cycle/);
  assert.match(line, /config\.toml/);
});

test("buildSetupStatusBarLeft keeps key hints visible when controller messages exist", () => {
  const line = buildSetupStatusBarLeft({
    stepIndex: 6,
    totalSteps: 6,
    connected: true,
    outputPath: "config.toml",
    status: "saved",
    stepHint: "Enter:Save",
    message: "Saved config.toml",
  });

  assert.match(line, /Keys: Enter:Save/);
  assert.doesNotMatch(line, /Saved config\.toml/);
});
