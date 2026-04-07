import { readFile } from "node:fs/promises";
import { nowMs } from "../debug-metrics.js";
import { TypeScriptHarriRenderer } from "./ts-harri.js";
import type {
  AsciiRenderInput,
  AsciiRenderLayout,
  AsciiRenderResult,
  AsciiRendererBackend,
  AsciiRendererOptions,
  AsciiRasterDimensions,
} from "./types.js";

interface HarriWasmExports extends WebAssembly.Exports {
  readonly memory: WebAssembly.Memory;
  alloc(length: number): number;
  dealloc(ptr: number, len: number, cap: number): void;
  renderer_create(cellWidth: number, cellHeight: number): number;
  renderer_destroy(handle: number): void;
  renderer_set_glyphs(
    handle: number,
    glyphCharsPtr: number,
    glyphCharsLen: number,
    glyphVectorsPtr: number,
    glyphVectorsLen: number,
    vectorSize: number,
  ): number;
  renderer_render(
    handle: number,
    pixelsPtr: number,
    pixelsLen: number,
    width: number,
    height: number,
    columns: number,
    rows: number,
  ): number;
  renderer_output_ptr(handle: number): number;
  renderer_output_len(handle: number): number;
  renderer_sgr_change_count(handle: number): number;
  renderer_cache_hits(handle: number): number;
  renderer_cache_misses(handle: number): number;
  renderer_sample_count(handle: number): number;
  renderer_lookup_count(handle: number): number;
  last_error_ptr(): number;
  last_error_len(): number;
}

interface WasmAllocation {
  ptr: number;
  len: number;
  cap: number;
}

interface RustWasmHarriRendererOptions {
  rendererId?: string;
  label?: string;
}

const UTF8_DECODER = new TextDecoder();
let wasmExportsPromise: Promise<HarriWasmExports> | null = null;

function resolveHarriWasmUrl(): URL {
  return new URL("../../../wasm/harri-core.wasm", import.meta.url);
}

async function loadHarriWasmExports(): Promise<HarriWasmExports> {
  if (!wasmExportsPromise) {
    wasmExportsPromise = (async () => {
      const wasmBytes = await readFile(resolveHarriWasmUrl());
      const { instance } = await WebAssembly.instantiate(wasmBytes, {});
      return instance.exports as unknown as HarriWasmExports;
    })();
  }
  return await wasmExportsPromise;
}

function allocateBytes(exports: HarriWasmExports, bytes: Uint8Array): WasmAllocation {
  if (bytes.byteLength === 0) {
    return { ptr: 0, len: 0, cap: 0 };
  }
  const ptr = exports.alloc(bytes.byteLength);
  new Uint8Array(exports.memory.buffer, ptr, bytes.byteLength).set(bytes);
  return {
    ptr,
    len: bytes.byteLength,
    cap: bytes.byteLength,
  };
}

function freeAllocation(exports: HarriWasmExports, allocation: WasmAllocation): void {
  if (allocation.cap === 0) {
    return;
  }
  exports.dealloc(allocation.ptr, allocation.len, allocation.cap);
}

function readLastError(exports: HarriWasmExports): Error {
  const ptr = exports.last_error_ptr();
  const len = exports.last_error_len();
  if (len <= 0) {
    return new Error("Unknown Harri WASM error");
  }
  const bytes = new Uint8Array(exports.memory.buffer, ptr, len).slice();
  return new Error(UTF8_DECODER.decode(bytes));
}

export class RustWasmHarriRenderer implements AsciiRendererBackend {
  readonly kind = "wasm" as const;
  readonly algorithm = "shape-lookup-rust-wasm";
  readonly pixelFormat = "rgb24" as const;
  readonly id: string;
  readonly label: string;

  private readonly geometryRenderer: TypeScriptHarriRenderer;
  private exports: HarriWasmExports | null = null;
  private handle = 0;

  constructor(
    options: AsciiRendererOptions = {},
    {
      rendererId = "ts-harri",
      label = "Harri (WASM)",
    }: RustWasmHarriRendererOptions = {},
  ) {
    this.id = rendererId;
    this.label = label;
    this.geometryRenderer = new TypeScriptHarriRenderer(options);
  }

