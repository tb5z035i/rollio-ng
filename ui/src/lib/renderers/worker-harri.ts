import { existsSync } from "node:fs";
import { Worker } from "node:worker_threads";
import { fileURLToPath } from "node:url";
import { incrementGauge, nowMs, setGauge } from "../debug-metrics.js";
import { TypeScriptHarriRenderer } from "./ts-harri.js";
import type {
  AsciiRenderInput,
  AsciiRenderLayout,
  AsciiRenderResult,
  AsciiRendererBackend,
  AsciiRendererOptions,
  AsciiRasterDimensions,
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

type HarriWorkerResponse =
  | { type: "ready"; threadId: number }
  | { type: "renderResult"; requestId: number; result: AsciiRenderResult }
  | { type: "error"; requestId?: number; message: string; stack?: string }
  | { type: "log"; level: "info" | "warn" | "error"; message: string };

interface PendingRenderRequest {
  resolve: (result: AsciiRenderResult) => void;
  reject: (reason?: unknown) => void;
  startedAtMs: number;
  width: number;
  height: number;
  layout: AsciiRenderLayout;
}

class RendererDisposedError extends Error {
  constructor() {
    super("Harri worker renderer disposed");
    this.name = "RendererDisposedError";
  }
}

function resolveWorkerModuleUrl(): URL {
  if (!import.meta.url.endsWith(".ts")) {
    return new URL("./ts-harri.worker.js", import.meta.url);
  }

  // When the parent code is running straight from `src/` (for example under
  // `tsx --test`), prefer the compiled worker from `dist/` if it exists so the
  // worker executes the same module graph as the shipped app.
  const builtWorkerUrl = new URL("../../../dist/lib/renderers/ts-harri.worker.js", import.meta.url);
  if (existsSync(fileURLToPath(builtWorkerUrl))) {
    return builtWorkerUrl;
  }

  return new URL("./ts-harri.worker.ts", import.meta.url);
}

function createError(message: string, stack?: string): Error {
  const error = new Error(message);
  if (stack) {
    error.stack = stack;
  }
  return error;
}

function normalizeError(error: unknown): Error {
  return error instanceof Error ? error : new Error(String(error));
}

function truthyEnv(name: string): boolean {
  const value = process.env[name]?.trim().toLowerCase();
  return value === "1" || value === "true" || value === "yes" || value === "on";
}

function formatLayout(layout: AsciiRenderLayout): string {
  return `${layout.columns}x${layout.rows}`;
}

function formatRaster(width: number, height: number): string {
  return `${width}x${height}`;
}

const HARRI_WORKER_GAUGE_PREFIX = "stream.harri_worker";
const HARRI_WORKER_DEBUG = truthyEnv("ROLLIO_DEBUG_HARRI_WORKER");

function cloneTransferablePixels(pixels: Buffer | Uint8Array): Uint8Array {
  if (
    !Buffer.isBuffer(pixels) &&
    pixels.byteOffset === 0 &&
    pixels.byteLength === pixels.buffer.byteLength &&
    !(pixels.buffer instanceof SharedArrayBuffer)
  ) {
    return new Uint8Array(pixels.buffer, 0, pixels.byteLength);
  }

  const copy = new Uint8Array(pixels.byteLength);
  copy.set(pixels);
  return copy;
}

export class WorkerThreadHarriRenderer implements AsciiRendererBackend {
  readonly id = "ts-harri";
  readonly label = "Harri (Worker)";
  readonly pixelFormat = "rgb24" as const;

  private readonly geometryRenderer: TypeScriptHarriRenderer;
  private fallbackRenderer: TypeScriptHarriRenderer | null = null;
  private worker: Worker | null = null;
  private readyPromise: Promise<void> | null = null;
  private readyResolver:
    | {
        resolve: () => void;
        reject: (reason?: unknown) => void;
      }
    | null = null;
  private readonly pendingRequests = new Map<number, PendingRenderRequest>();
  private nextRequestId = 1;
  private useFallback = false;
  private disposed = false;
  private terminatingWorker = false;

  constructor(private readonly options: AsciiRendererOptions = {}) {
    this.geometryRenderer = new TypeScriptHarriRenderer(options);
    this.setWorkerState("idle");
    setGauge(`${HARRI_WORKER_GAUGE_PREFIX}.pending_requests`, 0);
    setGauge(`${HARRI_WORKER_GAUGE_PREFIX}.last_error`, "None");
    setGauge(`${HARRI_WORKER_GAUGE_PREFIX}.last_log`, "None");
    setGauge(`${HARRI_WORKER_GAUGE_PREFIX}.thread_id`, "n/a");
    setGauge(`${HARRI_WORKER_GAUGE_PREFIX}.last_request_id`, "n/a");
    setGauge(`${HARRI_WORKER_GAUGE_PREFIX}.last_layout`, "n/a");
    setGauge(`${HARRI_WORKER_GAUGE_PREFIX}.last_raster`, "n/a");
  }

  get kind(): "typescript" | "worker" {
    return this.useFallback ? "typescript" : "worker";
  }

  get algorithm(): string {
    return this.useFallback ? "shape-lookup-typescript-fallback" : "shape-lookup-rust-wasm-worker";
  }

  describeRaster(layout: AsciiRenderLayout): AsciiRasterDimensions {
    return this.geometryRenderer.describeRaster(layout);
  }

  layoutForRaster(raster: AsciiRasterDimensions): AsciiRenderLayout {
    return this.geometryRenderer.layoutForRaster(raster);
  }

  async prepare(): Promise<void> {
    if (this.useFallback) {
      await this.getFallbackRenderer().prepare?.();
      return;
    }

    try {
      await this.ensureWorkerReady();
    } catch (error) {
      this.activateFallback(normalizeError(error), "prepare");
      await this.getFallbackRenderer().prepare?.();
    }
  }

  async render(input: AsciiRenderInput): Promise<AsciiRenderResult> {
    if (this.useFallback) {
      return await this.getFallbackRenderer().render(input);
    }

    try {
      await this.ensureWorkerReady();
      return await this.renderInWorker(input);
    } catch (error) {
      if (this.useFallback) {
        return await this.getFallbackRenderer().render(input);
      }
      throw error;
    }
  }

  async dispose(): Promise<void> {
    this.disposed = true;
    this.setWorkerState("disposed");
    setGauge(`${HARRI_WORKER_GAUGE_PREFIX}.pending_requests`, 0);
    if (!this.worker) {
      return;
    }

    this.terminatingWorker = true;
    const worker = this.worker;
    this.clearWorkerState();
    this.rejectAllPending(new RendererDisposedError());
    await worker.terminate();
  }

  private getFallbackRenderer(): TypeScriptHarriRenderer {
    if (!this.fallbackRenderer) {
      this.fallbackRenderer = new TypeScriptHarriRenderer(this.options);
    }
    return this.fallbackRenderer;
  }

  private async ensureWorkerReady(): Promise<void> {
    if (this.disposed) {
      throw new Error("Harri worker renderer already disposed");
    }

    if (this.useFallback) {
      throw new Error("Harri worker renderer running in fallback mode");
    }

    if (!this.readyPromise) {
      this.readyPromise = this.startWorker();
    }

    await this.readyPromise;
  }

  private async startWorker(): Promise<void> {
    const workerUrl = resolveWorkerModuleUrl();
    const startStartedAtMs = nowMs();
    this.setWorkerState("starting");
    incrementGauge(`${HARRI_WORKER_GAUGE_PREFIX}.starts`);
    setGauge(`${HARRI_WORKER_GAUGE_PREFIX}.module`, workerUrl.href);
    const worker = new Worker(workerUrl);
    this.worker = worker;
    this.terminatingWorker = false;

    worker.on("message", (message: HarriWorkerResponse) => {
      this.handleWorkerMessage(message);
    });
    worker.on("error", (error) => {
      this.handleWorkerFailure(error);
    });
    worker.on("exit", (code) => {
      if (!this.terminatingWorker && !this.disposed && code !== 0) {
        this.handleWorkerFailure(
          new Error(`Harri worker exited unexpectedly with code ${code}`),
        );
      } else {
        this.clearWorkerState();
      }
    });

    const readyPromise = new Promise<void>((resolve, reject) => {
      this.readyResolver = { resolve, reject };
    });
    void readyPromise.catch(() => undefined);

    const initMessage: HarriWorkerRequest = {
      type: "init",
      options: this.options,
    };
    worker.postMessage(initMessage);

    await readyPromise;
    setGauge(
      `${HARRI_WORKER_GAUGE_PREFIX}.last_start_ms`,
      nowMs() - startStartedAtMs,
    );
  }

  private async renderInWorker(input: AsciiRenderInput): Promise<AsciiRenderResult> {
    const worker = this.worker;
    if (!worker) {
      throw new Error("Harri worker is not available");
    }

    const pixels = cloneTransferablePixels(input.pixels);
    const requestId = this.nextRequestId++;
    const startedAtMs = nowMs();

    const requestPromise = new Promise<AsciiRenderResult>((resolve, reject) => {
      this.pendingRequests.set(requestId, {
        resolve,
        reject,
        startedAtMs,
        width: input.width,
        height: input.height,
        layout: input.layout,
      });
    });
    // Attach a local rejection handler immediately so rapid dispose/switch
    // cycles do not surface as unhandled rejections before the caller awaits.
    void requestPromise.catch(() => undefined);
    incrementGauge(`${HARRI_WORKER_GAUGE_PREFIX}.submitted`);
    setGauge(`${HARRI_WORKER_GAUGE_PREFIX}.last_request_id`, requestId);
    setGauge(
      `${HARRI_WORKER_GAUGE_PREFIX}.last_layout`,
      formatLayout(input.layout),
    );
    setGauge(
      `${HARRI_WORKER_GAUGE_PREFIX}.last_raster`,
      formatRaster(input.width, input.height),
    );
    setGauge(
      `${HARRI_WORKER_GAUGE_PREFIX}.pending_requests`,
      this.pendingRequests.size,
    );
    this.setWorkerState("busy");

    const message: HarriWorkerRequest = {
      type: "render",
      requestId,
      input: {
        pixels,
        width: input.width,
        height: input.height,
        layout: input.layout,
      },
    };
    const transferBuffer = pixels.buffer;
    if (transferBuffer instanceof ArrayBuffer) {
      worker.postMessage(message, [transferBuffer]);
    } else {
      worker.postMessage(message);
    }

    return await requestPromise;
  }

  private handleWorkerMessage(message: HarriWorkerResponse): void {
    switch (message.type) {
      case "ready":
        setGauge(`${HARRI_WORKER_GAUGE_PREFIX}.thread_id`, message.threadId);
        setGauge(`${HARRI_WORKER_GAUGE_PREFIX}.last_error`, "None");
        incrementGauge(`${HARRI_WORKER_GAUGE_PREFIX}.ready`);
        this.setWorkerState("ready");
        this.readyResolver?.resolve();
        this.readyResolver = null;
        return;
      case "renderResult": {
        const pending = this.pendingRequests.get(message.requestId);
        if (!pending) {
          return;
        }
        this.pendingRequests.delete(message.requestId);
        const roundtripMs = nowMs() - pending.startedAtMs;
        const adapterMs = Math.max(0, roundtripMs - message.result.stats.timings.totalMs);
        incrementGauge(`${HARRI_WORKER_GAUGE_PREFIX}.completed`);
        setGauge(
          `${HARRI_WORKER_GAUGE_PREFIX}.pending_requests`,
          this.pendingRequests.size,
        );
        setGauge(`${HARRI_WORKER_GAUGE_PREFIX}.last_roundtrip_ms`, roundtripMs);
        setGauge(
          `${HARRI_WORKER_GAUGE_PREFIX}.last_compute_ms`,
          message.result.stats.timings.totalMs,
        );
        setGauge(`${HARRI_WORKER_GAUGE_PREFIX}.last_adapter_ms`, adapterMs);
        this.setWorkerState("ready");
        pending.resolve({
          ...message.result,
          stats: {
            ...message.result.stats,
            timings: {
              ...message.result.stats.timings,
              adapterMs,
            },
          },
        });
        return;
      }
      case "error": {
        const error = createError(message.message, message.stack);
        if (message.requestId === undefined) {
          this.handleWorkerFailure(error);
          return;
        }
        const pending = this.pendingRequests.get(message.requestId);
        if (!pending) {
          return;
        }
        this.pendingRequests.delete(message.requestId);
        incrementGauge(`${HARRI_WORKER_GAUGE_PREFIX}.request_errors`);
        setGauge(
          `${HARRI_WORKER_GAUGE_PREFIX}.pending_requests`,
          this.pendingRequests.size,
        );
        setGauge(
          `${HARRI_WORKER_GAUGE_PREFIX}.last_error`,
          `request ${message.requestId}: ${error.message}`,
        );
        this.setWorkerState(this.useFallback ? "fallback" : "ready");
        pending.reject(error);
        return;
      }
      case "log":
        this.handleWorkerLog(message.level, message.message);
        return;
    }
  }

  private handleWorkerFailure(error: Error): void {
    if (this.disposed) {
      return;
    }

    this.activateFallback(error, "worker failure");
    this.readyResolver?.reject(error);
    this.readyResolver = null;
    this.readyPromise = null;

    const worker = this.worker;
    this.clearWorkerState();
    this.rejectAllPending(error);
    void worker?.terminate().catch(() => undefined);
  }

  private rejectAllPending(error: Error): void {
    for (const pending of this.pendingRequests.values()) {
      pending.reject(error);
    }
    this.pendingRequests.clear();
    setGauge(`${HARRI_WORKER_GAUGE_PREFIX}.pending_requests`, 0);
  }

  private clearWorkerState(): void {
    this.worker = null;
    this.readyPromise = null;
    this.readyResolver = null;
    this.terminatingWorker = false;
    setGauge(`${HARRI_WORKER_GAUGE_PREFIX}.thread_id`, "n/a");
  }

  private setWorkerState(state: string): void {
    setGauge(`${HARRI_WORKER_GAUGE_PREFIX}.mode`, this.useFallback ? "fallback" : "worker");
    setGauge(`${HARRI_WORKER_GAUGE_PREFIX}.state`, state);
  }

  private activateFallback(error: Error, context: string): void {
    this.useFallback = true;
    incrementGauge(`${HARRI_WORKER_GAUGE_PREFIX}.fallbacks`);
    setGauge(`${HARRI_WORKER_GAUGE_PREFIX}.last_error`, `${context}: ${error.message}`);
    this.handleWorkerLog("warn", `fallback (${context}): ${error.message}`);
    this.setWorkerState("fallback");
  }

  private handleWorkerLog(level: "info" | "warn" | "error", message: string): void {
    const formatted = `${level}: ${message}`;
    incrementGauge(`${HARRI_WORKER_GAUGE_PREFIX}.logs`);
    setGauge(`${HARRI_WORKER_GAUGE_PREFIX}.last_log`, formatted);
    if (HARRI_WORKER_DEBUG) {
      console.error(`[harri-worker] ${formatted}`);
    }
  }
}
