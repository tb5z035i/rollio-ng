use std::cell::RefCell;
use std::collections::HashMap;
use std::slice;

const INTERNAL_MASK_COUNT: usize = 6;
const EXTERNAL_MASK_COUNT: usize = 10;
const LOOKUP_RANGE: u32 = 8;
const GLOBAL_CONTRAST_EXPONENT: f32 = 1.55;
const DIRECTIONAL_CONTRAST_EXPONENT: f32 = 1.45;
const RESET_BYTES: &[u8] = b"\x1b[0m";

const INTERNAL_CIRCLES: [(f32, f32, f32); INTERNAL_MASK_COUNT] = [
    (0.24, 0.18, 0.24),
    (0.76, 0.18, 0.24),
    (0.18, 0.50, 0.24),
    (0.82, 0.50, 0.24),
    (0.24, 0.82, 0.24),
    (0.76, 0.82, 0.24),
];

const EXTERNAL_CIRCLES: [(f32, f32, f32); EXTERNAL_MASK_COUNT] = [
    (0.20, -0.12, 0.24),
    (0.80, -0.12, 0.24),
    (-0.12, 0.20, 0.24),
    (1.12, 0.20, 0.24),
    (-0.12, 0.50, 0.24),
    (1.12, 0.50, 0.24),
    (-0.12, 0.80, 0.24),
    (1.12, 0.80, 0.24),
    (0.20, 1.12, 0.24),
    (0.80, 1.12, 0.24),
];

const AFFECTING_EXTERNAL_INDICES: [&[usize]; INTERNAL_MASK_COUNT] = [
    &[0, 1, 2, 4],
    &[0, 1, 3, 5],
    &[2, 4, 6],
    &[3, 5, 7],
    &[4, 6, 8, 9],
    &[5, 7, 8, 9],
];

