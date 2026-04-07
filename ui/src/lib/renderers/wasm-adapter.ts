import type {
  AsciiRenderInput,
  AsciiRenderLayout,
  AsciiRenderResult,
  AsciiRendererBackend,
  AsciiRasterDimensions,
} from "./types.js";

export interface AsciiWasmRendererModule {
  describeRaster(layout: AsciiRenderLayout): AsciiRasterDimensions;
  layoutForRaster(raster: AsciiRasterDimensions): AsciiRenderLayout;
  prepare?(): Promise<void>;
  render(input: AsciiRenderInput): Promise<AsciiRenderResult> | AsciiRenderResult;
  dispose?(): Promise<void>;
}

export class WasmAsciiRendererAdapter implements AsciiRendererBackend {
  readonly kind = "wasm" as const;
  readonly algorithm: string;
  readonly pixelFormat = "rgb24" as const;

  constructor(
    readonly id: string,
    readonly label: string,
    private readonly module: AsciiWasmRendererModule,
    algorithm = "wasm-renderer",
  ) {
    this.algorithm = algorithm;
  }

  describeRaster(layout: AsciiRenderLayout): AsciiRasterDimensions {
    return this.module.describeRaster(layout);
  }

  layoutForRaster(raster: AsciiRasterDimensions): AsciiRenderLayout {
    return this.module.layoutForRaster(raster);
  }

  async prepare(): Promise<void> {
    await this.module.prepare?.();
  }

  async render(input: AsciiRenderInput): Promise<AsciiRenderResult> {
    return await this.module.render(input);
  }

  async dispose(): Promise<void> {
    await this.module.dispose?.();
  }
}
