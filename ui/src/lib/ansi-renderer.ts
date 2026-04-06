/**
 * ANSI half-block renderer: converts raw RGB pixels into terminal art using
 * the lower half-block character (▄ U+2584).
 *
 * Each terminal character cell represents 1 column × 2 rows of pixels:
 * - Background color → top pixel
 * - Foreground color → bottom pixel
 * - Character → ▄ (lower half block)
 *
 * Optimizations:
 * - Supports both 24-bit truecolor and ANSI 256-color output
 * - Emits fg/bg codes only when the color actually changes within a row
 * - Reuses precomputed ANSI 256 SGR strings when palette mode is selected
 * - Reuses precomputed decimal strings for truecolor SGR assembly
 *
 * Note: Each rendered row ends with a reset, so tracking fg/bg state within
 * a row is still safe and avoids color bleed across rows while cutting down
 * the amount of ANSI output that Ink has to push to the terminal.
 */

import { nearestAnsi256 } from "./color-palette.js";

const HALF_BLOCK = "\u2584";
const RESET = "\x1b[0m";
const TRUECOLOR_FG_PREFIX = "\x1b[38;2;";
const TRUECOLOR_BG_PREFIX = "\x1b[48;2;";
const DECIMAL_COMPONENT = Array.from({ length: 256 }, (_, idx) => String(idx));
const FG_SGR = Array.from({ length: 256 }, (_, idx) => `\x1b[38;5;${idx}m`);
const BG_SGR = Array.from({ length: 256 }, (_, idx) => `\x1b[48;5;${idx}m`);

export type AnsiColorMode = "truecolor" | "ansi256";

export interface AnsiRenderOptions {
  colorMode?: AnsiColorMode;
}

export interface AnsiRenderResult {
  lines: string[];
  cellCount: number;
  sgrChangeCount: number;
}

/**
 * Render RGB pixel data as an array of ANSI half-block lines.
 *
 * Returns one string per character row (no newlines within strings).
 * Each string contains exactly `imgWidth` visible characters plus
 * invisible ANSI escape sequences.
 *
 * @param rgbPixels - Raw RGB24 pixel data (3 bytes per pixel, row-major)
 * @param imgWidth  - Width of the image in pixels
 * @param imgHeight - Height of the image in pixels (should be even)
 * @param options   - Color output mode; defaults to full 24-bit truecolor
 * @returns ANSI-escaped rows plus basic output complexity stats
 */
export function renderToAnsiLines(
  rgbPixels: Buffer | Uint8Array,
  imgWidth: number,
  imgHeight: number,
  options: AnsiRenderOptions = {},
): AnsiRenderResult {
  const colorMode = options.colorMode ?? "truecolor";
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
    let topIdx = topRowStart;
    let botIdx = botRowStart;
    if (colorMode === "truecolor") {
      let previousBgR = -1;
      let previousBgG = -1;
      let previousBgB = -1;
      let previousFgR = -1;
      let previousFgG = -1;
      let previousFgB = -1;

      for (let x = 0; x < imgWidth; x++) {
        const bgR = pixels[topIdx];
        const bgG = pixels[topIdx + 1];
        const bgB = pixels[topIdx + 2];
        const fgR = pixels[botIdx];
        const fgG = pixels[botIdx + 1];
        const fgB = pixels[botIdx + 2];

        if (bgR !== previousBgR || bgG !== previousBgG || bgB !== previousBgB) {
          parts[partCount++] =
            TRUECOLOR_BG_PREFIX +
            DECIMAL_COMPONENT[bgR] +
            ";" +
            DECIMAL_COMPONENT[bgG] +
            ";" +
            DECIMAL_COMPONENT[bgB] +
            "m";
          previousBgR = bgR;
          previousBgG = bgG;
          previousBgB = bgB;
          sgrChangeCount += 1;
        }
        if (fgR !== previousFgR || fgG !== previousFgG || fgB !== previousFgB) {
          parts[partCount++] =
            TRUECOLOR_FG_PREFIX +
            DECIMAL_COMPONENT[fgR] +
            ";" +
            DECIMAL_COMPONENT[fgG] +
            ";" +
            DECIMAL_COMPONENT[fgB] +
            "m";
          previousFgR = fgR;
          previousFgG = fgG;
          previousFgB = fgB;
          sgrChangeCount += 1;
        }
        parts[partCount++] = HALF_BLOCK;
        topIdx += 3;
        botIdx += 3;
      }
    } else {
      let previousBgAnsi = -1;
      let previousFgAnsi = -1;

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
