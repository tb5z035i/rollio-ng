import sharp from "sharp";
import { nowMs } from "../debug-metrics.js";
import { nearestAnsi256 } from "../color-palette.js";
import {
  measureOutputBytes,
  type AsciiRenderInput,
  type AsciiRenderLayout,
  type AsciiRenderResult,
  type AsciiRendererBackend,
  type AsciiRasterDimensions,
} from "./types.js";

const RESET = "\x1b[0m";
const FG_SGR = Array.from({ length: 256 }, (_, idx) => `\x1b[38;5;${idx}m`);
const CELL_WIDTH = 8;
const CELL_HEIGHT = 12;
const CIRCLE_RADIUS = 0.24;
const GLOBAL_CONTRAST_EXPONENT = 1.55;
const DIRECTIONAL_CONTRAST_EXPONENT = 1.45;
const LOOKUP_RANGE = 10;
const ASCII_GLYPHS = Array.from({ length: 95 }, (_, idx) =>
  String.fromCharCode(32 + idx),
);

interface CircleSpec {
  cx: number;
  cy: number;
  radius: number;
}

interface SamplePoint {
  dx: number;
  dy: number;
}

interface GlyphEntry {
  char: string;
  vector: number[];
}

interface GlyphDatabase {
  glyphs: GlyphEntry[];
  cache: Map<number, number>;
}

const INTERNAL_CIRCLES: CircleSpec[] = [
  { cx: 0.24, cy: 0.18, radius: CIRCLE_RADIUS },
  { cx: 0.76, cy: 0.18, radius: CIRCLE_RADIUS },
  { cx: 0.18, cy: 0.5, radius: CIRCLE_RADIUS },
  { cx: 0.82, cy: 0.5, radius: CIRCLE_RADIUS },
  { cx: 0.24, cy: 0.82, radius: CIRCLE_RADIUS },
  { cx: 0.76, cy: 0.82, radius: CIRCLE_RADIUS },
];

const EXTERNAL_CIRCLES: CircleSpec[] = [
  { cx: 0.2, cy: -0.12, radius: CIRCLE_RADIUS },
  { cx: 0.8, cy: -0.12, radius: CIRCLE_RADIUS },
  { cx: -0.12, cy: 0.2, radius: CIRCLE_RADIUS },
  { cx: 1.12, cy: 0.2, radius: CIRCLE_RADIUS },
  { cx: -0.12, cy: 0.5, radius: CIRCLE_RADIUS },
  { cx: 1.12, cy: 0.5, radius: CIRCLE_RADIUS },
  { cx: -0.12, cy: 0.8, radius: CIRCLE_RADIUS },
  { cx: 1.12, cy: 0.8, radius: CIRCLE_RADIUS },
  { cx: 0.2, cy: 1.12, radius: CIRCLE_RADIUS },
  { cx: 0.8, cy: 1.12, radius: CIRCLE_RADIUS },
];

const AFFECTING_EXTERNAL_INDICES = [
  [0, 1, 2, 4],
  [0, 1, 3, 5],
  [2, 4, 6],
  [3, 5, 7],
  [4, 6, 8, 9],
  [5, 7, 8, 9],
];

const INTERNAL_MASKS = INTERNAL_CIRCLES.map((circle) =>
  buildMask(circle, CELL_WIDTH, CELL_HEIGHT),
);
const EXTERNAL_MASKS = EXTERNAL_CIRCLES.map((circle) =>
  buildMask(circle, CELL_WIDTH, CELL_HEIGHT),
);

let glyphDatabasePromise: Promise<GlyphDatabase> | null = null;

export class TypeScriptHarriRenderer implements AsciiRendererBackend {
  readonly id = "ts-harri";
  readonly label = "TypeScript Harri";
  readonly kind = "typescript" as const;
  readonly algorithm = "shape-lookup";

  describeRaster(layout: AsciiRenderLayout): AsciiRasterDimensions {
    return {
      width: layout.columns * CELL_WIDTH,
      height: layout.rows * CELL_HEIGHT,
    };
  }

  layoutForRaster(raster: AsciiRasterDimensions): AsciiRenderLayout {
    return {
      columns: Math.max(1, Math.ceil(raster.width / CELL_WIDTH)),
      rows: Math.max(1, Math.ceil(raster.height / CELL_HEIGHT)),
    };
  }

  async prepare(): Promise<void> {
    await getGlyphDatabase();
  }

