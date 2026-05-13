use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

// H.264 NVENC's documented per-codec minimum is ~145x49 on Turing+.
// AV1 NVENC on Ada+ is 160x64. After 16-byte alignment the smallest
// width that works on all three (H.264 / HEVC / AV1) NVENC paths is
// 160; the smallest height is 64. We pick the higher one (160) for
// both axes so the floor is a single number that's safe regardless of
// which axis the UI happens to shrink first. Browsers also tend to
// render tiny <video> targets with poor scaling, so this doubles as a
// UX floor.
const MIN_PREVIEW_DIMENSION: u32 = 160;
// Cap at the camera's native 1920 width: anything larger would force
// the encoder to upscale (pure overhead) and push libav's auto-picked
// H.264 level past 4.2, which is the highest level most browsers'
// WebCodecs implementations decode reliably across GPU + software
// paths. Capping here also stops the UI's wild `set_preview_size`
// requests from driving the encoder into Level 6.0 territory.
const MAX_PREVIEW_DIMENSION: u32 = 1920;
// NVENC's surface allocator and most HW encoders require width/height
// aligned to a small power of two. 16 is the largest constraint we've
// seen across NVENC / VAAPI / libsvtav1; using it everywhere keeps the
// open path predictable regardless of which backend `auto` resolves to.
const PREVIEW_DIMENSION_ALIGNMENT: u32 = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PreviewSize {
    pub width: u32,
    pub height: u32,
}

/// Result of `set_requested_size`. `changed = false` means the
/// post-clamp dims matched what was already stored, so callers can
/// skip the downstream `PreviewControl::SetSize` forward.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SizeUpdate {
    pub size: PreviewSize,
    pub changed: bool,
}

impl PreviewSize {
    pub fn new(width: u32, height: u32) -> Self {
        // When either axis exceeds the cap, scale BOTH by the same
        // factor so the UI's requested aspect ratio survives — without
        // this, a (4096, 2694) request would land at (1920, 1920),
        // a square preview totally unrelated to what the UI asked for.
        let max_dim = width.max(height);
        let (scaled_w, scaled_h) = if max_dim > MAX_PREVIEW_DIMENSION {
            let scale = MAX_PREVIEW_DIMENSION as f64 / max_dim as f64;
            (
                (width as f64 * scale).round() as u32,
                (height as f64 * scale).round() as u32,
            )
        } else {
            (width, height)
        };
        Self {
            width: clamp_preview_dimension(scaled_w),
            height: clamp_preview_dimension(scaled_h),
        }
    }
}

#[derive(Debug)]
pub struct RuntimePreviewConfig {
    default_size: PreviewSize,
    active_width: AtomicU32,
    active_height: AtomicU32,
    connected_clients: AtomicUsize,
}

impl RuntimePreviewConfig {
    pub fn new(default_width: u32, default_height: u32) -> Self {
        let default_size = PreviewSize::new(default_width, default_height);
        Self {
            default_size,
            active_width: AtomicU32::new(default_size.width),
            active_height: AtomicU32::new(default_size.height),
            connected_clients: AtomicUsize::new(0),
        }
    }

    #[cfg(test)]
    pub fn current_size(&self) -> PreviewSize {
        PreviewSize {
            width: self.active_width.load(Ordering::Relaxed),
            height: self.active_height.load(Ordering::Relaxed),
        }
    }

    /// Update the active preview dims and report whether the new value
    /// differs from what we had stored. Callers use the `changed` bit
    /// to suppress redundant `PreviewControl::SetSize` forwards — the
    /// UI fires `set_preview_size` on every layout/ResizeObserver tick
    /// (many times per second), and most of those land on the same
    /// 16-aligned bucket. Skipping the unchanged ones avoids tearing
    /// down and reopening the encoder's codec session repeatedly,
    /// which would otherwise starve the stream of stable packets.
    pub fn set_requested_size(&self, width: u32, height: u32) -> SizeUpdate {
        let next = PreviewSize::new(width, height);
        let prev_width = self.active_width.swap(next.width, Ordering::AcqRel);
        let prev_height = self.active_height.swap(next.height, Ordering::AcqRel);
        SizeUpdate {
            size: next,
            changed: prev_width != next.width || prev_height != next.height,
        }
    }

    pub fn reset_to_default(&self) -> PreviewSize {
        self.set_requested_size(self.default_size.width, self.default_size.height)
            .size
    }

    pub fn client_connected(&self) {
        self.connected_clients.fetch_add(1, Ordering::Relaxed);
    }