  describeRaster(layout: AsciiRenderLayout): AsciiRasterDimensions {
    return this.geometryRenderer.describeRaster(layout);
  }

  layoutForRaster(raster: AsciiRasterDimensions): AsciiRenderLayout {
    return this.geometryRenderer.layoutForRaster(raster);
  }

  async prepare(): Promise<void> {
    if (this.handle !== 0 && this.exports) {
      return;
    }

    const exports = await loadHarriWasmExports();
    const glyphPayload = await this.geometryRenderer.exportGlyphPayload();
    const handle = exports.renderer_create(glyphPayload.cellWidth, glyphPayload.cellHeight);
    if (handle === 0) {
      throw readLastError(exports);
    }

    const glyphCharsAllocation = allocateBytes(exports, glyphPayload.glyphChars);
    const glyphVectorsAllocation = allocateBytes(
      exports,
      new Uint8Array(
        glyphPayload.glyphVectors.buffer,
        glyphPayload.glyphVectors.byteOffset,
        glyphPayload.glyphVectors.byteLength,
      ),
    );
    try {
      const ok = exports.renderer_set_glyphs(
        handle,
        glyphCharsAllocation.ptr,
        glyphCharsAllocation.len,
        glyphVectorsAllocation.ptr,
        glyphVectorsAllocation.len,
        glyphPayload.vectorLength,
      );
      if (ok === 0) {
        exports.renderer_destroy(handle);
        throw readLastError(exports);
      }
    } finally {
      freeAllocation(exports, glyphCharsAllocation);
      freeAllocation(exports, glyphVectorsAllocation);
    }

    this.exports = exports;
    this.handle = handle;
  }

  async render(input: AsciiRenderInput): Promise<AsciiRenderResult> {
    await this.prepare();
    if (!this.exports || this.handle === 0) {
      throw new Error("Harri WASM renderer not initialized");
    }

    const expected = this.describeRaster(input.layout);
    if (input.width !== expected.width || input.height !== expected.height) {
      throw new Error(
        `${this.id} expected raster ${expected.width}x${expected.height}, received ` +
          `${input.width}x${input.height}`,
      );
    }

    const pixelBytes = new Uint8Array(
      input.pixels.buffer,
      input.pixels.byteOffset,
      input.pixels.byteLength,
    );
    const pixelAllocation = allocateBytes(this.exports, pixelBytes);
    const startedAtMs = nowMs();
    try {
      const ok = this.exports.renderer_render(
        this.handle,
        pixelAllocation.ptr,
        pixelAllocation.len,
        input.width,
        input.height,
        input.layout.columns,
        input.layout.rows,
      );
      if (ok === 0) {
        throw readLastError(this.exports);
      }

      const outputPtr = this.exports.renderer_output_ptr(this.handle);
      const outputLen = this.exports.renderer_output_len(this.handle);
      const outputBytes =
        outputLen > 0
          ? new Uint8Array(this.exports.memory.buffer, outputPtr, outputLen).slice()
          : new Uint8Array();
      const outputText = UTF8_DECODER.decode(outputBytes);

      return {
        backendId: this.id,
        lines: outputText.length > 0 ? outputText.split("\n") : [],
        stats: {
          rasterWidth: input.width,
          rasterHeight: input.height,
          outputColumns: input.layout.columns,
          outputRows: input.layout.rows,
          outputBytes: outputBytes.byteLength,
          cellCount: input.layout.columns * input.layout.rows,
          sampleCount: this.exports.renderer_sample_count(this.handle),
          lookupCount: this.exports.renderer_lookup_count(this.handle),
          sgrChangeCount: this.exports.renderer_sgr_change_count(this.handle),
          cacheHits: this.exports.renderer_cache_hits(this.handle),
          cacheMisses: this.exports.renderer_cache_misses(this.handle),
          timings: {
            totalMs: nowMs() - startedAtMs,
          },
        },
      };
    } finally {
      freeAllocation(this.exports, pixelAllocation);
    }
  }

  async dispose(): Promise<void> {
    if (!this.exports || this.handle === 0) {
      return;
    }
    this.exports.renderer_destroy(this.handle);
    this.handle = 0;
  }
}
