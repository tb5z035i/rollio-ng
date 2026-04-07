import { TypeScriptHalfBlockRenderer } from "./half-block.js";
import { WorkerThreadNativeRustRenderer } from "./native-rust.js";
import type { AsciiRendererBackend, AsciiRendererOptions } from "./types.js";

export const ASCII_RENDERER_IDS = ["ts-half-block", "native-rust"] as const;

export type AsciiRendererId = (typeof ASCII_RENDERER_IDS)[number];

export const ASCII_RENDERER_LABELS: Record<AsciiRendererId, string> = {
  "ts-half-block": "Half Block",
  "native-rust": "Rust (Native)",
};

export function createAsciiRendererBackend(
  id: AsciiRendererId,
  options: AsciiRendererOptions = {},
): AsciiRendererBackend {
  switch (id) {
    case "ts-half-block":
      return new TypeScriptHalfBlockRenderer();
    case "native-rust":
      return new WorkerThreadNativeRustRenderer(options);
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
  return "native-rust";
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
  AsciiPixelFormat,
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
