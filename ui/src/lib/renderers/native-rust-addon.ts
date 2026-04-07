import { createRequire } from "node:module";
import { existsSync } from "node:fs";
import { fileURLToPath } from "node:url";

export interface NativeAsciiAddonModule {
  NativeAsciiRenderer: new (
    cellWidth: number,
    cellHeight: number,
    glyphChars: Uint8Array,
    glyphVectors: Uint8Array,
    vectorSize: number,
  ) => {
    render(
      pixels: Uint8Array,
      width: number,
      height: number,
      columns: number,
      rows: number,
    ): {
      lines: string[];
      stats: {
        totalMs: number;
        sampleCount: number;
        lookupCount: number;
        cacheHits: number;
        cacheMisses: number;
        cellCount: number;
        outputBytes: number;
        sgrChangeCount?: number;
        assembleMs?: number;
      };
    };
  };
}

function resolveNativeAsciiAddonUrl(): URL {
  return new URL("../../../native/rollio-native-ascii.node", import.meta.url);
}

export function loadNativeAsciiAddon(): NativeAsciiAddonModule {
  const addonPath = fileURLToPath(resolveNativeAsciiAddonUrl());
  if (!existsSync(addonPath)) {
    throw new Error(`Native ASCII addon not found at ${addonPath}`);
  }
  const require = createRequire(import.meta.url);
  return require(addonPath) as NativeAsciiAddonModule;
}
