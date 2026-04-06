/**
 * ANSI half-block renderer: converts raw RGB pixels into ANSI 256-color
 * terminal art using the lower half-block character (▄ U+2584).
 *
 * Each terminal character cell represents 1 column × 2 rows of pixels:
 * - Background color → top pixel
 * - Foreground color → bottom pixel
 * - Character → ▄ (lower half block)
 *
 * Optimizations:
 * - O(1) color lookup via pre-computed LUT (color-palette.ts)
 * - Reuses precomputed SGR strings for ANSI 256 colors
 * - Emits fg/bg codes only when the color actually changes within a row
 *
 * Note: Each rendered row ends with a reset, so tracking fg/bg state within
 * a row is still safe and avoids color bleed across rows while cutting down
 * the amount of ANSI output that Ink has to push to the terminal.
 */

import { nearestAnsi256 } from "./color-palette.js";

const HALF_BLOCK = "\u2584";
const RESET = "\x1b[0m";
const FG_SGR = Array.from({ length: 256 }, (_, idx) => `\x1b[38;5;${idx}m`);
const BG_SGR = Array.from({ length: 256 }, (_, idx) => `\x1b[48;5;${idx}m`);

export interface AnsiRenderResult {
  lines: string[];
  cellCount: number;
  sgrChangeCount: number;
}

/**
 * Render RGB pixel data as an array of ANSI 256-color half-block lines.
 *
 * Returns one string per character row (no newlines within strings).
 * Each string contains exactly `imgWidth` visible characters plus
 * invisible ANSI escape sequences.
 *
 * @param rgbPixels - Raw RGB24 pixel data (3 bytes per pixel, row-major)
 * @param imgWidth  - Width of the image in pixels
 * @param imgHeight - Height of the image in pixels (should be even)
 * @returns ANSI-escaped rows plus basic output complexity stats
 */
export function renderToAnsiLines(
  rgbPixels: Buffer | Uint8Array,
  imgWidth: number,
  imgHeight: number,
): AnsiRenderResult {
  const charRows = Math.floor(imgHeight / 2);
  if (charRows === 0 || imgWidth === 0) {
    return {
      lines: [],
      cellCount: 0,
      sgrChangeCount: 0,
    };
  }

  const lines = new Array<string>(charRows);
  const maxPartsPerRow = imgWidth * 3 + 1;
  let sgrChangeCount = 0;
  const pixels = rgbPixels;
  const rowStride = imgWidth * 3;

  for (let cy = 0; cy < charRows; cy++) {
    const topRowStart = cy * 2 * rowStride;
    const botRowStart = topRowStart + rowStride;
    const parts = new Array<string>(maxPartsPerRow);
    let partCount = 0;
    let previousBgAnsi = -1;
    let previousFgAnsi = -1;
    let topIdx = topRowStart;
    let botIdx = botRowStart;

    for (let x = 0; x < imgWidth; x++) {
      const bgAnsi = nearestAnsi256(
        pixels[topIdx],
        pixels[topIdx + 1],
        pixels[topIdx + 2],
      );

      const fgAnsi = nearestAnsi256(
        pixels[botIdx],
        pixels[botIdx + 1],
        pixels[botIdx + 2],
      );

      if (bgAnsi !== previousBgAnsi) {
        parts[partCount++] = BG_SGR[bgAnsi];
        previousBgAnsi = bgAnsi;
        sgrChangeCount += 1;
      }
      if (fgAnsi !== previousFgAnsi) {
        parts[partCount++] = FG_SGR[fgAnsi];
        previousFgAnsi = fgAnsi;
        sgrChangeCount += 1;
      }
      parts[partCount++] = HALF_BLOCK;
      topIdx += 3;
      botIdx += 3;
    }

    parts[partCount++] = RESET;
    parts.length = partCount;
    lines[cy] = parts.join("");
  }

  return {
    lines,
    cellCount: charRows * imgWidth,
    sgrChangeCount,
  };
}
