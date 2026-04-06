import { nowMs } from "../debug-metrics.js";
import { renderToAnsiLines } from "../ansi-renderer.js";
import {
  assertExpectedRaster,
  measureOutputBytes,
  type AsciiRenderInput,
  type AsciiRenderLayout,
  type AsciiRenderResult,
  type AsciiRendererBackend,
  type AsciiRasterDimensions,
} from "./types.js";

export class TypeScriptHalfBlockRenderer implements AsciiRendererBackend {
  readonly id = "ts-half-block";
  readonly label = "TypeScript Half Block";
  readonly kind = "typescript" as const;
  readonly algorithm = "half-block-truecolor";

  describeRaster(layout: AsciiRenderLayout): AsciiRasterDimensions {
    return {
      width: layout.columns,
      height: layout.rows * 2,
    };
  }

  layoutForRaster(raster: AsciiRasterDimensions): AsciiRenderLayout {
    return {
      columns: Math.max(1, raster.width),
      rows: Math.max(1, Math.ceil(raster.height / 2)),
    };
  }

  async render(input: AsciiRenderInput): Promise<AsciiRenderResult> {
    assertExpectedRaster(this, input.width, input.height, input.layout);

    const ansiStartMs = nowMs();
    const ansiResult = renderToAnsiLines(input.pixels, input.width, input.height, {
      colorMode: "truecolor",
    });
    const ansiMs = nowMs() - ansiStartMs;

    return {
      backendId: this.id,
      lines: ansiResult.lines,
      stats: {
        rasterWidth: input.width,
        rasterHeight: input.height,
        outputColumns: input.layout.columns,
        outputRows: input.layout.rows,
        outputBytes: measureOutputBytes(ansiResult.lines),
        cellCount: ansiResult.cellCount,
        sgrChangeCount: ansiResult.sgrChangeCount,
        timings: {
          totalMs: ansiMs,
          ansiMs,
        },
      },
    };
  }
}