thread_local! {
    static NEXT_HANDLE: RefCell<u32> = const { RefCell::new(1) };
    static RENDERERS: RefCell<HashMap<u32, Renderer>> = RefCell::new(HashMap::new());
    static LAST_ERROR: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

#[derive(Clone, Copy)]
struct SamplePoint {
    dx: i32,
    dy: i32,
}

#[derive(Clone, Copy)]
struct Glyph {
    ch: u8,
    vector: [f32; INTERNAL_MASK_COUNT],
}

struct Renderer {
    cell_width: usize,
    cell_height: usize,
    internal_masks: [Vec<SamplePoint>; INTERNAL_MASK_COUNT],
    external_masks: [Vec<SamplePoint>; EXTERNAL_MASK_COUNT],
    glyphs: Vec<Glyph>,
    cache: HashMap<u32, usize>,
    fg_sgr: Vec<Vec<u8>>,
    gray_lut: [u8; 256],
    last_output: Vec<u8>,
    last_sgr_change_count: u32,
    last_cache_hits: u32,
    last_cache_misses: u32,
    last_sample_count: u32,
    last_lookup_count: u32,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct NativeHarriRenderStats {
    pub sgr_change_count: u32,
    pub cache_hits: u32,
    pub cache_misses: u32,
    pub sample_count: u32,
    pub lookup_count: u32,
    pub output_bytes: u32,
}

pub struct NativeHarriRenderer {
    renderer: Renderer,
}

impl Renderer {
    fn new(cell_width: usize, cell_height: usize) -> Self {
        let internal_masks = std::array::from_fn(|index| {
            let (cx, cy, radius) = INTERNAL_CIRCLES[index];
            build_mask(cx, cy, radius, cell_width, cell_height)
        });
        let external_masks = std::array::from_fn(|index| {
            let (cx, cy, radius) = EXTERNAL_CIRCLES[index];
            build_mask(cx, cy, radius, cell_width, cell_height)
        });
        Self {
            cell_width,
            cell_height,
            internal_masks,
            external_masks,
            glyphs: Vec::new(),
            cache: HashMap::new(),
            fg_sgr: build_fg_sgr(),
            gray_lut: build_gray_lut(),
            last_output: Vec::new(),
            last_sgr_change_count: 0,
            last_cache_hits: 0,
            last_cache_misses: 0,
            last_sample_count: 0,
            last_lookup_count: 0,
        }
    }

    fn set_glyphs(
        &mut self,
        glyph_chars: &[u8],
        glyph_vectors_bytes: &[u8],
        vector_size: usize,
    ) -> Result<(), String> {
        if vector_size != INTERNAL_MASK_COUNT {
            return Err(format!(
                "expected {INTERNAL_MASK_COUNT} glyph-vector components, received {vector_size}"
            ));
        }
        if glyph_chars.is_empty() {
            return Err("Harri glyph database is empty".into());
        }
        let expected_bytes = glyph_chars
            .len()
            .checked_mul(vector_size)
            .and_then(|value| value.checked_mul(std::mem::size_of::<f32>()))
            .ok_or_else(|| "glyph database size overflowed".to_string())?;
        if glyph_vectors_bytes.len() != expected_bytes {
            return Err(format!(
                "expected {expected_bytes} glyph-vector bytes, received {}",
                glyph_vectors_bytes.len()
            ));
        }

        self.glyphs.clear();
        self.glyphs.reserve(glyph_chars.len());
        self.cache.clear();

        for (glyph_index, &glyph_char) in glyph_chars.iter().enumerate() {
            let mut vector = [0.0; INTERNAL_MASK_COUNT];
            let glyph_offset = glyph_index * vector_size * std::mem::size_of::<f32>();
            for (component_index, component) in vector.iter_mut().enumerate() {
                let offset = glyph_offset + component_index * std::mem::size_of::<f32>();
                *component = f32::from_le_bytes(
                    glyph_vectors_bytes[offset..offset + std::mem::size_of::<f32>()]
                        .try_into()
                        .map_err(|_| "glyph-vector payload had invalid length".to_string())?,
                );
            }
            self.glyphs.push(Glyph {
                ch: glyph_char,
                vector,
            });
        }

        Ok(())
    }

    fn render(
        &mut self,
        pixels: &[u8],
        width: usize,
        height: usize,
        columns: usize,
        rows: usize,
    ) -> Result<(), String> {
        self.validate_render_dimensions(width, height, columns, rows)?;
        let expected_pixels = width
            .checked_mul(height)
            .and_then(|value| value.checked_mul(3))
            .ok_or_else(|| "pixel buffer length overflowed".to_string())?;
        if pixels.len() != expected_pixels {
            return Err(format!(
                "expected {expected_pixels} RGB bytes, received {}",
                pixels.len()
            ));
        }
        if self.glyphs.is_empty() {
            return Err("Harri glyph database not initialized".into());
        }

        let luminance_plane = build_luminance_plane(pixels, width, height);
        self.render_luminance_plane(&luminance_plane, width, height, columns, rows)
    }

    fn render_luma(
        &mut self,
        pixels: &[u8],
        width: usize,
        height: usize,
        columns: usize,
        rows: usize,
    ) -> Result<(), String> {
        self.validate_render_dimensions(width, height, columns, rows)?;
        let expected_pixels = width
            .checked_mul(height)
            .ok_or_else(|| "pixel buffer length overflowed".to_string())?;
        if pixels.len() != expected_pixels {
            return Err(format!(
                "expected {expected_pixels} grayscale bytes, received {}",
                pixels.len()
            ));
        }
        if self.glyphs.is_empty() {
            return Err("Harri glyph database not initialized".into());
        }

        let luminance_plane = build_luminance_plane_from_luma(pixels, width, height);
        self.render_luminance_plane(&luminance_plane, width, height, columns, rows)
    }

    fn validate_render_dimensions(
        &self,
        width: usize,
        height: usize,
        columns: usize,
        rows: usize,
    ) -> Result<(), String> {
        let expected_width = columns
            .checked_mul(self.cell_width)
            .ok_or_else(|| "render width overflowed".to_string())?;
        let expected_height = rows
            .checked_mul(self.cell_height)
            .ok_or_else(|| "render height overflowed".to_string())?;
        if width != expected_width || height != expected_height {
            return Err(format!(
                "ts-harri expected raster {expected_width}x{expected_height}, received {width}x{height}"
            ));
        }
        Ok(())
    }

    fn render_luminance_plane(
        &mut self,
        luminance_plane: &[f32],
        width: usize,
        height: usize,
        columns: usize,
        rows: usize,
    ) -> Result<(), String> {
        let mut output = Vec::with_capacity(
            columns
                .checked_mul(rows)
                .and_then(|value| value.checked_mul(8))
                .unwrap_or(0),
        );
        let mut cache_hits = 0u32;
        let mut cache_misses = 0u32;
        let mut sgr_change_count = 0u32;

        for row in 0..rows {
            let origin_y = (row * self.cell_height) as i32;
            let mut previous_fg_ansi = u8::MAX;
            for column in 0..columns {
                let origin_x = (column * self.cell_width) as i32;
                let mut internal_vector = [0.0; INTERNAL_MASK_COUNT];
                for (index, mask) in self.internal_masks.iter().enumerate() {
                    internal_vector[index] =
                        sample_plane_mask(luminance_plane, width, height, origin_x, origin_y, mask);
                }
                let mut external_vector = [0.0; EXTERNAL_MASK_COUNT];
                for (index, mask) in self.external_masks.iter().enumerate() {
                    external_vector[index] =
                        sample_plane_mask(luminance_plane, width, height, origin_x, origin_y, mask);
                }

                let average_luminance =
                    internal_vector.iter().copied().sum::<f32>() / INTERNAL_MASK_COUNT as f32;
                let contrasted = apply_global_contrast(apply_directional_contrast(
                    internal_vector,
                    external_vector,
                ));
                let cache_key = quantize_vector(&contrasted);
                let glyph_index = if let Some(index) = self.cache.get(&cache_key).copied() {
                    cache_hits = cache_hits.saturating_add(1);
                    index
                } else {
                    cache_misses = cache_misses.saturating_add(1);
                    let index = find_best_glyph(&contrasted, &self.glyphs);
                    self.cache.insert(cache_key, index);
                    index
                };

                let luminance_byte = (average_luminance.clamp(0.0, 1.0) * 255.0).round() as u8;
                let fg_ansi = self.gray_lut[luminance_byte as usize];
                if fg_ansi != previous_fg_ansi {
                    output.extend_from_slice(&self.fg_sgr[fg_ansi as usize]);
                    previous_fg_ansi = fg_ansi;
                    sgr_change_count = sgr_change_count.saturating_add(1);
                }
                output.push(self.glyphs[glyph_index].ch);
            }
            output.extend_from_slice(RESET_BYTES);
            if row + 1 < rows {
                output.push(b'\n');
            }
        }

        self.last_output = output;
        self.last_sgr_change_count = sgr_change_count;
        self.last_cache_hits = cache_hits;
        self.last_cache_misses = cache_misses;
        self.last_sample_count = saturating_u32(
            columns
                .saturating_mul(rows)
                .saturating_mul(INTERNAL_MASK_COUNT + EXTERNAL_MASK_COUNT),
        );
        self.last_lookup_count = saturating_u32(columns.saturating_mul(rows));
        Ok(())
    }
}

impl NativeHarriRenderer {
    pub fn new(cell_width: usize, cell_height: usize) -> Result<Self, String> {
        if cell_width == 0 || cell_height == 0 {
            return Err("Harri renderer cell dimensions must be non-zero".into());
        }
        Ok(Self {
            renderer: Renderer::new(cell_width, cell_height),
        })
    }

    pub fn set_glyphs(
        &mut self,
        glyph_chars: &[u8],
        glyph_vectors_bytes: &[u8],
        vector_size: usize,
    ) -> Result<(), String> {
        self.renderer
            .set_glyphs(glyph_chars, glyph_vectors_bytes, vector_size)
    }

    pub fn render_luma(
        &mut self,
        pixels: &[u8],
        width: usize,
        height: usize,
        columns: usize,
        rows: usize,
    ) -> Result<(), String> {
        self.renderer
            .render_luma(pixels, width, height, columns, rows)
    }

    pub fn output_text(&self) -> String {
        String::from_utf8_lossy(&self.renderer.last_output).into_owned()
    }

    pub fn stats(&self) -> NativeHarriRenderStats {
        NativeHarriRenderStats {
            sgr_change_count: self.renderer.last_sgr_change_count,
            cache_hits: self.renderer.last_cache_hits,
            cache_misses: self.renderer.last_cache_misses,
            sample_count: self.renderer.last_sample_count,
            lookup_count: self.renderer.last_lookup_count,
            output_bytes: saturating_u32(self.renderer.last_output.len()),
        }
    }
}

fn build_mask(
    cx: f32,
    cy: f32,
    radius: f32,
    cell_width: usize,
    cell_height: usize,
) -> Vec<SamplePoint> {
    let center_x = cx * cell_width as f32;
    let center_y = cy * cell_height as f32;
    let radius = radius * cell_width as f32;
    let radius_squared = radius * radius;
    let min_x = (center_x - radius).floor() as i32;
    let max_x = (center_x + radius).ceil() as i32;
    let min_y = (center_y - radius).floor() as i32;
    let max_y = (center_y + radius).ceil() as i32;
    let mut points = Vec::new();

    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let dx = x as f32 + 0.5 - center_x;
            let dy = y as f32 + 0.5 - center_y;
            if dx * dx + dy * dy <= radius_squared {
                points.push(SamplePoint { dx: x, dy: y });
            }
        }
    }

    points
}

