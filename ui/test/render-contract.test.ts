import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import {
  ASCII_RENDERER_IDS,
  createAsciiRendererBackend,
  type AsciiRenderLayout,
} from "../src/lib/renderers/index.js";
import {
  BENCHMARK_CASES,
  FIXTURES,
  benchmarkBackendForFixture,
  disposeBackends,
  stripAnsi,
  visibleWidth,
} from "./render-harness.js";

test("all configured backends render fixture cases with expected dimensions", async () => {
  const backendIds = ASCII_RENDERER_IDS;
  const backends = backendIds.map((id) => createAsciiRendererBackend(id));
  try {
    for (const backend of backends) {
      await backend.prepare?.();
    }

    const fixtures = FIXTURES.slice(0, 2);
    const cases = BENCHMARK_CASES.slice(0, 2);

    for (const backend of backends) {
      for (const fixture of fixtures) {
        const fixtureBytes = await readFile(fixture.path);
        for (const benchmarkCase of cases) {
          const measurement = await benchmarkBackendForFixture(
            backend,
            fixture.name,
            fixtureBytes,
            benchmarkCase,
          );
          assert.equal(measurement.outputColumns, benchmarkCase.layout.columns);
          assert.equal(measurement.outputRows, benchmarkCase.layout.rows);
          assert.ok(measurement.outputBytes > 0);

          const raster = backend.describeRaster(benchmarkCase.layout);
          const result = await backend.render({
            pixels: new Uint8Array(raster.width * raster.height * 3),
            width: raster.width,
            height: raster.height,
            layout: benchmarkCase.layout,
          });
          assert.equal(result.lines.length, benchmarkCase.layout.rows);
          for (const line of result.lines) {
            assert.equal(visibleWidth(line), benchmarkCase.layout.columns);
            assert.ok(stripAnsi(line).length >= benchmarkCase.layout.columns);
          }
        }
      }
    }
  } finally {
    await disposeBackends(backends);
  }
});

test("ts-harri produces visible ASCII glyphs for shaped input", async () => {
  const backend = createAsciiRendererBackend("ts-harri");
  await backend.prepare?.();

  try {
    const layout: AsciiRenderLayout = { columns: 12, rows: 6 };
    const raster = backend.describeRaster(layout);
    const pixels = new Uint8Array(raster.width * raster.height * 3);

    for (let y = 0; y < raster.height; y++) {
      for (let x = 0; x < raster.width; x++) {
        const dx = x - raster.width / 2;
        const dy = y - raster.height / 2;
        const onCircle =
          dx * dx + dy * dy <= Math.pow(Math.min(raster.width, raster.height) * 0.28, 2);
        if (!onCircle) {
          continue;
        }
        const offset = (y * raster.width + x) * 3;
        pixels[offset] = 255;
        pixels[offset + 1] = 255;
        pixels[offset + 2] = 255;
      }
    }

    const result = await backend.render({
      pixels,
      width: raster.width,
      height: raster.height,
      layout,
    });
    const text = stripAnsi(result.lines.join("\n"));
    assert.match(text, /[^\s]/);
  } finally {
    await disposeBackends([backend]);
  }
});
