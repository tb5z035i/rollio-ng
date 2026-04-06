import { performance } from "node:perf_hooks";
import { parentPort, threadId } from "node:worker_threads";
import type {
  AsciiRenderInput,
  AsciiRenderLayout,
  AsciiRenderResult,
  AsciiRendererOptions,
} from "./types.js";

type HarriWorkerRequest =
  | {
      type: "init";
      options: AsciiRendererOptions;
    }
  | {
      type: "render";
      requestId: number;
      input: {
        pixels: Uint8Array;
        width: number;
        height: number;
        layout: AsciiRenderLayout;
      };
    };

type HarriWorkerLogLevel = "info" | "warn" | "error";

interface HarriRendererInstance {
  prepare?(): Promise<void>;
  render(input: AsciiRenderInput): Promise<AsciiRenderResult>;
}

type HarriRendererConstructor = new (
  options: AsciiRendererOptions,
) => HarriRendererInstance;

function postError(requestId: number | undefined, error: unknown): void {
  const message = error instanceof Error ? error.message : String(error);
  const stack = error instanceof Error ? error.stack : undefined;
  parentPort?.postMessage({
    type: "error",
    requestId,
    message,
    stack,
  });
}

function postLog(level: HarriWorkerLogLevel, message: string): void {
  parentPort?.postMessage({
    type: "log",
    level,
    message,
  });
}

function formatCellGeometry(options: AsciiRendererOptions): string {
  const geometry = options.cellGeometry;
  if (!geometry) {
    return "default";
  }
  return `${geometry.pixelWidth}x${geometry.pixelHeight}`;
}

function shouldLogRender(renderCount: number): boolean {
  return renderCount <= 3 || renderCount % 120 === 0;
}

let rendererConstructorPromise: Promise<HarriRendererConstructor> | null = null;

async function loadRendererConstructor(): Promise<HarriRendererConstructor> {
  if (!rendererConstructorPromise) {
    rendererConstructorPromise = import(
      import.meta.url.endsWith(".ts") ? "./ts-harri.ts" : "./ts-harri.js"
    ).then((module) => module.TypeScriptHarriRenderer);
  }
  return await rendererConstructorPromise;
}

if (!parentPort) {
  throw new Error("Harri worker requires a parentPort");
}

let renderer: HarriRendererInstance | null = null;
let renderCount = 0;

parentPort.on("message", async (message: HarriWorkerRequest) => {
  try {
    switch (message.type) {
      case "init": {
        const prepareStartedAtMs = performance.now();
        postLog("info", `init thread=${threadId} cell=${formatCellGeometry(message.options)}`);
        const TypeScriptHarriRenderer = await loadRendererConstructor();
        renderer = new TypeScriptHarriRenderer(message.options);
        await renderer.prepare?.();
        parentPort?.postMessage({ type: "ready", threadId });
        postLog(
          "info",
          `ready thread=${threadId} prepare=${(performance.now() - prepareStartedAtMs).toFixed(2)}ms`,
        );
        return;
      }
      case "render": {
        if (!renderer) {
          throw new Error("Harri worker received render before initialization");
        }
        renderCount += 1;
        if (shouldLogRender(renderCount)) {
          postLog(
            "info",
            `render#${renderCount} req=${message.requestId} raster=${message.input.width}x${message.input.height} layout=${message.input.layout.columns}x${message.input.layout.rows}`,
          );
        }
        const result = await renderer.render({
          pixels: message.input.pixels,
          width: message.input.width,
          height: message.input.height,
          layout: message.input.layout,
        });
        if (shouldLogRender(renderCount)) {
          postLog(
            "info",
            `done#${renderCount} req=${message.requestId} total=${result.stats.timings.totalMs.toFixed(2)}ms cache=${result.stats.cacheHits ?? 0}/${result.stats.cacheMisses ?? 0}`,
          );
        }
        parentPort?.postMessage({
          type: "renderResult",
          requestId: message.requestId,
          result,
        });
        return;
      }
    }
  } catch (error) {
    const prefix = message.type === "render" ? `req=${message.requestId}` : "init";
    postLog(
      "error",
      `${prefix} ${error instanceof Error ? error.message : String(error)}`,
    );
    postError(message.type === "render" ? message.requestId : undefined, error);
  }
});
