/// Color bar frame generator for test publisher.
///
/// Generates time-varying SMPTE-style color bar patterns with a scrolling
/// gradient and burned-in timestamp overlay. The pattern changes every frame
/// so the UI clearly shows dynamic updates.
///
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

/// Simple 5×7 pixel font for digits 0-9 and colon/dot.
/// Each glyph is 7 rows of 5 bits (MSB-first).
#[rustfmt::skip]
const DIGIT_FONT: [[u8; 7]; 12] = [
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
    // 10 = ':' (colon)
    [0b00000, 0b00100, 0b00000, 0b00000, 0b00000, 0b00100, 0b00000],
    // 11 = '.' (dot)
    [0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00100],
];

const FONT_W: u32 = 5;
const FONT_H: u32 = 7;
const FONT_SCALE: u32 = 2; // each font pixel → 2×2 screen pixels
const CHAR_W: u32 = (FONT_W + 1) * FONT_SCALE; // +1 for inter-char spacing
const CHAR_H: u32 = FONT_H * FONT_SCALE;

/// Generate a time-varying color bar test pattern with timestamp overlay.
///
/// The pattern changes over time:
/// - Color bars scroll horizontally at a rate proportional to elapsed time
/// - A diagonal gradient is overlaid that shifts with each frame
/// - A timestamp string (HH:MM:SS.ff) is burned in at center-bottom
///
/// `elapsed_secs`: seconds since publisher started (drives the animation)
/// `frame_index`: current frame number (shown in overlay)
pub fn generate_color_bars(
    buf: &mut [u8],
    width: u32,
    height: u32,
    elapsed_secs: f64,
    frame_index: u64,
) {
    let w = width as usize;
    let h = height as usize;
    debug_assert!(buf.len() >= w * h * 3);

    let bar_width = (w / 8).max(1);
    // Scroll offset: bars shift right over time (1 bar width per second)
    let scroll = (elapsed_secs * bar_width as f64) as usize;

    for y in 0..h {
        let row_offset = y * w * 3;
        // Vertical brightness modulation: subtle diagonal gradient that moves over time
        let v_mod = ((y as f64 / h as f64) * 0.3 + elapsed_secs * 0.1).sin() * 0.15 + 0.85;

        for x in 0..w {
            // Scroll the bars
            let sx = (x + scroll) % w;
            let bar_idx = (sx / bar_width).min(7);
            let (r, g, b) = BAR_COLORS[bar_idx];

            // Apply vertical brightness modulation
            let px = row_offset + x * 3;
            buf[px] = (r as f64 * v_mod).min(255.0) as u8;
            buf[px + 1] = (g as f64 * v_mod).min(255.0) as u8;
            buf[px + 2] = (b as f64 * v_mod).min(255.0) as u8;
        }
    }

    // Burn in timestamp at center-bottom
    burn_in_timestamp(buf, width, height, elapsed_secs, frame_index);
}

/// Burn a timestamp string into the frame at center-bottom.
fn burn_in_timestamp(buf: &mut [u8], width: u32, height: u32, elapsed_secs: f64, frame_index: u64) {
    // Format: HH:MM:SS.ff #NNNNN
    let total_secs = elapsed_secs as u64;
    let hours = total_secs / 3600;
    let minutes = (total_secs % 3600) / 60;
    let seconds = total_secs % 60;
    let centisecs = ((elapsed_secs.fract()) * 100.0) as u64;

    let time_str = format!(
        "{:02}:{:02}:{:02}.{:02}",
        hours, minutes, seconds, centisecs
    );
    let frame_str = format!("#{frame_index}");
    let full_str = format!("{time_str}  {frame_str}");

    let glyphs = str_to_glyphs(&full_str);
    let text_pixel_w = glyphs.len() as u32 * CHAR_W;
    let text_pixel_h = CHAR_H;
    let margin: u32 = FONT_SCALE * 2;

    let box_w = text_pixel_w + margin * 2;
    let box_h = text_pixel_h + margin * 2;

    if box_w > width || box_h > height {
        return;
    }

    // Position: center-bottom
    let box_x = (width - box_w) / 2;
    let box_y = height - box_h - margin;

    // Draw semi-transparent dark background
    for by in 0..box_h {
        for bx in 0..box_w {
            let px_x = box_x + bx;
            let px_y = box_y + by;
            let px = (px_y as usize * width as usize + px_x as usize) * 3;
            if px + 2 < buf.len() {
                buf[px] /= 3;
                buf[px + 1] /= 3;
                buf[px + 2] /= 3;
            }
        }
    }

    // Draw text
    let text_x = box_x + margin;
    let text_y = box_y + margin;
    draw_text(buf, width, height, text_x, text_y, &glyphs);
}

/// Convert a string of digits, colons, dots, spaces, and '#' to glyph indices.
fn str_to_glyphs(s: &str) -> Vec<Option<usize>> {
    s.chars()
        .map(|c| match c {
            '0'..='9' => Some(c as usize - '0' as usize),
            ':' => Some(10),
            '.' => Some(11),
            '#' => None, // draw as a special char
            ' ' => None, // space = blank glyph
            _ => None,
        })
        .collect()
}

/// Draw text glyphs at a position.
fn draw_text(
    buf: &mut [u8],
    width: u32,
    height: u32,
    start_x: u32,
    start_y: u32,
    glyphs: &[Option<usize>],
) {
    for (di, glyph_idx) in glyphs.iter().enumerate() {
        let dx = start_x + di as u32 * CHAR_W;

        let glyph = match glyph_idx {
            Some(idx) => &DIGIT_FONT[*idx],
            None => {
                // Space or unsupported char — skip
                continue;
            }
        };

        for gy in 0..FONT_H {
            let row_bits = glyph[gy as usize];
            for gx in 0..FONT_W {
                if row_bits & (1 << (FONT_W - 1 - gx)) != 0 {
                    for sy in 0..FONT_SCALE {
                        for sx in 0..FONT_SCALE {
                            let px_x = dx + gx * FONT_SCALE + sx;
                            let px_y = start_y + gy * FONT_SCALE + sy;
                            if px_x < width && px_y < height {
                                let px = (px_y as usize * width as usize + px_x as usize) * 3;
                                if px + 2 < buf.len() {
                                    // White text with a slight green tint for visibility
                                    buf[px] = 220;
                                    buf[px + 1] = 255;
                                    buf[px + 2] = 220;
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
