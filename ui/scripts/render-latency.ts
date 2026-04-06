import { Console } from "node:console";
import { access, readFile } from "node:fs/promises";
import path from "node:path";
import process from "node:process";
import sharp from "sharp";
import {
  ASCII_RENDERER_IDS,
  isAsciiRendererId,
  type AsciiRasterDimensions,
  type AsciiRendererBackend,
  type AsciiRendererId,
} from "../src/lib/renderers/index.js";
import {
  CLI_RASTER_CASES,
  createBackends,
  disposeBackends,
  type RasterBenchmarkCase,
  type RenderMeasurement,
} from "../test/render-harness.js";

const PROGRESS_REPEAT_THRESHOLD = 5;
const PROGRESS_TOTAL_THRESHOLD = 12;
const PROGRESS_BAR_WIDTH = 24;
const DEFAULT_PREVIEW_COLUMNS = 120;
const DEFAULT_PREVIEW_ROWS = 40;

type ViewMode = "fit" | "full";

interface PreparedSourceImage {
  pixels: Buffer | Uint8Array;
  width: number;
  height: number;
  decodeMs: number;
}

interface CliOptions {
  imagePath: string;
  backendIds: AsciiRendererId[];
  cases: RasterBenchmarkCase[];
  repeat: number;
  viewBackendIds: AsciiRendererId[];
  viewMode: ViewMode;
}

interface SummaryRow {
  backend: string;
  targetRaster: string;
  actualRaster: string;
  outputGrid: string;
  runs: number;
  totalAvgMs: string;
  totalP95Ms: string;
  resizeAvgMs: string;
  renderAvgMs: string;
  renderP95Ms: string;
  backendAvgMs: string;
  sampleAvgMs: string;
  lookupAvgMs: string;
  assembleAvgMs: string;
  ansiAvgMs: string;
  adapterAvgMs: string;
  outputBytes: number;
}

interface ProgressTracker {
  advance(detail: string): void;
  finish(): void;
}

async function main(): Promise<void> {
  const options = await parseArgs(process.argv.slice(2));
  const allBackendIds = uniqueRendererIds([
    ...options.backendIds,
    ...options.viewBackendIds,
  ]);
  const backends = await createBackends(allBackendIds);
  const benchmarkBackends = options.backendIds
    .map((backendId) => backends.find((backend) => backend.id === backendId))
    .filter((backend): backend is AsciiRendererBackend => backend !== undefined);
  const imageBytes = await readFile(options.imagePath);
  const sourceImage = await prepareSourceImage(imageBytes);
  const originalRaster = {
    width: sourceImage.width,
    height: sourceImage.height,
  };

  try {
    console.error(`Image: ${options.imagePath}`);
    console.error(`Original raster: ${originalRaster.width}x${originalRaster.height}`);
    console.error(
      `Decode once: ${formatMs(sourceImage.decodeMs)}ms (excluded from per-run totals)`,
    );
    console.error(`Backends: ${options.backendIds.join(", ")}`);
    console.error(
      `Target rasters: ${options.cases
        .map((rasterCase) => `${rasterCase.raster.width}x${rasterCase.raster.height}`)
        .join(", ")}`,
    );
    console.error(`Repeats: ${options.repeat}`);
    if (options.viewBackendIds.length > 0 && process.stdout.isTTY) {
      console.error(
        "Preview output is written to stdout; pipe to `less -R -S` if you want scrolling without wrapping.",
      );
    }

    const totalBenchmarkSteps =
      benchmarkBackends.length * options.cases.length * options.repeat;
    const progress = createProgressTracker(
      totalBenchmarkSteps,
      options.repeat >= PROGRESS_REPEAT_THRESHOLD ||
        totalBenchmarkSteps >= PROGRESS_TOTAL_THRESHOLD,
    );

    const summaryRows: SummaryRow[] = [];
    for (const backend of benchmarkBackends) {
      for (const rasterCase of options.cases) {
        const samples: RenderMeasurement[] = [];
        for (let run = 0; run < options.repeat; run++) {
          samples.push(
            await benchmarkBackendForPreparedRasterCase(
              backend,
              path.basename(options.imagePath),
              sourceImage,
              rasterCase,
            ),
          );
          progress.advance(
            `${backend.id} ${rasterCase.raster.width}x${rasterCase.raster.height} (${run + 1}/${options.repeat})`,
          );
        }
        summaryRows.push(summarize(backend.id, rasterCase, samples));
      }
    }
    progress.finish();

    const tableConsole =
      options.viewBackendIds.length > 0
        ? new Console({ stdout: process.stderr, stderr: process.stderr })
        : console;
    tableConsole.table(summaryRows);

    if (options.viewBackendIds.length > 0) {
      await printOriginalResolutionPreviews(
        backends,
        options.viewBackendIds,
        sourceImage,
        options.viewMode,
      );
    }
  } finally {
    await disposeBackends(backends);
  }
}

