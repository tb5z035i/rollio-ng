import { TypeScriptHarriRenderer } from "./ts-harri.js";
import { TypeScriptHalfBlockRenderer } from "./half-block.js";
import type { AsciiRendererBackend } from "./types.js";

export const ASCII_RENDERER_IDS = [
  "ts-half-block",
  "ts-harri",
] as const;

export type AsciiRendererId = (typeof ASCII_RENDERER_IDS)[number];

export function createAsciiRendererBackend(
  id: AsciiRendererId,
): AsciiRendererBackend {
  switch (id) {
    case "ts-half-block":
      return new TypeScriptHalfBlockRenderer();
    case "ts-harri":
      return new TypeScriptHarriRenderer();
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
  return "ts-harri";
}

export function isAsciiRendererId(value: string): value is AsciiRendererId {
  return (ASCII_RENDERER_IDS as readonly string[]).includes(value);
}

export type {
  AsciiRenderInput,
  AsciiRenderLayout,
  AsciiRenderResult,
  AsciiRenderStats,
  AsciiRenderTimings,
  AsciiRendererBackend,
  AsciiRasterDimensions,
} from "./types.js";
export { WasmAsciiRendererAdapter, type AsciiWasmRendererModule } from "./wasm-adapter.js";