fn build_luminance_plane(pixels: &[u8], width: usize, height: usize) -> Vec<f32> {
    let mut plane = Vec::with_capacity(width.saturating_mul(height));
    for chunk in pixels.chunks_exact(3) {
        let r = chunk[0] as f32;
        let g = chunk[1] as f32;
        let b = chunk[2] as f32;
        plane.push((0.2126 * r + 0.7152 * g + 0.0722 * b) / 255.0);
    }
    plane
}

fn build_luminance_plane_from_luma(pixels: &[u8], width: usize, height: usize) -> Vec<f32> {
    let mut plane = Vec::with_capacity(width.saturating_mul(height));
    for &value in pixels {
        plane.push(value as f32 / 255.0);
    }
    plane
}

fn sample_plane_mask(
    plane: &[f32],
    width: usize,
    height: usize,
    origin_x: i32,
    origin_y: i32,
    mask: &[SamplePoint],
) -> f32 {
    let mut sum = 0.0f32;
    let mut count = 0usize;
    for point in mask {
        let x = origin_x + point.dx;
        let y = origin_y + point.dy;
        if x < 0 || y < 0 {
            continue;
        }
        let x = x as usize;
        let y = y as usize;
        if x >= width || y >= height {
            continue;
        }
        sum += plane[y * width + x];
        count += 1;
    }
    if count == 0 {
        0.0
    } else {
        sum / count as f32
    }
}

