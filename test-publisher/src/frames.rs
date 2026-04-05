/// Color bar frame generator for test publisher.
///
/// Generates SMPTE-style color bar patterns with a burned-in frame counter.
/// Writes directly into a caller-provided buffer to avoid per-frame allocation.

/// Standard 8-bar color pattern: white, yellow, cyan, green, magenta, red, blue, black.
const BAR_COLORS: [(u8, u8, u8); 8] = [
    (255, 255, 255), // white
    (255, 255, 0),   // yellow
    (0, 255, 255),   // cyan
    (0, 255, 0),     // green
    (255, 0, 255),   // magenta
    (255, 0, 0),     // red
    (0, 0, 255),     // blue
    (0, 0, 0),       // black
];

/// Simple 5×7 pixel font for digits 0-9.
/// Each digit is encoded as 7 rows of 5 bits (MSB-first, left-to-right).
#[rustfmt::skip]
const DIGIT_FONT: [[u8; 7]; 10] = [
    // 0
    [0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110],
    // 1
    [0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
    // 2
    [0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111],
    // 3
    [0b11111, 0b00010, 0b00100, 0b00010, 0b00001, 0b10001, 0b01110],
    // 4
    [0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010],
    // 5
    [0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110],
    // 6
    [0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110],
    // 7
    [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000],
    // 8
    [0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110],
    // 9
    [0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100],
];

const FONT_W: u32 = 5;
const FONT_H: u32 = 7;
const FONT_SCALE: u32 = 3; // each font pixel → 3×3 screen pixels
const CHAR_W: u32 = FONT_W * FONT_SCALE + FONT_SCALE; // +scale for inter-char spacing
const CHAR_H: u32 = FONT_H * FONT_SCALE;

/// Generate a color bar test pattern with a burned-in frame counter.
///
/// Writes RGB24 pixel data directly into `buf`.
/// `buf` must have length >= `width * height * 3`.
pub fn generate_color_bars(buf: &mut [u8], width: u32, height: u32, frame_index: u64) {
    let w = width as usize;
    let h = height as usize;
    debug_assert!(buf.len() >= w * h * 3);

    let bar_width = w / 8;

    // Fill color bars
    for y in 0..h {
        let row_offset = y * w * 3;
        for x in 0..w {
            let bar_idx = if bar_width > 0 {
                (x / bar_width).min(7)
            } else {
                0
            };
            let (r, g, b) = BAR_COLORS[bar_idx];
            let px = row_offset + x * 3;
            buf[px] = r;
            buf[px + 1] = g;
            buf[px + 2] = b;
        }
    }

    // Burn in frame counter at top-left with a dark background box
    burn_in_counter(buf, width, height, frame_index);
}

/// Burn in a frame counter number at the top-left of the frame.
fn burn_in_counter(buf: &mut [u8], width: u32, height: u32, frame_index: u64) {
    let text = frame_index.to_string();
    let digits: Vec<usize> = text
        .chars()
        .filter_map(|c| c.to_digit(10).map(|d| d as usize))
        .collect();

    let text_pixel_w = digits.len() as u32 * CHAR_W;
    let text_pixel_h = CHAR_H;
    let margin: u32 = FONT_SCALE * 2;

    // Background box dimensions
    let box_w = text_pixel_w + margin * 2;
    let box_h = text_pixel_h + margin * 2;

    if box_w > width || box_h > height {
        return; // Frame too small for text
    }

    // Draw dark semi-transparent background box
    for by in 0..box_h {
        for bx in 0..box_w {
            let px = ((by as usize) * (width as usize) + bx as usize) * 3;
            if px + 2 < buf.len() {
                // Dark background (25% of original)
                buf[px] = buf[px] / 4;
                buf[px + 1] = buf[px + 1] / 4;
                buf[px + 2] = buf[px + 2] / 4;
            }
        }
    }

    // Draw digits
    for (di, &digit) in digits.iter().enumerate() {
        let glyph = &DIGIT_FONT[digit];
        let dx = margin + di as u32 * CHAR_W;
        let dy = margin;

        for gy in 0..FONT_H {
            let row_bits = glyph[gy as usize];
            for gx in 0..FONT_W {
                if row_bits & (1 << (FONT_W - 1 - gx)) != 0 {
                    // Scale up the font pixel
                    for sy in 0..FONT_SCALE {
                        for sx in 0..FONT_SCALE {
                            let px_x = dx + gx * FONT_SCALE + sx;
                            let px_y = dy + gy * FONT_SCALE + sy;
                            if px_x < width && px_y < height {
                                let px =
                                    (px_y as usize * width as usize + px_x as usize) * 3;
                                if px + 2 < buf.len() {
                                    buf[px] = 255;
                                    buf[px + 1] = 255;
                                    buf[px + 2] = 255;
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

