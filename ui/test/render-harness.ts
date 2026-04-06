import { readFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import sharp from "sharp";
import { nowMs } from "../src/lib/debug-metrics.js";
import {
  ASCII_RENDERER_IDS,
  createAsciiRendererBackend,
  type AsciiRenderLayout,
  type AsciiRendererBackend,
  type AsciiRendererId,
  type AsciiRasterDimensions,
} from "../src/lib/renderers/index.js";

export interface FixtureSpec {
  name: string;
  path: string;
}

export interface BenchmarkCase {
  name: string;
  layout: AsciiRenderLayout;
}

export interface RasterBenchmarkCase {
  name: string;
  raster: AsciiRasterDimensions;
}

export interface RenderMeasurement {
  fixture: string;
  caseName: string;
  backendId: string;
  backendLabel: string;
  rasterWidth: number;
  rasterHeight: number;
  outputColumns: number;
  outputRows: number;
  resizeMs: number;
  renderCallMs: number;
  backendMs: number;
  sampleMs?: number;
  lookupMs?: number;
  assembleMs?: number;
  ansiMs?: number;
  adapterMs?: number;
  outputBytes: number;
}

const FIXTURES_DIR = path.resolve(
  fileURLToPath(new URL(".", import.meta.url)),
  "./fixtures",
);

export const FIXTURES: FixtureSpec[] = [
  {
    name: "diagonal-edges",
    path: path.join(FIXTURES_DIR, "diagonal-edges.svg"),
  },
  {
    name: "gradient-panels",
    path: path.join(FIXTURES_DIR, "gradient-panels.svg"),
  },
];

export const BENCHMARK_CASES: BenchmarkCase[] = [
  {
    name: "small",
    layout: { columns: 32, rows: 12 },
  },
  {
    name: "medium",
    layout: { columns: 64, rows: 20 },
  },
  {
    name: "large",
    layout: { columns: 96, rows: 28 },
  },
];

export const CLI_RASTER_CASES: RasterBenchmarkCase[] = [
  {
    name: "qvga",
    raster: { width: 320, height: 240 },
  },
  {
    name: "vga",
    raster: { width: 640, height: 480 },
  },
  {
    name: "hd",
    raster: { width: 1280, height: 720 },
  },
];

export async function createBackends(
  ids: readonly AsciiRendererId[] = ASCII_RENDERER_IDS,
): Promise<AsciiRendererBackend[]> {
  const backends = ids.map((id) => createAsciiRendererBackend(id));
  for (const backend of backends) {
    await backend.prepare?.();
    await warmBackend(backend);
  }
  return backends;
}

export async function disposeBackends(
  backends: readonly AsciiRendererBackend[],
): Promise<void> {
  for (const backend of backends) {
    await backend.dispose?.();
  }
}

export async function benchmarkMatrix(
  backends: readonly AsciiRendererBackend[],
  fixtures: readonly FixtureSpec[] = FIXTURES,
  cases: readonly BenchmarkCase[] = BENCHMARK_CASES,
): Promise<RenderMeasurement[]> {
  const measurements: RenderMeasurement[] = [];

  for (const fixture of fixtures) {
    const fixtureBytes = await readFile(fixture.path);
    for (const benchmarkCase of cases) {
      for (const backend of backends) {
        measurements.push(
          await benchmarkBackendForFixture(backend, fixture.name, fixtureBytes, benchmarkCase),
        );
      }
    }
  }

  return measurements;
}

export async function benchmarkBackendForFixture(
  backend: AsciiRendererBackend,
  fixtureName: string,
  fixtureBytes: Buffer,
  benchmarkCase: BenchmarkCase,
): Promise<RenderMeasurement> {
  const raster = backend.describeRaster(benchmarkCase.layout);

  const resizeStartMs = nowMs();
  const { data, info } = await sharp(fixtureBytes)
    .resize(raster.width, raster.height, {
      fit: "contain",
      background: { r: 0, g: 0, b: 0, alpha: 1 },
      kernel: sharp.kernel.nearest,
    })
    .removeAlpha()
    .raw()
    .toBuffer({ resolveWithObject: true });
  const resizeMs = nowMs() - resizeStartMs;

  const renderStartMs = nowMs();
  const result = await backend.render({
    pixels: data,
    width: info.width,
    height: info.height,
    layout: benchmarkCase.layout,
  });
  const renderCallMs = nowMs() - renderStartMs;

  return {
    fixture: fixtureName,
    caseName: benchmarkCase.name,
    backendId: backend.id,
    backendLabel: backend.label,
    rasterWidth: info.width,
    rasterHeight: info.height,
    outputColumns: result.stats.outputColumns,
    outputRows: result.stats.outputRows,
    resizeMs,
    renderCallMs,
    backendMs: result.stats.timings.totalMs,
    sampleMs: result.stats.timings.sampleMs,
    lookupMs: result.stats.timings.lookupMs,
    assembleMs: result.stats.timings.assembleMs,
    ansiMs: result.stats.timings.ansiMs,
    adapterMs: result.stats.timings.adapterMs,
    outputBytes: result.stats.outputBytes,
  };
}

export async function benchmarkBackendForRasterCase(
  backend: AsciiRendererBackend,
  fixtureName: string,
  fixtureBytes: Buffer,
  rasterCase: RasterBenchmarkCase,
): Promise<RenderMeasurement> {
  return await benchmarkBackendForFixture(
    backend,
    fixtureName,
    fixtureBytes,
    {
      name: rasterCase.name,
      layout: backend.layoutForRaster(rasterCase.raster),
    },
  );
}

export function visibleWidth(line: string): number {
  return stripAnsi(line).length;
}

export function stripAnsi(text: string): string {
  return text.replace(/\x1b\[[0-9;]*m/g, "");
}

async function warmBackend(backend: AsciiRendererBackend): Promise<void> {
  const layout = { columns: 4, rows: 2 };
  const raster = backend.describeRaster(layout);
  await backend.render({
    pixels: new Uint8Array(raster.width * raster.height * 3),
    width: raster.width,
    height: raster.height,
    layout,
  });
}