  async render(input: AsciiRenderInput): Promise<AsciiRenderResult> {
    const expected = this.describeRaster(input.layout);
    if (input.width !== expected.width || input.height !== expected.height) {
      throw new Error(
        `${this.id} expected raster ${expected.width}x${expected.height}, received ` +
          `${input.width}x${input.height}`,
      );
    }

    const glyphDb = await getGlyphDatabase();
    const totalStartMs = nowMs();

    const sampleStartMs = nowMs();
    const luminancePlane = buildLuminancePlane(input.pixels, input.width, input.height);
    const sampledCells = sampleCells(luminancePlane, input.width, input.layout);
    const sampleMs = nowMs() - sampleStartMs;

    const lookupStartMs = nowMs();
    let cacheHits = 0;
    let cacheMisses = 0;
    const renderedCells = sampledCells.map((cell) => {
      const contrasted = applyGlobalContrast(
        applyDirectionalContrast(
          cell.internalVector,
          cell.externalVector,
          DIRECTIONAL_CONTRAST_EXPONENT,
        ),
        GLOBAL_CONTRAST_EXPONENT,
      );
      const cacheKey = quantizeVector(contrasted);
      let glyphIndex = glyphDb.cache.get(cacheKey);
      if (glyphIndex === undefined) {
        cacheMisses += 1;
        glyphIndex = findBestGlyph(contrasted, glyphDb.glyphs);
        glyphDb.cache.set(cacheKey, glyphIndex);
      } else {
        cacheHits += 1;
      }

      return {
        glyph: glyphDb.glyphs[glyphIndex].char,
        luminance: cell.averageLuminance,
      };
    });
    const lookupMs = nowMs() - lookupStartMs;

    const assembleStartMs = nowMs();
    let sgrChangeCount = 0;
    const lines = new Array<string>(input.layout.rows);
    for (let row = 0; row < input.layout.rows; row++) {
      const parts: string[] = [];
      let previousFgAnsi = -1;
      for (let column = 0; column < input.layout.columns; column++) {
        const cell = renderedCells[row * input.layout.columns + column];
        const luminanceByte = Math.max(
          0,
          Math.min(255, Math.round(cell.luminance * 255)),
        );
        const fgAnsi = nearestAnsi256(
          luminanceByte,
          luminanceByte,
          luminanceByte,
        );
        if (fgAnsi !== previousFgAnsi) {
          parts.push(FG_SGR[fgAnsi]);
          previousFgAnsi = fgAnsi;
          sgrChangeCount += 1;
        }
        parts.push(cell.glyph);
      }
      parts.push(RESET);
      lines[row] = parts.join("");
    }
    const assembleMs = nowMs() - assembleStartMs;
    const totalMs = nowMs() - totalStartMs;

    return {
      backendId: this.id,
      lines,
      stats: {
        rasterWidth: input.width,
        rasterHeight: input.height,
        outputColumns: input.layout.columns,
        outputRows: input.layout.rows,
        outputBytes: measureOutputBytes(lines),
        cellCount: input.layout.columns * input.layout.rows,
        sampleCount:
          input.layout.columns *
          input.layout.rows *
          (INTERNAL_MASKS.length + EXTERNAL_MASKS.length),
        lookupCount: input.layout.columns * input.layout.rows,
        sgrChangeCount,
        cacheHits,
        cacheMisses,
        timings: {
          totalMs,
          sampleMs,
          lookupMs,
          assembleMs,
        },
      },
    };
  }
}

async function getGlyphDatabase(): Promise<GlyphDatabase> {
  if (!glyphDatabasePromise) {
    glyphDatabasePromise = buildGlyphDatabase();
  }
  return await glyphDatabasePromise;
}

async function buildGlyphDatabase(): Promise<GlyphDatabase> {
  const rawGlyphs: GlyphEntry[] = [];
  const maxComponents = new Array<number>(INTERNAL_MASKS.length).fill(0);

  for (const char of ASCII_GLYPHS) {
    const glyphMask = await rasterizeGlyph(char);
    const vector = INTERNAL_MASKS.map((mask) => sampleScalarMask(glyphMask, mask));
    vector.forEach((value, index) => {
      maxComponents[index] = Math.max(maxComponents[index], value);
    });
    rawGlyphs.push({ char, vector });
  }

  return {
    glyphs: rawGlyphs.map((glyph) => ({
      char: glyph.char,
      vector: glyph.vector.map((value, index) =>
        maxComponents[index] > 0 ? value / maxComponents[index] : 0,
      ),
    })),
    cache: new Map(),
  };
}

