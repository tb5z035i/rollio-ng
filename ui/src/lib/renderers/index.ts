import { TypeScriptHalfBlockRenderer } from "./half-block.js";
import { WorkerThreadHarriRenderer } from "./worker-harri.js";
import type { AsciiRendererBackend, AsciiRendererOptions } from "./types.js";

export const ASCII_RENDERER_IDS = [
  "ts-half-block",
  "ts-harri",
] as const;

export type AsciiRendererId = (typeof ASCII_RENDERER_IDS)[number];

export const ASCII_RENDERER_LABELS: Record<AsciiRendererId, string> = {
  "ts-half-block": "Half Block",
  "ts-harri": "Harri",
};

export function createAsciiRendererBackend(
  id: AsciiRendererId,
  options: AsciiRendererOptions = {},
): AsciiRendererBackend {
  switch (id) {
    case "ts-half-block":
      return new TypeScriptHalfBlockRenderer();
    case "ts-harri":
      return new WorkerThreadHarriRenderer(options);
  }
}

export function listAsciiRendererBackends(): AsciiRendererBackend[] {
  return ASCII_RENDERER_IDS.map((id) => createAsciiRendererBackend(id));
}

export function defaultAsciiRendererId(): AsciiRendererId {
  const selected = process.env.ROLLIO_ASCII_RENDERER;
  if (selected && isAsciiRendererId(selected)) {
    return selected;
  }
  return "ts-half-block";
}

export function isAsciiRendererId(value: string): value is AsciiRendererId {
  return (ASCII_RENDERER_IDS as readonly string[]).includes(value);
}

export function nextAsciiRendererId(current: AsciiRendererId): AsciiRendererId {
  const currentIndex = ASCII_RENDERER_IDS.indexOf(current);
  const nextIndex = (currentIndex + 1) % ASCII_RENDERER_IDS.length;
  return ASCII_RENDERER_IDS[nextIndex] ?? ASCII_RENDERER_IDS[0];
}

export function getAsciiRendererLabel(id: AsciiRendererId): string {
  return ASCII_RENDERER_LABELS[id];
}

export type {
  AsciiCellGeometry,
  AsciiRenderInput,
  AsciiRenderLayout,
  AsciiRenderResult,
  AsciiRenderStats,
  AsciiRenderTimings,
  AsciiRendererBackend,
  AsciiRendererOptions,
  AsciiRasterDimensions,
} from "./types.js";
export { WasmAsciiRendererAdapter, type AsciiWasmRendererModule } from "./wasm-adapter.js";
