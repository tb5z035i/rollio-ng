import {
  DEFAULT_ASCII_CELL_GEOMETRY,
  type AsciiCellGeometry,
  type AsciiRenderLayout,
  type AsciiRendererOptions,
  type AsciiRasterDimensions,
} from "./types.js";

export const HARRI_DEFAULT_CELL_WIDTH = 8;
export const HARRI_MIN_CELL_HEIGHT = 8;
export const HARRI_MAX_CELL_HEIGHT = 32;

export class HarriGeometry {
  readonly cellWidth: number;
  readonly cellHeight: number;

  constructor(options: AsciiRendererOptions = {}) {
    const geometry = normalizeCellGeometry(options.cellGeometry);
    this.cellWidth = HARRI_DEFAULT_CELL_WIDTH;
    this.cellHeight = clampCellHeight(
      Math.round((this.cellWidth * geometry.pixelHeight) / geometry.pixelWidth),
    );
  }

  describeRaster(layout: AsciiRenderLayout): AsciiRasterDimensions {
    return {
      width: layout.columns * this.cellWidth,
      height: layout.rows * this.cellHeight,
    };
  }

  layoutForRaster(raster: AsciiRasterDimensions): AsciiRenderLayout {
    return {
      columns: Math.max(1, Math.ceil(raster.width / this.cellWidth)),
      rows: Math.max(1, Math.ceil(raster.height / this.cellHeight)),
    };
  }
}

function normalizeCellGeometry(geometry?: AsciiCellGeometry): AsciiCellGeometry {
  const candidate = geometry ?? DEFAULT_ASCII_CELL_GEOMETRY;
  const pixelWidth =
    Number.isFinite(candidate.pixelWidth) && candidate.pixelWidth > 0
      ? candidate.pixelWidth
      : DEFAULT_ASCII_CELL_GEOMETRY.pixelWidth;
  const pixelHeight =
    Number.isFinite(candidate.pixelHeight) && candidate.pixelHeight > 0
      ? candidate.pixelHeight
      : DEFAULT_ASCII_CELL_GEOMETRY.pixelHeight;

  return {
    pixelWidth,
    pixelHeight,
  };
}

function clampCellHeight(value: number): number {
  return Math.max(HARRI_MIN_CELL_HEIGHT, Math.min(HARRI_MAX_CELL_HEIGHT, value));
}