fn apply_directional_contrast(
    internal_vector: [f32; INTERNAL_MASK_COUNT],
    external_vector: [f32; EXTERNAL_MASK_COUNT],
) -> [f32; INTERNAL_MASK_COUNT] {
    let mut result = [0.0; INTERNAL_MASK_COUNT];
    for index in 0..INTERNAL_MASK_COUNT {
        let mut max_value = internal_vector[index];
        for &external_index in AFFECTING_EXTERNAL_INDICES[index] {
            max_value = max_value.max(external_vector[external_index]);
        }
        result[index] = if max_value <= 0.0 {
            0.0
        } else {
            (internal_vector[index] / max_value).powf(DIRECTIONAL_CONTRAST_EXPONENT) * max_value
        };
    }
    result
}

fn apply_global_contrast(vector: [f32; INTERNAL_MASK_COUNT]) -> [f32; INTERNAL_MASK_COUNT] {
    let max_value = vector.iter().copied().fold(0.0, f32::max);
    if max_value <= 0.0 {
        return [0.0; INTERNAL_MASK_COUNT];
    }

    let mut result = [0.0; INTERNAL_MASK_COUNT];
    for (index, value) in vector.iter().copied().enumerate() {
        result[index] = (value / max_value).powf(GLOBAL_CONTRAST_EXPONENT) * max_value;
    }
    result
}

fn quantize_vector(vector: &[f32; INTERNAL_MASK_COUNT]) -> u32 {
    let mut key = 0u32;
    for &value in vector {
        let quantized = (value * (LOOKUP_RANGE - 1) as f32).round();
        let quantized = quantized.clamp(0.0, (LOOKUP_RANGE - 1) as f32) as u32;
        key = key * LOOKUP_RANGE + quantized;
    }
    key
}

fn find_best_glyph(vector: &[f32; INTERNAL_MASK_COUNT], glyphs: &[Glyph]) -> usize {
    let mut best_index = 0usize;
    let mut best_distance = f32::INFINITY;
    for (index, glyph) in glyphs.iter().enumerate() {
        let mut distance = 0.0f32;
        for (component, value) in vector.iter().enumerate().take(INTERNAL_MASK_COUNT) {
            let delta = *value - glyph.vector[component];
            distance += delta * delta;
        }
        if distance < best_distance {
            best_distance = distance;
            best_index = index;
        }
    }
    best_index
}

