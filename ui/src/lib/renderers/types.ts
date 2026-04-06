export interface AsciiRenderLayout {
  columns: number;
  rows: number;
}

export interface AsciiRasterDimensions {
  width: number;
  height: number;
}

export interface AsciiRenderInput {
  pixels: Buffer | Uint8Array;
  width: number;
  height: number;
  layout: AsciiRenderLayout;
}

export interface AsciiRenderTimings {
  totalMs: number;
  sampleMs?: number;
  lookupMs?: number;
  assembleMs?: number;
  ansiMs?: number;
  adapterMs?: number;
}

export interface AsciiRenderStats {
  rasterWidth: number;
  rasterHeight: number;
  outputColumns: number;
  outputRows: number;
  outputBytes: number;
  cellCount: number;
  sampleCount?: number;
  lookupCount?: number;
  sgrChangeCount?: number;
  cacheHits?: number;
  cacheMisses?: number;
  timings: AsciiRenderTimings;
}

export interface AsciiRenderResult {
  backendId: string;
  lines: string[];
  stats: AsciiRenderStats;
}

export interface AsciiRendererBackend {
  id: string;
  label: string;
  kind: "typescript" | "rust" | "wasm";
  algorithm: string;
  describeRaster(layout: AsciiRenderLayout): AsciiRasterDimensions;
  layoutForRaster(raster: AsciiRasterDimensions): AsciiRenderLayout;
  prepare?(): Promise<void>;
  render(input: AsciiRenderInput): Promise<AsciiRenderResult>;
  dispose?(): Promise<void>;
}

export function assertExpectedRaster(
  backend: AsciiRendererBackend,
  width: number,
  height: number,
  layout: AsciiRenderLayout,
): void {
  const expected = backend.describeRaster(layout);
  if (width !== expected.width || height !== expected.height) {
    throw new Error(
      `${backend.id} expected raster ${expected.width}x${expected.height}, ` +
        `received ${width}x${height}`,
    );
  }
}

export function measureOutputBytes(lines: string[]): number {
  if (lines.length === 0) {
    return 0;
  }

  let total = lines.length - 1; // account for newline separators
  for (const line of lines) {
    total += Buffer.byteLength(line, "utf8");
  }
  return total;
}