function summarize(
  backendId: string,
  rasterCase: RasterBenchmarkCase,
  samples: readonly RenderMeasurement[],
): SummaryRow {
  return {
    backend: backendId,
    targetRaster: `${rasterCase.raster.width}x${rasterCase.raster.height}`,
    actualRaster: `${samples[0]?.rasterWidth ?? 0}x${samples[0]?.rasterHeight ?? 0}`,
    outputGrid: `${samples[0]?.outputColumns ?? 0}x${samples[0]?.outputRows ?? 0}`,
    runs: samples.length,
    totalAvgMs: formatMs(
      average(samples.map((sample) => sample.resizeMs + sample.renderCallMs)),
    ),
    totalP95Ms: formatMs(
      percentile(
        samples.map((sample) => sample.resizeMs + sample.renderCallMs),
        0.95,
      ),
    ),
    resizeAvgMs: formatMs(average(samples.map((sample) => sample.resizeMs))),
    renderAvgMs: formatMs(average(samples.map((sample) => sample.renderCallMs))),
    renderP95Ms: formatMs(percentile(samples.map((sample) => sample.renderCallMs), 0.95)),
    backendAvgMs: formatMs(average(samples.map((sample) => sample.backendMs))),
    sampleAvgMs: formatOptionalMs(samples.map((sample) => sample.sampleMs)),
    lookupAvgMs: formatOptionalMs(samples.map((sample) => sample.lookupMs)),
    assembleAvgMs: formatOptionalMs(samples.map((sample) => sample.assembleMs)),
    ansiAvgMs: formatOptionalMs(samples.map((sample) => sample.ansiMs)),
    adapterAvgMs: formatOptionalMs(samples.map((sample) => sample.adapterMs)),
    outputBytes: samples[0]?.outputBytes ?? 0,
  };
}

async function parseArgs(argv: string[]): Promise<CliOptions> {
  if (argv.includes("--help") || argv.includes("-h")) {
    printUsage();
    process.exit(0);
  }

  let imagePath = "";
  let backendIds: AsciiRendererId[] = [...ASCII_RENDERER_IDS];
  let cases = CLI_RASTER_CASES;
  let repeat = 5;
  let viewBackendIds: AsciiRendererId[] = [];
  let viewMode: ViewMode = "fit";

  for (let index = 0; index < argv.length; index++) {
    const arg = argv[index];
    switch (arg) {
      case "--image":
      case "-i":
        imagePath = requireValue(argv, ++index, arg);
        break;
      case "--backend":
      case "-b":
        backendIds = parseBackendIds(requireValue(argv, ++index, arg));
        break;
      case "--resolutions":
      case "-r":
        cases = parseRasterCases(requireValue(argv, ++index, arg));
        break;
      case "--repeat":
        repeat = parseRepeat(requireValue(argv, ++index, arg));
        break;
      case "--view":
      case "--view-backend":
        viewBackendIds = parseBackendIds(requireValue(argv, ++index, arg));
        break;
      case "--view-mode":
        viewMode = parseViewMode(requireValue(argv, ++index, arg));
        break;
      case "--view-full":
        viewMode = "full";
        break;
      default:
        throw new Error(`Unknown argument: ${arg}\n\n${usageText()}`);
    }
  }

  if (!imagePath) {
    throw new Error(`Missing required --image option.\n\n${usageText()}`);
  }

  const resolvedImagePath = path.resolve(process.cwd(), imagePath);
  await access(resolvedImagePath);

  return {
    imagePath: resolvedImagePath,
    backendIds,
    cases,
    repeat,
    viewBackendIds,
    viewMode,
  };
}

