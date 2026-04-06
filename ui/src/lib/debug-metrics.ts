import { performance } from "node:perf_hooks";

export type DebugGaugeValue = boolean | number | string;

interface TimingMetric {
  lastMs: number;
  totalMs: number;
  maxMs: number;
  samples: number;
  updatedAtMs: number;
}

interface GaugeMetric {
  value: DebugGaugeValue;
  updatedAtMs: number;
}

export interface TimingMetricSnapshot {
  lastMs: number;
  avgMs: number;
  maxMs: number;
  samples: number;
  updatedAgoMs: number;
}

export interface GaugeMetricSnapshot {
  value: DebugGaugeValue;
  updatedAgoMs: number;
}

export interface DebugSnapshot {
  capturedAtMs: number;
  timings: Record<string, TimingMetricSnapshot>;
  gauges: Record<string, GaugeMetricSnapshot>;
}

const timings = new Map<string, TimingMetric>();
const gauges = new Map<string, GaugeMetric>();

export function nowMs(): number {
  return performance.now();
}

export function recordTiming(name: string, durationMs: number): void {
  if (!Number.isFinite(durationMs) || durationMs < 0) return;

  const updatedAtMs = nowMs();
  const existing = timings.get(name);
  if (existing) {
    existing.lastMs = durationMs;
    existing.totalMs += durationMs;
    existing.maxMs = Math.max(existing.maxMs, durationMs);
    existing.samples += 1;
    existing.updatedAtMs = updatedAtMs;
    return;
  }

  timings.set(name, {
    lastMs: durationMs,
    totalMs: durationMs,
    maxMs: durationMs,
    samples: 1,
    updatedAtMs,
  });
}

export function setGauge(name: string, value: DebugGaugeValue): void {
  gauges.set(name, {
    value,
    updatedAtMs: nowMs(),
  });
}

export function incrementGauge(name: string, delta = 1): number {
  const existing = gauges.get(name)?.value;
  const nextValue = typeof existing === "number" ? existing + delta : delta;
  setGauge(name, nextValue);
  return nextValue;
}

export function snapshotDebugMetrics(): DebugSnapshot {
  const capturedAtMs = nowMs();
  const timingSnapshots: Record<string, TimingMetricSnapshot> = {};
  const gaugeSnapshots: Record<string, GaugeMetricSnapshot> = {};

  for (const [name, metric] of timings) {
    timingSnapshots[name] = {
      lastMs: metric.lastMs,
      avgMs: metric.totalMs / metric.samples,
      maxMs: metric.maxMs,
      samples: metric.samples,
      updatedAgoMs: capturedAtMs - metric.updatedAtMs,
    };
  }

  for (const [name, gauge] of gauges) {
    gaugeSnapshots[name] = {
      value: gauge.value,
      updatedAgoMs: capturedAtMs - gauge.updatedAtMs,
    };
  }

  return {
    capturedAtMs,
    timings: timingSnapshots,
    gauges: gaugeSnapshots,
  };
}
