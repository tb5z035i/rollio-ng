import assert from "node:assert/strict";
import test from "node:test";
import { buildSetupStatusBarLeft } from "../src/components/SetupStatusBar.js";

// Key hints have moved to a dedicated `KeyHintsBar` row. The status bar
// keeps only step progress, websocket health, and the output file path —
// any test-time assertion that relied on `Keys:` here should target the
// hints bar instead.

test("buildSetupStatusBarLeft includes step progress and output path", () => {
  const line = buildSetupStatusBarLeft({
    stepIndex: 3,
    totalSteps: 6,
    connected: true,
    outputPath: "/tmp/setup/config.toml",
    status: "editing",
  });

  assert.match(line, /3\/6/);
  assert.match(line, /WS: Connected/);
  assert.match(line, /config\.toml/);
  assert.doesNotMatch(line, /Keys:/);
});

test("buildSetupStatusBarLeft drops in-flight messages from the bar text", () => {
  const line = buildSetupStatusBarLeft({
    stepIndex: 6,
    totalSteps: 6,
    connected: true,
    outputPath: "config.toml",
    status: "saved",
    message: "Saved config.toml",
  });

  assert.doesNotMatch(line, /Saved config\.toml/);
});