function parseBackendIds(value: string): AsciiRendererId[] {
  if (value === "all") {
    return [...ASCII_RENDERER_IDS];
  }

  const ids = value
    .split(",")
    .map((item) => item.trim())
    .filter(Boolean);
  if (ids.length === 0) {
    throw new Error("Expected at least one backend id.");
  }

  for (const id of ids) {
    if (!isAsciiRendererId(id)) {
      throw new Error(
        `Unknown backend "${id}". Valid backends: ${ASCII_RENDERER_IDS.join(", ")}`,
      );
    }
  }

  return ids;
}

function uniqueRendererIds(ids: readonly AsciiRendererId[]): AsciiRendererId[] {
  return [...new Set(ids)];
}

function parseRasterCases(value: string): RasterBenchmarkCase[] {
  const resolutions = value
    .split(",")
    .map((item) => item.trim())
    .filter(Boolean);
  if (resolutions.length === 0) {
    throw new Error("Expected at least one raster size like 640x480.");
  }

  return resolutions.map((resolution) => {
    const match = /^(\d+)x(\d+)$/i.exec(resolution);
    if (!match) {
      throw new Error(
        `Invalid resolution "${resolution}". Use pixel raster sizes like 640x480.`,
      );
    }

    return {
      name: resolution,
      raster: {
        width: Number.parseInt(match[1], 10),
        height: Number.parseInt(match[2], 10),
      },
    };
  });
}

function parseRepeat(value: string): number {
  const repeat = Number.parseInt(value, 10);
  if (!Number.isInteger(repeat) || repeat < 1) {
    throw new Error(`Invalid repeat count "${value}". Expected integer >= 1.`);
  }
  return repeat;
}

function parseViewMode(value: string): ViewMode {
  if (value === "fit" || value === "full") {
    return value;
  }
  throw new Error(`Invalid view mode "${value}". Expected "fit" or "full".`);
}

function requireValue(argv: string[], index: number, flag: string): string {
  const value = argv[index];
  if (!value) {
    throw new Error(`Missing value for ${flag}.`);
  }
  return value;
}

async function prepareSourceImage(imageBytes: Buffer): Promise<PreparedSourceImage> {
  const decodeStartMs = performance.now();
  const { data, info } = await sharp(imageBytes)
    .removeAlpha()
    .raw()
    .toBuffer({ resolveWithObject: true });
  const decodeMs = performance.now() - decodeStartMs;

  return {
    pixels: data,
    width: info.width,
    height: info.height,
    decodeMs,
  };
}

