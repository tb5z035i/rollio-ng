const UI_PIPELINE_LOG_INTERVAL_MS = 10_000;

interface MetricStats {
  count: number;
  sum: number;
  min: number;
  max: number;
}

interface CameraPipelineStats {
  wsPackets: number;
  decodedFrames: number;
  displayedFrames: number;
  keyframes: number;
  payloadBytes: number;
  lastSequence: number | null;
  wsReceiveAgeMs: MetricStats;
  wsReceiveGapMs: MetricStats;
  decodeWaitMs: MetricStats;
  decodeOutputAgeMs: MetricStats;
  decodeQueueSize: MetricStats;
  displayAgeMs: MetricStats;
  decodeToDisplayMs: MetricStats;
  canvasDrawMs: MetricStats;
  lastWsReceiveAtMs: number | null;
}

interface WsPacketObservation {
  name: string;
  sequence: number;
  sourceTimestampUs: number;
  payloadBytes: number;
  isKeyframe: boolean;
  receivedAtWallTimeMs: number;
}

interface DecodeSubmitObservation {
  name: string;
  queueSizeBefore?: number;
  queueSizeAfter?: number;
}

interface DecodeOutputObservation {
  name: string;
  sourceTimestampUs: number;
  submittedAtWallTimeMs?: number;
  outputAtWallTimeMs: number;
  queueSize?: number;
}

interface DisplayObservation {
  name: string;
  sourceTimestampUs: number;
  decodedAtWallTimeMs?: number;
  displayedAtWallTimeMs: number;
  canvasDrawMs: number;
}

let enabled = false;
let lastLogAtMs = performance.now();
const cameras = new Map<string, CameraPipelineStats>();

export function setUiPipelineLoggingEnabled(nextEnabled: boolean): void {
  if (enabled === nextEnabled) {
    return;
  }
  enabled = nextEnabled;
  reset();
  if (enabled) {
    console.info("[rollio-ui pipeline] advanced pipeline logs enabled");
  }
}

export function observeWsPacket(observation: WsPacketObservation): void {
  if (!enabled) {
    return;
  }
  const stats = cameraStats(observation.name);
  stats.wsPackets += 1;
  stats.payloadBytes += observation.payloadBytes;
  stats.lastSequence = observation.sequence;
  if (observation.isKeyframe) {
    stats.keyframes += 1;
  }
  if (observation.sourceTimestampUs !== 0) {
    observe(
      stats.wsReceiveAgeMs,
      Math.max(0, observation.receivedAtWallTimeMs - observation.sourceTimestampUs / 1_000),
    );
  }
  if (stats.lastWsReceiveAtMs !== null) {
    observe(
      stats.wsReceiveGapMs,
      observation.receivedAtWallTimeMs - stats.lastWsReceiveAtMs,
    );
  }
  stats.lastWsReceiveAtMs = observation.receivedAtWallTimeMs;
  maybeLog();
}

export function observeDecodeSubmit(observation: DecodeSubmitObservation): void {
  if (!enabled) {
    return;
  }
  const stats = cameraStats(observation.name);
  const queueSize = observation.queueSizeAfter ?? observation.queueSizeBefore;
  if (typeof queueSize === "number") {
    observe(stats.decodeQueueSize, queueSize);
  }
  maybeLog();
}

export function observeDecodeOutput(observation: DecodeOutputObservation): void {
  if (!enabled) {
    return;
  }
  const stats = cameraStats(observation.name);
  stats.decodedFrames += 1;
  if (observation.sourceTimestampUs !== 0) {
    observe(
      stats.decodeOutputAgeMs,
      Math.max(0, observation.outputAtWallTimeMs - observation.sourceTimestampUs / 1_000),
    );
  }
  if (typeof observation.submittedAtWallTimeMs === "number") {
    observe(
      stats.decodeWaitMs,
      Math.max(0, observation.outputAtWallTimeMs - observation.submittedAtWallTimeMs),
    );
  }
  if (typeof observation.queueSize === "number") {
    observe(stats.decodeQueueSize, observation.queueSize);
  }
  maybeLog();
}

