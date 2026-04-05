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
 * - Batches same-color runs to reduce escape code count
 * - Pre-allocates output array based on terminal dimensions
 */

import { nearestAnsi256 } from "./color-palette.js";

const HALF_BLOCK = "\u2584";
const RESET = "\x1b[0m";

/**
 * Render RGB pixel data as ANSI 256-color half-block art.
 *
 * The input `rgbPixels` should already be resized to the target resolution
 * (done by sharp on the decode side). The renderer just maps pixels to
 * terminal characters.
 *
 * @param rgbPixels - Raw RGB24 pixel data (3 bytes per pixel, row-major)
 * @param imgWidth  - Width of the image in pixels
 * @param imgHeight - Height of the image in pixels (should be even for best results)
 * @returns ANSI-escaped string ready for terminal output
 */
export function renderToAnsi(
  rgbPixels: Buffer | Uint8Array,
  imgWidth: number,
  imgHeight: number,
): string {
  // Each character row represents 2 pixel rows
  const charRows = Math.floor(imgHeight / 2);
  if (charRows === 0 || imgWidth === 0) return "";

  // Pre-allocate parts array: each row has at most imgWidth cells + reset + newline
  const parts: string[] = [];

  for (let cy = 0; cy < charRows; cy++) {
    const topRowY = cy * 2;
    const botRowY = cy * 2 + 1;

    let prevBg = -1;
    let prevFg = -1;

    for (let x = 0; x < imgWidth; x++) {
      // Top pixel → background color
      const topIdx = (topRowY * imgWidth + x) * 3;
      const bgAnsi = nearestAnsi256(
        rgbPixels[topIdx],
        rgbPixels[topIdx + 1],
        rgbPixels[topIdx + 2],
      );

      // Bottom pixel → foreground color
      const botIdx = (botRowY * imgWidth + x) * 3;
      const fgAnsi = nearestAnsi256(
        rgbPixels[botIdx],
        rgbPixels[botIdx + 1],
        rgbPixels[botIdx + 2],
      );

      // Only emit escape codes when colors change (batching optimization)
      if (bgAnsi !== prevBg && fgAnsi !== prevFg) {
        parts.push(`\x1b[48;5;${bgAnsi};38;5;${fgAnsi}m${HALF_BLOCK}`);
      } else if (bgAnsi !== prevBg) {
        parts.push(`\x1b[48;5;${bgAnsi}m${HALF_BLOCK}`);
      } else if (fgAnsi !== prevFg) {
        parts.push(`\x1b[38;5;${fgAnsi}m${HALF_BLOCK}`);
      } else {
        parts.push(HALF_BLOCK);
      }

      prevBg = bgAnsi;
      prevFg = fgAnsi;
    }

    parts.push(RESET);
    if (cy < charRows - 1) {
      parts.push("\n");
    }
  }

  return parts.join("");
}