async function benchmarkBackendForPreparedRasterCase(
  backend: AsciiRendererBackend,
  fixtureName: string,
  sourceImage: PreparedSourceImage,
  rasterCase: RasterBenchmarkCase,
): Promise<RenderMeasurement> {
  const layout = backend.layoutForRaster(rasterCase.raster);
  const raster = backend.describeRaster(layout);

  const resizeStartMs = performance.now();
  const pixels = resizeRgbNearest(
    sourceImage.pixels,
    sourceImage.width,
    sourceImage.height,
    raster.width,
    raster.height,
  );
  const resizeMs = performance.now() - resizeStartMs;

  const renderStartMs = performance.now();
  const result = await backend.render({
    pixels,
    width: raster.width,
    height: raster.height,
    layout,
  });
  const renderCallMs = performance.now() - renderStartMs;

  return {
    fixture: fixtureName,
    caseName: rasterCase.name,
    backendId: backend.id,
    backendLabel: backend.label,
    rasterWidth: raster.width,
    rasterHeight: raster.height,
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

async function printOriginalResolutionPreviews(
  backends: readonly AsciiRendererBackend[],
  viewBackendIds: readonly AsciiRendererId[],
  sourceImage: PreparedSourceImage,
  viewMode: ViewMode,
): Promise<void> {
  const selectedBackends = viewBackendIds
    .map((backendId) => backends.find((backend) => backend.id === backendId))
    .filter((backend): backend is AsciiRendererBackend => backend !== undefined);

  for (const backend of selectedBackends) {
    const originalRaster = {
      width: sourceImage.width,
      height: sourceImage.height,
    };
    const fullLayout = backend.layoutForRaster(originalRaster);
    const previewLayout =
      viewMode === "full"
        ? fullLayout
        : scaleLayoutToFitTerminal(fullLayout, currentTerminalPreviewSize());
    const previewRaster = backend.describeRaster(previewLayout);
    console.error(
      `Rendering preview for ${backend.id} from original raster ${originalRaster.width}x${originalRaster.height} (${viewMode} mode)...`,
    );
    const pixels = resizeRgbNearest(
      sourceImage.pixels,
      sourceImage.width,
      sourceImage.height,
      previewRaster.width,
      previewRaster.height,
    );
    const result = await backend.render({
      pixels,
      width: previewRaster.width,
      height: previewRaster.height,
      layout: previewLayout,
    });

    process.stdout.write(
      [
        "",
        `=== ${backend.id} preview | source=${originalRaster.width}x${originalRaster.height} | mode=${viewMode} | raster=${previewRaster.width}x${previewRaster.height} | output=${result.stats.outputColumns}x${result.stats.outputRows} ===`,
        ...result.lines,
        "",
      ].join("\n"),
    );
  }
}

function createProgressTracker(totalSteps: number, enabled: boolean): ProgressTracker {
  if (!enabled || totalSteps <= 0) {
    return {
      advance() {},
      finish() {},
    };
  }

  if (process.stderr.isTTY) {
    let completed = 0;
    let lastLineLength = 0;
    return {
      advance(detail: string) {
        completed += 1;
        const ratio = completed / totalSteps;
        const filled = Math.round(ratio * PROGRESS_BAR_WIDTH);
        const bar =
          "=".repeat(filled) + ".".repeat(Math.max(0, PROGRESS_BAR_WIDTH - filled));
        const line = `Benchmark progress [${bar}] ${completed}/${totalSteps} ${detail}`;
        lastLineLength = Math.max(lastLineLength, line.length);
        process.stderr.write(`\r${line.padEnd(lastLineLength)}`);
      },
      finish() {
        process.stderr.write("\n");
      },
    };
  }

  let completed = 0;
  const reportEvery = Math.max(1, Math.floor(totalSteps / 10));
  return {
    advance(detail: string) {
      completed += 1;
      if (completed % reportEvery === 0 || completed === totalSteps) {
        console.error(`Benchmark progress ${completed}/${totalSteps}: ${detail}`);
      }
    },
    finish() {},
  };
}

function scaleLayoutToFitTerminal(
  layout: { columns: number; rows: number },
  terminal: { columns: number; rows: number },
): { columns: number; rows: number } {
  const scale = Math.min(
    1,
    terminal.columns / Math.max(1, layout.columns),
    terminal.rows / Math.max(1, layout.rows),
  );
  return {
    columns: Math.max(1, Math.floor(layout.columns * scale)),
    rows: Math.max(1, Math.floor(layout.rows * scale)),
  };
}

function currentTerminalPreviewSize(): { columns: number; rows: number } {
  const envColumns = Number.parseInt(process.env.COLUMNS ?? "", 10);
  const envRows = Number.parseInt(process.env.LINES ?? "", 10);
  const columns =
    process.stdout.columns ??
    (Number.isFinite(envColumns) ? envColumns : DEFAULT_PREVIEW_COLUMNS);
  const rows =
    process.stdout.rows ??
    (Number.isFinite(envRows) ? envRows : DEFAULT_PREVIEW_ROWS);

  return {
    columns: Math.max(1, columns),
    rows: Math.max(4, rows - 6),
  };
}

function resizeRgbNearest(
  sourcePixels: Buffer | Uint8Array,
  sourceWidth: number,
  sourceHeight: number,
  targetWidth: number,
  targetHeight: number,
): Uint8Array {
  if (sourceWidth === targetWidth && sourceHeight === targetHeight) {
    return Uint8Array.from(sourcePixels);
  }

  const targetPixels = new Uint8Array(targetWidth * targetHeight * 3);
  for (let targetY = 0; targetY < targetHeight; targetY++) {
    const sourceY = Math.min(
      sourceHeight - 1,
      Math.floor((targetY * sourceHeight) / targetHeight),
    );
    for (let targetX = 0; targetX < targetWidth; targetX++) {
      const sourceX = Math.min(
        sourceWidth - 1,
        Math.floor((targetX * sourceWidth) / targetWidth),
      );
      const sourceOffset = (sourceY * sourceWidth + sourceX) * 3;
      const targetOffset = (targetY * targetWidth + targetX) * 3;
      targetPixels[targetOffset] = sourcePixels[sourceOffset];
      targetPixels[targetOffset + 1] = sourcePixels[sourceOffset + 1];
      targetPixels[targetOffset + 2] = sourcePixels[sourceOffset + 2];
    }
  }
  return targetPixels;
}

function average(values: readonly number[]): number {
  if (values.length === 0) {
    return Number.NaN;
  }
  return values.reduce((sum, value) => sum + value, 0) / values.length;
}

function percentile(values: readonly number[], percentileRank: number): number {
  if (values.length === 0) {
    return Number.NaN;
  }
  const sorted = [...values].sort((left, right) => left - right);
  const index = Math.min(
    sorted.length - 1,
    Math.max(0, Math.ceil(sorted.length * percentileRank) - 1),
  );
  return sorted[index];
}

function formatMs(value: number): string {
  return Number.isFinite(value) ? value.toFixed(2) : "n/a";
}

function formatOptionalMs(values: ReadonlyArray<number | undefined>): string {
  const finiteValues = values.filter(
    (value): value is number => value !== undefined && Number.isFinite(value),
  );
  if (finiteValues.length === 0) {
    return "n/a";
  }
  return formatMs(average(finiteValues));
}

function printUsage(): void {
  console.log(usageText());
}

function usageText(): string {
  return [
    "Usage: npm run render:latency -- --image <path> [options]",
    "",
    "Options:",
    "  --image, -i         Local image path to benchmark (required)",
    `  --backend, -b       Backend ids or "all" (${ASCII_RENDERER_IDS.join(", ")})`,
    "  --resolutions, -r   Comma-separated target raster sizes, e.g. 640x480,1280x720",
    "  --repeat            Number of repeated runs per backend/raster (default: 5)",
    '  --view              Backend ids or "all" to print original-resolution previews',
    '  --view-mode         "fit" (default) or "full" preview output size',
    "  --view-full         Shortcut for --view-mode full",
    "  --help, -h          Show this help",
    "",
    "Notes:",
    "  - Resolution values are target pixel rasters, not output character-grid sizes.",
    "  - Default benchmark rasters are 320x240, 640x480, and 1280x720.",
    "  - totalAvgMs is the main overall local-rendering number to look at here:",
    "    resize + backend render call.",
    "  - renderAvgMs is backend-call time only, excluding the resize step.",
    "  - Preview output is fit to the terminal by default; use --view-full for the",
    "    uncapped render, ideally with: | less -R -S",
  ].join("\n");
}

void main().catch((error: unknown) => {
  const message = error instanceof Error ? error.message : String(error);
  console.error(message);
  process.exit(1);
});