export function observeDisplay(observation: DisplayObservation): void {
  if (!enabled) {
    return;
  }
  const stats = cameraStats(observation.name);
  stats.displayedFrames += 1;
  if (observation.sourceTimestampUs !== 0) {
    observe(
      stats.displayAgeMs,
      Math.max(0, observation.displayedAtWallTimeMs - observation.sourceTimestampUs / 1_000),
    );
  }
  if (typeof observation.decodedAtWallTimeMs === "number") {
    observe(
      stats.decodeToDisplayMs,
      Math.max(0, observation.displayedAtWallTimeMs - observation.decodedAtWallTimeMs),
    );
  }
  observe(stats.canvasDrawMs, observation.canvasDrawMs);
  maybeLog();
}

function reset(): void {
  cameras.clear();
  lastLogAtMs = performance.now();
}

function cameraStats(name: string): CameraPipelineStats {
  const existing = cameras.get(name);
  if (existing) {
    return existing;
  }
  const created: CameraPipelineStats = {
    wsPackets: 0,
    decodedFrames: 0,
    displayedFrames: 0,
    keyframes: 0,
    payloadBytes: 0,
    lastSequence: null,
    wsReceiveAgeMs: emptyStats(),
    wsReceiveGapMs: emptyStats(),
    decodeWaitMs: emptyStats(),
    decodeOutputAgeMs: emptyStats(),
    decodeQueueSize: emptyStats(),
    displayAgeMs: emptyStats(),
    decodeToDisplayMs: emptyStats(),
    canvasDrawMs: emptyStats(),
    lastWsReceiveAtMs: null,
  };
  cameras.set(name, created);
  return created;
}

function emptyStats(): MetricStats {
  return {
    count: 0,
    sum: 0,
    min: 0,
    max: 0,
  };
}

function observe(stats: MetricStats, value: number): void {
  if (!Number.isFinite(value) || value < 0) {
    return;
  }
  if (stats.count === 0) {
    stats.min = value;
    stats.max = value;
  } else {
    stats.min = Math.min(stats.min, value);
    stats.max = Math.max(stats.max, value);
  }
  stats.sum += value;
  stats.count += 1;
}

function summary(stats: MetricStats): string {
  if (stats.count === 0) {
    return "n/a";
  }
  return `${(stats.sum / stats.count).toFixed(1)}/${stats.min.toFixed(1)}/${stats.max.toFixed(1)}`;
}

function maybeLog(): void {
  const now = performance.now();
  if (now - lastLogAtMs < UI_PIPELINE_LOG_INTERVAL_MS) {
    return;
  }
  for (const [name, stats] of cameras) {
    console.info(
      `[rollio-ui pipeline] camera=${name} ws_packets=${stats.wsPackets} ` +
        `decoded=${stats.decodedFrames} displayed=${stats.displayedFrames} ` +
        `keyframes=${stats.keyframes} bytes=${stats.payloadBytes} ` +
        `ws_receive_age_ms=${summary(stats.wsReceiveAgeMs)} ` +
        `ws_receive_gap_ms=${summary(stats.wsReceiveGapMs)} ` +
        `decode_output_age_ms=${summary(stats.decodeOutputAgeMs)} ` +
        `decode_wait_ms=${summary(stats.decodeWaitMs)} ` +
        `decode_queue_size=${summary(stats.decodeQueueSize)} ` +
        `display_age_ms=${summary(stats.displayAgeMs)} ` +
        `decode_to_display_ms=${summary(stats.decodeToDisplayMs)} ` +
        `canvas_draw_ms=${summary(stats.canvasDrawMs)} ` +
        `last_sequence=${stats.lastSequence ?? "n/a"}`,
    );
  }
  reset();
}