    pub fn client_disconnected(&self) -> Option<PreviewSize> {
        let mut current = self.connected_clients.load(Ordering::Relaxed);
        loop {
            if current == 0 {
                return None;
            }
            match self.connected_clients.compare_exchange(
                current,
                current - 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    if current == 1 {
                        return Some(self.reset_to_default());
                    }
                    return None;
                }
                Err(observed) => current = observed,
            }
        }
    }
}

fn clamp_preview_dimension(value: u32) -> u32 {
    let clamped = value.clamp(MIN_PREVIEW_DIMENSION, MAX_PREVIEW_DIMENSION);
    // Round down to the alignment so we never exceed the requested
    // dimension, then re-floor at MIN_PREVIEW_DIMENSION in case the
    // request itself was the minimum (and is already aligned).
    let aligned = clamped - (clamped % PREVIEW_DIMENSION_ALIGNMENT);
    aligned.max(MIN_PREVIEW_DIMENSION)
}

#[cfg(test)]
mod tests {
    use super::{PreviewSize, RuntimePreviewConfig};

    #[test]
    fn preview_size_clamps_invalid_values() {
        // 0 → MIN (160). 10_000 → MAX (1920). Both axis caps are
        // 16-aligned already, so no alignment fixup is needed here.
        assert_eq!(
            PreviewSize::new(0, 10_000),
            PreviewSize {
                width: 160,
                height: 1920,
            }
        );
    }

    #[test]
    fn preview_size_clamps_tiny_ui_request_to_nvenc_safe_floor() {
        // Browsers occasionally send DOM CSS pixels before layout
        // settles, producing (1, 387)-style requests. Without the
        // alignment floor the visualizer would forward that to the
        // preview encoder, which would crash NVENC's open with
        // "Frame Dimension less than the minimum supported value".
        // H.264 NVENC's actual minimum width is ~145 on Turing+, so
        // the floor lives at 160 (16-aligned, above NVENC's limit).
        let clamped = PreviewSize::new(1, 387);
        assert_eq!(clamped.width, 160);
        assert_eq!(clamped.height, 384);
    }

    #[test]
    fn set_requested_size_reports_changed_bit() {
        let config = RuntimePreviewConfig::new(320, 240);
        // Same clamped dims as the constructor → no change.
        let first = config.set_requested_size(320, 240);
        assert!(!first.changed);
        // Different post-clamp dims → changed.
        let second = config.set_requested_size(1280, 720);
        assert!(second.changed);
        // Two UI requests that land on the same 16-aligned bucket
        // (1280 and 1281 both clamp to 1280) → second is unchanged so
        // the WS handler can skip the downstream forward.
        let third = config.set_requested_size(1281, 720);
        assert!(!third.changed);
    }

    #[test]
    fn preview_size_preserves_aspect_when_downscaling_to_max() {
        // The UI fires `set_preview_size(4096, 2694)` on a 4K-ish
        // canvas. Without aspect-preserving scaling, both axes would
        // hit the 1920 cap and the encoder would produce a square
        // 1920x1920 stream — visually wrong and a waste of pixels.
        // With aspect preservation, the longer axis (4096) lands at
        // 1920 and the shorter follows proportionally.
        let clamped = PreviewSize::new(4096, 2694);
        assert_eq!(clamped.width, 1920);
        // 2694 * (1920/4096) ≈ 1263 → align down to 1248 (1248/16=78).
        assert_eq!(clamped.height, 1248);
        // The two axes are now in roughly the same 4096:2694 ratio
        // (1.52) as 1920:1248 (1.54), within rounding+alignment slop.
    }

    #[test]
    fn preview_size_aligns_down_to_avoid_exceeding_request() {
        // A 1674 CSS-pixel request must not become 1680 (rounding up
        // would overshoot the visible viewport). Round down to 1664.
        let clamped = PreviewSize::new(1674, 1047);
        assert_eq!(clamped.width, 1664);
        assert_eq!(clamped.height, 1040);
    }

    #[test]
    fn reset_only_happens_when_last_client_disconnects() {
        let config = RuntimePreviewConfig::new(320, 240);
        config.client_connected();
        config.client_connected();
        config.set_requested_size(800, 600);

        assert_eq!(config.client_disconnected(), None);
        assert_eq!(config.current_size(), PreviewSize::new(800, 600));
        assert_eq!(
            config.client_disconnected(),
            Some(PreviewSize::new(320, 240))
        );
        assert_eq!(config.current_size(), PreviewSize::new(320, 240));
    }
}
