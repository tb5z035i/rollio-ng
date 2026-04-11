import assert from "node:assert/strict";
import test from "node:test";
import {
  buildStatusBarLeft,
  formatElapsedMs,
} from "../src/components/StatusBar.js";

const episodeKeyBindings = {
  startKey: "s",
  stopKey: "e",
  keepKey: "k",
  discardKey: "x",
};

test("formatElapsedMs renders mm:ss timers", () => {
  assert.equal(formatElapsedMs(5_250), "0:05");
  assert.equal(formatElapsedMs(125_000), "2:05");
});

test("buildStatusBarLeft shows recording timer and stop hint", () => {
  const line = buildStatusBarLeft({
    mode: "Collect",
    state: "recording",
    episodeCount: 2,
    elapsedMs: 5_250,
    episodeKeyBindings,
    connected: true,
    debugEnabled: false,
    rendererLabel: "native-rust",
  });

  assert.match(line, /Recording 0:05/);
  assert.match(line, /e:Stop/);
  assert.match(line, /Ep: 2/);
});

test("buildStatusBarLeft shows keep and discard prompts when pending", () => {
  const line = buildStatusBarLeft({
    mode: "Collect",
    state: "pending",
    episodeCount: 1,
    elapsedMs: 2_000,
    episodeKeyBindings,
    connected: true,
    debugEnabled: true,
  });

  assert.match(line, /Pending/);
  assert.match(line, /k:Keep x:Discard/);
  assert.match(line, /d:Debug On/);
});
