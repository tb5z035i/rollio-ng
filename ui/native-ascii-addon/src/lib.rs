use std::time::Instant;

use napi::bindgen_prelude::{Buffer, Error, Result};
use napi_derive::napi;
use rollio_harri_wasm_core::NativeHarriRenderer;

fn to_napi_error(error: impl std::fmt::Display) -> Error {
    Error::from_reason(error.to_string())
}

#[napi(object)]
pub struct NativeAsciiRenderStats {
    pub total_ms: f64,
    pub sample_count: u32,
    pub lookup_count: u32,
    pub cache_hits: u32,
    pub cache_misses: u32,
    pub cell_count: u32,
    pub output_bytes: u32,
    pub sgr_change_count: Option<u32>,
    pub assemble_ms: Option<f64>,
}

#[napi(object)]
pub struct NativeAsciiRenderResult {
    pub lines: Vec<String>,
    pub stats: NativeAsciiRenderStats,
}

#[napi]
pub struct NativeAsciiRenderer {
    renderer: NativeHarriRenderer,
}

#[napi]
impl NativeAsciiRenderer {
    #[napi(constructor)]
    pub fn new(
        cell_width: u32,
        cell_height: u32,
        glyph_chars: Buffer,
        glyph_vectors: Buffer,
        vector_size: u32,
    ) -> Result<Self> {
        let mut renderer =
            NativeHarriRenderer::new(cell_width as usize, cell_height as usize)
            .map_err(to_napi_error)?;
        renderer
            .set_glyphs(glyph_chars.as_ref(), glyph_vectors.as_ref(), vector_size as usize)
            .map_err(to_napi_error)?;
        Ok(Self { renderer })
    }

    #[napi]
    pub fn render(
        &mut self,
        pixels: Buffer,
        width: u32,
        height: u32,
        columns: u32,
        rows: u32,
    ) -> Result<NativeAsciiRenderResult> {
        let started_at = Instant::now();
        self.renderer
            .render_luma(
                pixels.as_ref(),
                width as usize,
                height as usize,
                columns as usize,
                rows as usize,
            )
            .map_err(to_napi_error)?;
        let stats = self.renderer.stats();
        let output_text = self.renderer.output_text();

        Ok(NativeAsciiRenderResult {
            stats: NativeAsciiRenderStats {
                total_ms: started_at.elapsed().as_secs_f64() * 1_000.0,
                sample_count: stats.sample_count,
                lookup_count: stats.lookup_count,
                cache_hits: stats.cache_hits,
                cache_misses: stats.cache_misses,
                cell_count: columns.saturating_mul(rows),
                output_bytes: stats.output_bytes,
                sgr_change_count: Some(stats.sgr_change_count),
                assemble_ms: None,
            },
            lines: if output_text.is_empty() {
                Vec::new()
            } else {
                output_text.split('\n').map(str::to_owned).collect()
            },
        })
    }
}
