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
  channelsForBackend,
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
            pixels: new Uint8Array(
              raster.width * raster.height * channelsForBackend(backend),
            ),
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

test("native-rust uses the worker-backed native path", async () => {
  const backend = createAsciiRendererBackend("native-rust");
  await backend.prepare?.();

  try {
    assert.equal(backend.kind, "worker");
    assert.equal(backend.algorithm, "shape-lookup-rust-native-harri");
    assert.equal(backend.pixelFormat, "luma8");
    const layout: AsciiRenderLayout = { columns: 8, rows: 4 };
    const raster = backend.describeRaster(layout);
    const result = await backend.render({
      pixels: new Uint8Array(raster.width * raster.height),
      width: raster.width,
      height: raster.height,
      layout,
    });
    assert.equal(result.backendId, "native-rust");
    assert.equal(result.lines.length, layout.rows);
    assert.match(result.lines[0] ?? "", /\x1b\[38;5;\d+m/);
    assert.notEqual(result.stats.timings.adapterMs, undefined);
  } finally {
    await disposeBackends([backend]);
  }
});

test("native-rust matches ts-harri on deterministic grayscale input", async () => {
  const nativeBackend = createAsciiRendererBackend("native-rust");
  const tsBackend = createAsciiRendererBackend("ts-harri");
  await nativeBackend.prepare?.();
  await tsBackend.prepare?.();

  try {
    const layout: AsciiRenderLayout = { columns: 16, rows: 6 };
    const nativeRaster = nativeBackend.describeRaster(layout);
    const tsRaster = tsBackend.describeRaster(layout);
    assert.deepEqual(nativeRaster, tsRaster);

    const lumaPixels = new Uint8Array(nativeRaster.width * nativeRaster.height);
    const rgbPixels = new Uint8Array(tsRaster.width * tsRaster.height * 3);
    for (let y = 0; y < nativeRaster.height; y++) {
      for (let x = 0; x < nativeRaster.width; x++) {
        const value = Math.round((x / Math.max(1, nativeRaster.width - 1)) * 255);
        lumaPixels[y * nativeRaster.width + x] = value;
        const offset = (y * tsRaster.width + x) * 3;
        rgbPixels[offset] = value;
        rgbPixels[offset + 1] = value;
        rgbPixels[offset + 2] = value;
      }
    }

    const nativeResult = await nativeBackend.render({
      pixels: lumaPixels,
      width: nativeRaster.width,
      height: nativeRaster.height,
      layout,
    });
    const tsResult = await tsBackend.render({
      pixels: rgbPixels,
      width: tsRaster.width,
      height: tsRaster.height,
      layout,
    });
    assert.deepEqual(nativeResult.lines, tsResult.lines);
  } finally {
    await disposeBackends([nativeBackend, tsBackend]);
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

test("ts-harri uses the worker-backed path by default", async () => {
  const backend = createAsciiRendererBackend("ts-harri");
  await backend.prepare?.();

  try {
    assert.equal(backend.kind, "worker");
    assert.equal(backend.algorithm, "shape-lookup-rust-wasm-worker");
    const layout: AsciiRenderLayout = { columns: 8, rows: 4 };
    const raster = backend.describeRaster(layout);
    const result = await backend.render({
      pixels: new Uint8Array(raster.width * raster.height * 3),
      width: raster.width,
      height: raster.height,
      layout,
    });
    assert.equal(backend.kind, "worker");
    assert.notEqual(result.stats.timings.adapterMs, undefined);
  } finally {
    await disposeBackends([backend]);
  }
});

test("wasm-harri uses the direct wasm path", async () => {
  const backend = createAsciiRendererBackend("wasm-harri");
  await backend.prepare?.();

  try {
    assert.equal(backend.kind, "wasm");
    assert.equal(backend.algorithm, "shape-lookup-rust-wasm");
    const layout: AsciiRenderLayout = { columns: 8, rows: 4 };
    const raster = backend.describeRaster(layout);
    const result = await backend.render({
      pixels: new Uint8Array(raster.width * raster.height * 3),
      width: raster.width,
      height: raster.height,
      layout,
    });
    assert.equal(backend.kind, "wasm");
    assert.equal(result.backendId, "wasm-harri");
    assert.equal(result.stats.timings.adapterMs, undefined);
  } finally {
    await disposeBackends([backend]);
  }
});

test("ts-half-block emits truecolor ANSI for paired pixels", async () => {
  const backend = createAsciiRendererBackend("ts-half-block");
  const result = await backend.render({
    pixels: Uint8Array.from([
      255,
      32,
      16,
      12,
      200,
      240,
    ]),
    width: 1,
    height: 2,
    layout: { columns: 1, rows: 1 },
  });

  assert.equal(result.lines.length, 1);
  assert.match(result.lines[0], /\x1b\[48;2;255;32;16m/);
  assert.match(result.lines[0], /\x1b\[38;2;12;200;240m/);
  assert.equal(stripAnsi(result.lines[0]), "▄");
});

test("ts-harri dispose during in-flight render avoids unhandled rejections", async () => {
  const backend = createAsciiRendererBackend("ts-harri");
  await backend.prepare?.();

  const unhandled: string[] = [];
  const onUnhandled = (reason: unknown) => {
    unhandled.push(reason instanceof Error ? reason.message : String(reason));
  };

  process.on("unhandledRejection", onUnhandled);
  try {
    const layout: AsciiRenderLayout = { columns: 96, rows: 28 };
    const raster = backend.describeRaster(layout);
    const renderPromise = backend.render({
      pixels: new Uint8Array(raster.width * raster.height * 3),
      width: raster.width,
      height: raster.height,
      layout,
    });
    const disposePromise = backend.dispose?.() ?? Promise.resolve();

    await Promise.allSettled([renderPromise, disposePromise]);
    await new Promise((resolve) => setTimeout(resolve, 0));
    assert.deepEqual(unhandled, []);
  } finally {
    process.off("unhandledRejection", onUnhandled);
    await backend.dispose?.();
  }
});
