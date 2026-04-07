import {
  BENCHMARK_CASES,
  FIXTURES,
  benchmarkMatrix,
  createBackends,
  disposeBackends,
} from "./render-harness.js";

async function main(): Promise<void> {
  const backends = await createBackends();
  try {
    const measurements = await benchmarkMatrix(backends, FIXTURES, BENCHMARK_CASES);
    console.table(
      measurements.map((measurement) => ({
        fixture: measurement.fixture,
        case: measurement.caseName,
        backend: measurement.backendId,
        raster: `${measurement.rasterWidth}x${measurement.rasterHeight}`,
        output: `${measurement.outputColumns}x${measurement.outputRows}`,
        resizeMs: measurement.resizeMs.toFixed(2),
        renderMs: measurement.renderCallMs.toFixed(2),
        backendMs: measurement.backendMs.toFixed(2),
        sampleMs: measurement.sampleMs?.toFixed(2) ?? "n/a",
        lookupMs: measurement.lookupMs?.toFixed(2) ?? "n/a",
        assembleMs: measurement.assembleMs?.toFixed(2) ?? "n/a",
        ansiMs: measurement.ansiMs?.toFixed(2) ?? "n/a",
        adapterMs: measurement.adapterMs?.toFixed(2) ?? "n/a",
        outputBytes: measurement.outputBytes,
      })),
    );
  } finally {
    await disposeBackends(backends);
  }
}

void main();
