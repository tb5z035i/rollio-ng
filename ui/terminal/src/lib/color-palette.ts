/**
 * ANSI 256-color palette with pre-computed lookup table.
 *
 * Performance: O(1) per-pixel color mapping via a 32768-entry LUT
 * (5-bit quantized RGB → nearest ANSI 256 color index).
 * This is ~100× faster than brute-force O(256) search per pixel.
 */

/** The full 256-color ANSI palette as [R, G, B] triples. */
const PALETTE: Array<[number, number, number]> = [];

// Standard 16 system colors (indices 0-15)
const SYSTEM_COLORS: Array<[number, number, number]> = [
  [0, 0, 0],
  [128, 0, 0],
  [0, 128, 0],
  [128, 128, 0],
  [0, 0, 128],
  [128, 0, 128],
  [0, 128, 128],
  [192, 192, 192],
  [128, 128, 128],
  [255, 0, 0],
  [0, 255, 0],
  [255, 255, 0],
  [0, 0, 255],
  [255, 0, 255],
  [0, 255, 255],
  [255, 255, 255],
];

for (const c of SYSTEM_COLORS) {
  PALETTE.push(c);
}

// 6×6×6 color cube (indices 16-231)
const CUBE_STEPS = [0, 95, 135, 175, 215, 255];
for (let r = 0; r < 6; r++) {
  for (let g = 0; g < 6; g++) {
    for (let b = 0; b < 6; b++) {
      PALETTE.push([CUBE_STEPS[r], CUBE_STEPS[g], CUBE_STEPS[b]]);
    }
  }
}

// 24-step grayscale ramp (indices 232-255)
for (let i = 0; i < 24; i++) {
  const v = 8 + i * 10;
  PALETTE.push([v, v, v]);
}

/**
 * Pre-computed lookup table: 32768 entries mapping 5-bit quantized RGB
 * to the nearest ANSI 256 color index (searching only indices 16-255
 * to avoid inconsistent system colors).
 */
const LUT = new Uint8Array(32768);

// Build the LUT at module load time
(function buildLUT() {
  for (let ri = 0; ri < 32; ri++) {
    const r = (ri << 3) | (ri >> 2); // expand 5-bit to 8-bit
    for (let gi = 0; gi < 32; gi++) {
      const g = (gi << 3) | (gi >> 2);
      for (let bi = 0; bi < 32; bi++) {
        const b = (bi << 3) | (bi >> 2);
        const idx = (ri << 10) | (gi << 5) | bi;

        let bestDist = Infinity;
        let bestIdx = 16;

        // Search only extended palette (16-255), skip system colors
        for (let pi = 16; pi < 256; pi++) {
          const [pr, pg, pb] = PALETTE[pi];
          const dr = r - pr;
          const dg = g - pg;
          const db = b - pb;
          const dist = dr * dr + dg * dg + db * db;
          if (dist < bestDist) {
            bestDist = dist;
            bestIdx = pi;
            if (dist === 0) break;
          }
        }

        LUT[idx] = bestIdx;
      }
    }
  }
})();

/**
 * Map an RGB color to the nearest ANSI 256-color index.
 * O(1) lookup via pre-computed table.
 */
export function nearestAnsi256(r: number, g: number, b: number): number {
  const idx = ((r >> 3) << 10) | ((g >> 3) << 5) | (b >> 3);
  return LUT[idx];
}