fn build_palette() -> Vec<[u8; 3]> {
    let mut palette = Vec::with_capacity(256);
    let system_colors = [
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
    palette.extend(system_colors);

    let cube_steps = [0, 95, 135, 175, 215, 255];
    for &r in &cube_steps {
        for &g in &cube_steps {
            for &b in &cube_steps {
                palette.push([r, g, b]);
            }
        }
    }

    for value in 0..24u8 {
        let gray = 8u8.saturating_add(value.saturating_mul(10));
        palette.push([gray, gray, gray]);
    }

    palette
}

fn build_gray_lut() -> [u8; 256] {
    let palette = build_palette();
    let mut lut = [16u8; 256];
    for gray in 0..=255u16 {
        let gray = gray as i32;
        let mut best_distance = i32::MAX;
        let mut best_index = 16u8;
        for (index, color) in palette.iter().enumerate().skip(16) {
            let dr = gray - color[0] as i32;
            let dg = gray - color[1] as i32;
            let db = gray - color[2] as i32;
            let distance = dr * dr + dg * dg + db * db;
            if distance < best_distance {
                best_distance = distance;
                best_index = index as u8;
                if distance == 0 {
                    break;
                }
            }
        }
        lut[gray as usize] = best_index;
    }
    lut
}

fn build_fg_sgr() -> Vec<Vec<u8>> {
    (0..256)
        .map(|index| format!("\x1b[38;5;{index}m").into_bytes())
        .collect()
}

fn saturating_u32(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

fn set_last_error(message: impl Into<String>) {
    LAST_ERROR.with(|slot| {
        *slot.borrow_mut() = message.into().into_bytes();
    });
}

fn clear_last_error() {
    LAST_ERROR.with(|slot| slot.borrow_mut().clear());
}

fn with_renderer_mut<T>(
    handle: u32,
    callback: impl FnOnce(&mut Renderer) -> Result<T, String>,
) -> Result<T, String> {
    RENDERERS.with(|renderers| {
        let mut renderers = renderers.borrow_mut();
        let renderer = renderers
            .get_mut(&handle)
            .ok_or_else(|| format!("Unknown Harri renderer handle {handle}"))?;
        callback(renderer)
    })
}

fn with_renderer<T>(handle: u32, callback: impl FnOnce(&Renderer) -> T) -> Result<T, String> {
    RENDERERS.with(|renderers| {
        let renderers = renderers.borrow();
        let renderer = renderers
            .get(&handle)
            .ok_or_else(|| format!("Unknown Harri renderer handle {handle}"))?;
        Ok(callback(renderer))
    })
}

fn next_handle() -> u32 {
    NEXT_HANDLE.with(|slot| {
        let mut slot = slot.borrow_mut();
        let handle = *slot;
        *slot = if *slot == u32::MAX { 1 } else { *slot + 1 };
        handle
    })
}

#[no_mangle]
pub extern "C" fn alloc(len: u32) -> *mut u8 {
    if len == 0 {
        return std::ptr::null_mut();
    }
    let mut buffer = Vec::<u8>::with_capacity(len as usize);
    let ptr = buffer.as_mut_ptr();
    std::mem::forget(buffer);
    ptr
}

/// # Safety
///
/// `ptr`, `len`, and `cap` must describe a buffer previously returned by
/// `alloc()` and not yet released.
#[no_mangle]
pub unsafe extern "C" fn dealloc(ptr: *mut u8, len: u32, cap: u32) {
    if ptr.is_null() || cap == 0 {
        return;
    }
    let _ = Vec::from_raw_parts(ptr, len as usize, cap as usize);
}

#[no_mangle]
pub extern "C" fn renderer_create(cell_width: u32, cell_height: u32) -> u32 {
    let cell_width = cell_width as usize;
    let cell_height = cell_height as usize;
    if cell_width == 0 || cell_height == 0 {
        set_last_error("Harri renderer cell dimensions must be non-zero");
        return 0;
    }

    let handle = next_handle();
    RENDERERS.with(|renderers| {
        renderers
            .borrow_mut()
            .insert(handle, Renderer::new(cell_width, cell_height));
    });
    clear_last_error();
    handle
}

#[no_mangle]
pub extern "C" fn renderer_destroy(handle: u32) {
    RENDERERS.with(|renderers| {
        renderers.borrow_mut().remove(&handle);
    });
}

/// # Safety
///
/// When the lengths are non-zero, `glyph_chars_ptr` and `glyph_vectors_ptr`
/// must point to readable buffers of `glyph_chars_len` and `glyph_vectors_len`
/// bytes respectively.
#[no_mangle]
pub unsafe extern "C" fn renderer_set_glyphs(
    handle: u32,
    glyph_chars_ptr: *const u8,
    glyph_chars_len: u32,
    glyph_vectors_ptr: *const u8,
    glyph_vectors_len: u32,
    vector_size: u32,
) -> u32 {
    let glyph_chars = if glyph_chars_len == 0 {
        &[][..]
    } else {
        slice::from_raw_parts(glyph_chars_ptr, glyph_chars_len as usize)
    };
    let glyph_vectors = if glyph_vectors_len == 0 {
        &[][..]
    } else {
        slice::from_raw_parts(glyph_vectors_ptr, glyph_vectors_len as usize)
    };
    match with_renderer_mut(handle, |renderer| {
        renderer.set_glyphs(glyph_chars, glyph_vectors, vector_size as usize)
    }) {
        Ok(()) => {
            clear_last_error();
            1
        }
        Err(error) => {
            set_last_error(error);
            0
        }
    }
}

/// # Safety
///
/// When `pixels_len` is non-zero, `pixels_ptr` must point to a readable RGB
/// buffer of exactly `pixels_len` bytes.
#[no_mangle]
pub unsafe extern "C" fn renderer_render(
    handle: u32,
    pixels_ptr: *const u8,
    pixels_len: u32,
    width: u32,
    height: u32,
    columns: u32,
    rows: u32,
) -> u32 {
    let pixels = if pixels_len == 0 {
        &[][..]
    } else {
        slice::from_raw_parts(pixels_ptr, pixels_len as usize)
    };
    match with_renderer_mut(handle, |renderer| {
        renderer.render(
            pixels,
            width as usize,
            height as usize,
            columns as usize,
            rows as usize,
        )
    }) {
        Ok(()) => {
            clear_last_error();
            1
        }
        Err(error) => {
            set_last_error(error);
            0
        }
    }
}

#[no_mangle]
pub extern "C" fn renderer_output_ptr(handle: u32) -> u32 {
    match with_renderer(handle, |renderer| renderer.last_output.as_ptr() as usize) {
        Ok(ptr) => ptr as u32,
        Err(error) => {
            set_last_error(error);
            0
        }
    }
}

#[no_mangle]
pub extern "C" fn renderer_output_len(handle: u32) -> u32 {
    match with_renderer(handle, |renderer| renderer.last_output.len()) {
        Ok(len) => saturating_u32(len),
        Err(error) => {
            set_last_error(error);
            0
        }
    }
}

#[no_mangle]
pub extern "C" fn renderer_sgr_change_count(handle: u32) -> u32 {
    match with_renderer(handle, |renderer| renderer.last_sgr_change_count) {
        Ok(value) => value,
        Err(error) => {
            set_last_error(error);
            0
        }
    }
}

#[no_mangle]
pub extern "C" fn renderer_cache_hits(handle: u32) -> u32 {
    match with_renderer(handle, |renderer| renderer.last_cache_hits) {
        Ok(value) => value,
        Err(error) => {
            set_last_error(error);
            0
        }
    }
}

#[no_mangle]
pub extern "C" fn renderer_cache_misses(handle: u32) -> u32 {
    match with_renderer(handle, |renderer| renderer.last_cache_misses) {
        Ok(value) => value,
        Err(error) => {
            set_last_error(error);
            0
        }
    }
}

#[no_mangle]
pub extern "C" fn renderer_sample_count(handle: u32) -> u32 {
    match with_renderer(handle, |renderer| renderer.last_sample_count) {
        Ok(value) => value,
        Err(error) => {
            set_last_error(error);
            0
        }
    }
}

#[no_mangle]
pub extern "C" fn renderer_lookup_count(handle: u32) -> u32 {
    match with_renderer(handle, |renderer| renderer.last_lookup_count) {
        Ok(value) => value,
        Err(error) => {
            set_last_error(error);
            0
        }
    }
}

#[no_mangle]
pub extern "C" fn last_error_ptr() -> u32 {
    LAST_ERROR.with(|slot| slot.borrow().as_ptr() as usize as u32)
}

#[no_mangle]
pub extern "C" fn last_error_len() -> u32 {
    LAST_ERROR.with(|slot| saturating_u32(slot.borrow().len()))
}
