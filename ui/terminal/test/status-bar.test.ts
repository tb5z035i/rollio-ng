import assert from "node:assert/strict";
import test from "node:test";
import {
  buildStatusBarLeft,
  formatElapsedMs,
} from "../src/components/StatusBar.js";

// Episode-state controls (start/stop/keep/discard) and debug/renderer
// toggles moved to the dedicated `KeyHintsBar` row above the status bar.
// The status bar now only carries always-visible session metadata.

test("formatElapsedMs renders mm:ss timers", () => {
  assert.equal(formatElapsedMs(5_250), "0:05");
  assert.equal(formatElapsedMs(125_000), "2:05");
});

test("buildStatusBarLeft shows recording timer and episode count", () => {
  const line = buildStatusBarLeft({
    mode: "Collect",
    state: "recording",
    episodeCount: 2,
    elapsedMs: 5_250,
    connected: true,
  });

  assert.match(line, /Recording 0:05/);
  assert.match(line, /Ep: 2/);
  assert.match(line, /WS: Connected/);
  assert.doesNotMatch(line, /:Stop/);
  assert.doesNotMatch(line, /Debug/);
});

test("buildStatusBarLeft shows pending state without per-key hints", () => {
  const line = buildStatusBarLeft({
    mode: "Collect",
    state: "pending",
    episodeCount: 1,
    elapsedMs: 2_000,
    connected: true,
  });

  assert.match(line, /Pending/);
  assert.doesNotMatch(line, /Keep/);
  assert.doesNotMatch(line, /Debug/);
});
