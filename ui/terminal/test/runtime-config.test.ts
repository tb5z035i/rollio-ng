import assert from "node:assert/strict";
import test from "node:test";
import { resolveRuntimeConfig } from "../src/runtime-config.js";

test("resolveRuntimeConfig falls back to the default websocket endpoint", () => {
  const runtimeConfig = resolveRuntimeConfig([], {});
  assert.equal(runtimeConfig.websocketUrl, "ws://localhost:9090");
  assert.equal(runtimeConfig.asciiRendererId, "native-rust");
});

test("resolveRuntimeConfig prefers environment configuration", () => {
  const runtimeConfig = resolveRuntimeConfig([], {
    ROLLIO_VISUALIZER_WS: "ws://127.0.0.1:9911",
    ROLLIO_ASCII_RENDERER: "native-rust",
  });
  assert.equal(runtimeConfig.websocketUrl, "ws://127.0.0.1:9911");
  assert.equal(runtimeConfig.asciiRendererId, "native-rust");
});

test("resolveRuntimeConfig lets CLI flags override environment values", () => {
  const runtimeConfig = resolveRuntimeConfig(
    ["--ws", "ws://127.0.0.1:9922", "--renderer", "ts-half-block"],
    {
      ROLLIO_VISUALIZER_WS: "ws://127.0.0.1:9911",
      ROLLIO_ASCII_RENDERER: "native-rust",
    },
  );
  assert.equal(runtimeConfig.websocketUrl, "ws://127.0.0.1:9922");
  assert.equal(runtimeConfig.asciiRendererId, "ts-half-block");
});
