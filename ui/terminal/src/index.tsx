import React from "react";
import { render } from "ink";
import { App } from "./App.js";
import { resolveRuntimeConfig } from "./runtime-config.js";

const runtimeConfig = resolveRuntimeConfig();

const app = render(
  <App
    websocketUrl={runtimeConfig.websocketUrl}
    initialAsciiRendererId={runtimeConfig.asciiRendererId}
    episodeKeyBindings={runtimeConfig.episodeKeyBindings}
  />,
  {
  maxFps: 60,
  incrementalRendering: true,
  },
);

let shutdownRequested = false;

function restoreTerminalState() {
  if (process.stdin.isTTY) {
    try {
      process.stdin.setRawMode(false);
    } catch {
      // Ignore restoration failures during shutdown.
    }
  }

  try {
    process.stdout.write("\x1B[?25h");
  } catch {
    // Ignore cursor restoration failures during shutdown.
  }
}

function shutdown(code: number) {
  if (shutdownRequested) {
    process.exit(code);
  }
  shutdownRequested = true;

  try {
    app.unmount();
  } catch {
    // Ignore unmount failures while forcing shutdown.
  }

  restoreTerminalState();
  process.exit(code);
}

process.once("SIGINT", () => {
  shutdown(130);
});

process.once("SIGTERM", () => {
  shutdown(143);
});

process.once("exit", () => {
  restoreTerminalState();
});

void app.waitUntilExit().finally(() => {
  restoreTerminalState();
});