async function rasterizeGlyph(char: string): Promise<Float32Array> {
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="${CELL_WIDTH}" height="${CELL_HEIGHT}" viewBox="0 0 ${CELL_WIDTH} ${CELL_HEIGHT}">
<rect width="100%" height="100%" fill="black"/>
<text x="50%" y="56%" text-anchor="middle" dominant-baseline="middle" fill="white" font-family="monospace" font-size="${CELL_HEIGHT * 0.9}">${escapeXml(char)}</text>
</svg>`;
  const { data, info } = await sharp(Buffer.from(svg))
    .greyscale()
    .raw()
    .toBuffer({ resolveWithObject: true });

  const mask = new Float32Array(info.width * info.height);
  for (let idx = 0; idx < mask.length; idx++) {
    mask[idx] = data[idx] / 255;
  }
  return mask;
}

function buildMask(
  circle: CircleSpec,
  cellWidth: number,
  cellHeight: number,
): SamplePoint[] {
  const centerX = circle.cx * cellWidth;
  const centerY = circle.cy * cellHeight;
  const radius = circle.radius * cellWidth;
  const radiusSquared = radius * radius;
  const points: SamplePoint[] = [];

  const minX = Math.floor(centerX - radius);
  const maxX = Math.ceil(centerX + radius);
  const minY = Math.floor(centerY - radius);
  const maxY = Math.ceil(centerY + radius);

  for (let y = minY; y <= maxY; y++) {
    for (let x = minX; x <= maxX; x++) {
      const dx = x + 0.5 - centerX;
      const dy = y + 0.5 - centerY;
      if (dx * dx + dy * dy <= radiusSquared) {
        points.push({ dx: x, dy: y });
      }
    }
  }

  return points;
}

function buildLuminancePlane(
  pixels: Buffer | Uint8Array,
  width: number,
  height: number,
): Float32Array {
  const plane = new Float32Array(width * height);
  for (let idx = 0; idx < plane.length; idx++) {
    const pixelOffset = idx * 3;
    const r = pixels[pixelOffset];
    const g = pixels[pixelOffset + 1];
    const b = pixels[pixelOffset + 2];
    plane[idx] = (0.2126 * r + 0.7152 * g + 0.0722 * b) / 255;
  }
  return plane;
}

function sampleCells(
  plane: Float32Array,
  width: number,
  layout: AsciiRenderLayout,
): Array<{
  internalVector: number[];
  externalVector: number[];
  averageLuminance: number;
}> {
  const cells: Array<{
    internalVector: number[];
    externalVector: number[];
    averageLuminance: number;
  }> = [];

  for (let row = 0; row < layout.rows; row++) {
    const originY = row * CELL_HEIGHT;
    for (let column = 0; column < layout.columns; column++) {
      const originX = column * CELL_WIDTH;
      const internalVector = INTERNAL_MASKS.map((mask) =>
        samplePlaneMask(plane, width, originX, originY, mask),
      );
      const externalVector = EXTERNAL_MASKS.map((mask) =>
        samplePlaneMask(plane, width, originX, originY, mask),
      );
      const averageLuminance =
        internalVector.reduce((sum, value) => sum + value, 0) / internalVector.length;
      cells.push({
        internalVector,
        externalVector,
        averageLuminance,
      });
    }
  }

  return cells;
}

function samplePlaneMask(
  plane: Float32Array,
  width: number,
  originX: number,
  originY: number,
  mask: SamplePoint[],
): number {
  let sum = 0;
  let count = 0;
  const height = Math.floor(plane.length / width);

  for (const point of mask) {
    const x = originX + point.dx;
    const y = originY + point.dy;
    if (x < 0 || y < 0 || x >= width || y >= height) {
      continue;
    }
    sum += plane[y * width + x];
    count += 1;
  }

  return count > 0 ? sum / count : 0;
}

function sampleScalarMask(values: Float32Array, mask: SamplePoint[]): number {
  let sum = 0;
  let count = 0;
  for (const point of mask) {
    if (
      point.dx < 0 ||
      point.dy < 0 ||
      point.dx >= CELL_WIDTH ||
      point.dy >= CELL_HEIGHT
    ) {
      continue;
    }
    sum += values[point.dy * CELL_WIDTH + point.dx];
    count += 1;
  }
  return count > 0 ? sum / count : 0;
}

function applyDirectionalContrast(
  internalVector: number[],
  externalVector: number[],
  exponent: number,
): number[] {
  return internalVector.map((value, index) => {
    let maxValue = value;
    for (const externalIndex of AFFECTING_EXTERNAL_INDICES[index]) {
      maxValue = Math.max(maxValue, externalVector[externalIndex] ?? 0);
    }
    if (maxValue <= 0) {
      return 0;
    }
    return Math.pow(value / maxValue, exponent) * maxValue;
  });
}

function applyGlobalContrast(vector: number[], exponent: number): number[] {
  const maxValue = Math.max(...vector, 0);
  if (maxValue <= 0) {
    return vector.map(() => 0);
  }

  return vector.map((value) => Math.pow(value / maxValue, exponent) * maxValue);
}

function quantizeVector(vector: number[]): number {
  let key = 0;
  for (const value of vector) {
    const quantized = Math.min(
      LOOKUP_RANGE - 1,
      Math.max(0, Math.floor(value * LOOKUP_RANGE)),
    );
    key = key * LOOKUP_RANGE + quantized;
  }
  return key;
}

function findBestGlyph(vector: number[], glyphs: GlyphEntry[]): number {
  let bestIndex = 0;
  let bestDistance = Number.POSITIVE_INFINITY;

  for (let index = 0; index < glyphs.length; index++) {
    const glyph = glyphs[index];
    let distance = 0;
    for (let component = 0; component < vector.length; component++) {
      const delta = vector[component] - glyph.vector[component];
      distance += delta * delta;
    }
    if (distance < bestDistance) {
      bestDistance = distance;
      bestIndex = index;
    }
  }

  return bestIndex;
}

function escapeXml(text: string): string {
  return text
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&apos;");
}
