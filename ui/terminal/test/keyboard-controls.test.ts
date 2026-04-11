import assert from "node:assert/strict";
import test from "node:test";
import { actionForInput } from "../src/lib/controls.js";

const episodeKeyBindings = {
  startKey: "s",
  stopKey: "e",
  keepKey: "k",
  discardKey: "x",
};

test("actionForInput preserves debug and renderer shortcuts", () => {
  assert.equal(actionForInput("d", episodeKeyBindings), "toggle_debug");
  assert.equal(actionForInput("r", episodeKeyBindings), "cycle_renderer");
});

test("actionForInput maps episode lifecycle keys", () => {
  assert.equal(actionForInput("s", episodeKeyBindings), "episode_start");
  assert.equal(actionForInput("e", episodeKeyBindings), "episode_stop");
  assert.equal(actionForInput("k", episodeKeyBindings), "episode_keep");
  assert.equal(actionForInput("x", episodeKeyBindings), "episode_discard");
});

test("actionForInput ignores unrelated keys", () => {
  assert.equal(actionForInput("q", episodeKeyBindings), null);
});
