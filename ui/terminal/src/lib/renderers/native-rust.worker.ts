import { parentPort } from "node:worker_threads";
import { loadNativeAsciiAddon, type NativeAsciiAddonModule } from "./native-rust-addon.js";
import type { AsciiRenderLayout, AsciiRenderResult } from "./types.js";

type NativeAsciiWorkerRequest =
  | {
      type: "init";
      geometry: {
        cellWidth: number;
        cellHeight: number;
      };
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

type NativeAsciiWorkerResponse =
  | { type: "ready" }
  | { type: "renderResult"; requestId: number; result: AsciiRenderResult }
  | { type: "error"; requestId?: number; message: string; stack?: string };

type NativeAsciiRendererInstance = InstanceType<NativeAsciiAddonModule["NativeAsciiRenderer"]>;

function postError(requestId: number | undefined, error: unknown): void {
  const message = error instanceof Error ? error.message : String(error);
  const stack = error instanceof Error ? error.stack : undefined;
  const response: NativeAsciiWorkerResponse = {
    type: "error",
    requestId,
    message,
    stack,
  };
  parentPort?.postMessage(response);
}

function mapRenderResult(
  nativeResult: ReturnType<NativeAsciiRendererInstance["render"]>,
  width: number,
  height: number,
  layout: AsciiRenderLayout,
): AsciiRenderResult {
  return {
    backendId: "native-rust",
    lines: nativeResult.lines,
    stats: {
      rasterWidth: width,
      rasterHeight: height,
      outputColumns: layout.columns,
      outputRows: layout.rows,
      outputBytes: nativeResult.stats.outputBytes,
      cellCount: nativeResult.stats.cellCount,
      sampleCount: nativeResult.stats.sampleCount,
      lookupCount: nativeResult.stats.lookupCount,
      sgrChangeCount: nativeResult.stats.sgrChangeCount,
      cacheHits: nativeResult.stats.cacheHits,
      cacheMisses: nativeResult.stats.cacheMisses,
      timings: {
        totalMs: nativeResult.stats.totalMs,
        sampleMs: nativeResult.stats.sampleMs,
        lookupMs: nativeResult.stats.lookupMs,
        assembleMs: nativeResult.stats.assembleMs,
      },
    },
  };
}

if (!parentPort) {
  throw new Error("Native ASCII worker requires a parentPort");
}

const addon = loadNativeAsciiAddon();
let renderer: NativeAsciiRendererInstance | null = null;

parentPort.on("message", (message: NativeAsciiWorkerRequest) => {
  try {
    switch (message.type) {
      case "init": {
        renderer = new addon.NativeAsciiRenderer(
          message.geometry.cellWidth,
          message.geometry.cellHeight,
        );
        const response: NativeAsciiWorkerResponse = { type: "ready" };
        parentPort?.postMessage(response);
        return;
      }
      case "render": {
        if (!renderer) {
          throw new Error("Native ASCII worker received render before initialization");
        }
        const pixels = Buffer.from(
          message.input.pixels.buffer,
          message.input.pixels.byteOffset,
          message.input.pixels.byteLength,
        );
        const response: NativeAsciiWorkerResponse = {
          type: "renderResult",
          requestId: message.requestId,
          result: mapRenderResult(
            renderer.render(
              pixels,
              message.input.width,
              message.input.height,
              message.input.layout.columns,
              message.input.layout.rows,
            ),
            message.input.width,
            message.input.height,
            message.input.layout,
          ),
        };
        parentPort?.postMessage(response);
        return;
      }
    }
  } catch (error) {
    postError(message.type === "render" ? message.requestId : undefined, error);
  }
});
