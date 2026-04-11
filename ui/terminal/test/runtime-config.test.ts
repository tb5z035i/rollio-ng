import assert from "node:assert/strict";
import test from "node:test";
import { resolveRuntimeConfig } from "../src/runtime-config.js";

test("resolveRuntimeConfig falls back to the default websocket endpoint", () => {
  const runtimeConfig = resolveRuntimeConfig([], {});
  assert.equal(runtimeConfig.websocketUrl, "ws://localhost:9090");
  assert.equal(runtimeConfig.asciiRendererId, "native-rust");
  assert.deepEqual(runtimeConfig.episodeKeyBindings, {
    startKey: "s",
    stopKey: "e",
    keepKey: "k",
    discardKey: "x",
  });
});

test("resolveRuntimeConfig prefers environment configuration", () => {
  const runtimeConfig = resolveRuntimeConfig([], {
    ROLLIO_VISUALIZER_WS: "ws://127.0.0.1:9911",
    ROLLIO_ASCII_RENDERER: "native-rust",
    ROLLIO_UI_START_KEY: "a",
    ROLLIO_UI_STOP_KEY: "b",
    ROLLIO_UI_KEEP_KEY: "c",
    ROLLIO_UI_DISCARD_KEY: "v",
  });
  assert.equal(runtimeConfig.websocketUrl, "ws://127.0.0.1:9911");
  assert.equal(runtimeConfig.asciiRendererId, "native-rust");
  assert.equal(runtimeConfig.episodeKeyBindings.startKey, "a");
  assert.equal(runtimeConfig.episodeKeyBindings.stopKey, "b");
  assert.equal(runtimeConfig.episodeKeyBindings.keepKey, "c");
  assert.equal(runtimeConfig.episodeKeyBindings.discardKey, "v");
});

test("resolveRuntimeConfig lets CLI flags override environment values", () => {
  const runtimeConfig = resolveRuntimeConfig(
    [
      "--ws",
      "ws://127.0.0.1:9922",
      "--renderer",
      "ts-half-block",
      "--start-key",
      "j",
      "--stop-key",
      "l",
      "--keep-key",
      "u",
      "--discard-key",
      "i",
    ],
    {
      ROLLIO_VISUALIZER_WS: "ws://127.0.0.1:9911",
      ROLLIO_ASCII_RENDERER: "native-rust",
      ROLLIO_UI_START_KEY: "a",
      ROLLIO_UI_STOP_KEY: "b",
      ROLLIO_UI_KEEP_KEY: "c",
      ROLLIO_UI_DISCARD_KEY: "v",
    },
  );
  assert.equal(runtimeConfig.websocketUrl, "ws://127.0.0.1:9922");
  assert.equal(runtimeConfig.asciiRendererId, "ts-half-block");
  assert.deepEqual(runtimeConfig.episodeKeyBindings, {
    startKey: "j",
    stopKey: "l",
    keepKey: "u",
    discardKey: "i",
  });
});

test("resolveRuntimeConfig accepts the native-rust-color preset", () => {
  const runtimeConfig = resolveRuntimeConfig([], {
    ROLLIO_ASCII_RENDERER: "native-rust-color",
  });
  assert.equal(runtimeConfig.asciiRendererId, "native-rust-color");
});
