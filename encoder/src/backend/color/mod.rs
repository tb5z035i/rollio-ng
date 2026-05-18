//! Color-channel encoder backends.
//!
//! A `ColorEncoderBackend` knows how to turn raw camera frames
//! (`Rgb24` / `Bgr24` / `Yuyv` / `Mjpeg` / `Gray8`) — and, in a future
//! phase, pre-encoded `H264AnnexB` — into encoded packets via one of
//! the supported color codecs (H.264 / H.265 / AV1 / MJPG).
//!
//! Backends are registered at process start in
//! [`ColorBackendRegistry::default_set`]. Resolution at session-open
//! time is driven by the user's `[encoder.preview] backend` (or
//! `[encoder] backend` for the recording role):
//!
//! - `EncoderBackend::Auto` walks the registry in priority order
//!   (Nvidia > Vaapi > Cpu) and picks the first that reports
//!   `available() && supports(codec, input)`.
//! - An explicit backend name routes directly to the matching impl and
//!   errors if it's not present or doesn't support the requested combo
//!   — fail loudly so config typos surface immediately instead of
//!   silently falling back to the wrong path.
//!
//! Phase 1 (this commit) hosts three thin backend wrappers around the
//! existing `LibavCodecSession`. Phases 3 and 4 swap in real
//! hardware-accelerated pipelines (NVDEC + scale_cuda + NVENC for
//! NVIDIA; corresponding VAAPI filter graph for Intel/AMD) inside the
//! same trait surface.

pub mod libav_cpu;
pub mod libav_nvidia;
pub mod libav_vaapi;
pub mod passthrough;

use std::sync::{Arc, OnceLock};

use rollio_types::config::{EncoderBackend, EncoderCodec};
use rollio_types::messages::PixelFormat;

use crate::codec::{CodecSession, CodecSessionParams, OwnedFrame};
use crate::error::{EncoderError, Result};

/// Stable identifier for each registered color backend. Maps onto
/// `EncoderBackend` from the project config; only color-eligible
/// variants appear here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ColorBackendId {
    Cpu,
    Nvidia,
    Vaapi,
    Passthrough,
    HorizonX5,
}

impl ColorBackendId {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cpu => "cpu",
            Self::Nvidia => "nvidia",
            Self::Vaapi => "vaapi",
            Self::Passthrough => "passthrough",
            Self::HorizonX5 => "horizon-x5",
        }
    }
}

/// Runtime-only refinement of `EncoderCodec` that excludes depth
/// codecs. Color backends never see `EncoderCodec::Rvl` because the
/// depth registry handles depth dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorCodec {
    H264,
    H265,
    Av1,
    Mjpg,
}

impl TryFrom<EncoderCodec> for ColorCodec {
    type Error = EncoderError;

    fn try_from(value: EncoderCodec) -> Result<Self> {
        match value {
            EncoderCodec::H264 => Ok(Self::H264),
            EncoderCodec::H265 => Ok(Self::H265),
            EncoderCodec::Av1 => Ok(Self::Av1),
            EncoderCodec::Mjpg => Ok(Self::Mjpg),
            EncoderCodec::Rvl => Err(EncoderError::message(
                "RVL is a depth codec; not routable to a color backend",
            )),
        }
    }
}

impl From<ColorCodec> for EncoderCodec {
    fn from(value: ColorCodec) -> Self {
        match value {
            ColorCodec::H264 => Self::H264,
            ColorCodec::H265 => Self::H265,
            ColorCodec::Av1 => Self::Av1,
            ColorCodec::Mjpg => Self::Mjpg,
        }
    }
}

/// Implementation contract for one color-side encoder backend.
///
/// Implementations are stateless singletons held in an `Arc` inside
/// the registry. The per-session state (encoder context, scaler,
/// MJPEG decoder, hardware-frames context, …) lives entirely inside
/// the `Box<dyn CodecSession>` returned by [`open_session`].
pub trait ColorEncoderBackend: Send + Sync {
    /// Stable identifier for logging and `EncoderBackend` mapping.
    fn id(&self) -> ColorBackendId;

    /// Higher value = tried first under `EncoderBackend::Auto`.
    /// Convention: 100 = NVIDIA, 50 = VAAPI, 10 = CPU. Future Horizon
    /// X5 picks its own number to slot wherever appropriate for the
    /// target deployment.
    fn priority(&self) -> u32;

    /// Cheap runtime probe — does this host actually have the
    /// hardware/libraries to run this backend? Called once per Auto
    /// resolution; should not perform expensive ffmpeg lookups.
    fn available(&self) -> bool;

    /// Whether this backend can encode `codec` from `input` frames.
    fn supports(&self, codec: ColorCodec, input: PixelFormat) -> bool;

    /// Construct a session that will accept frames matching
    /// `first_frame.header.pixel_format` (any subsequent format change
    /// is rejected by the session's frame-compatibility check).
    fn open_session(
        &self,
        params: &CodecSessionParams<'_>,
        first_frame: &OwnedFrame,
    ) -> Result<Box<dyn CodecSession>>;
}

/// Holder of every registered color backend. Looked up via
/// [`ColorBackendRegistry::global`] (or constructed standalone in
/// tests).
pub struct ColorBackendRegistry {
    backends: Vec<Arc<dyn ColorEncoderBackend>>,
}

impl std::fmt::Debug for ColorBackendRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let ids: Vec<_> = self.backends.iter().map(|b| b.id()).collect();
        f.debug_struct("ColorBackendRegistry")
            .field("backends", &ids)
            .finish()
    }
}

static REGISTRY: OnceLock<ColorBackendRegistry> = OnceLock::new();

impl ColorBackendRegistry {
    /// Process-wide singleton, initialized on first access with the
    /// default backend set.
    pub fn global() -> &'static ColorBackendRegistry {
        REGISTRY.get_or_init(Self::default_set)
    }

    /// The bundled backend set. Passthrough sits at the top of the
    /// priority list so under `Auto`, an H264AnnexB-in / H264-out
    /// stream gets relayed verbatim instead of bouncing through an
    /// unnecessary transcode.
    pub fn default_set() -> Self {
        let mut backends: Vec<Arc<dyn ColorEncoderBackend>> = vec![
            Arc::new(passthrough::PassthroughBackend),
            Arc::new(libav_nvidia::LibavNvidiaBackend),
            Arc::new(libav_vaapi::LibavVaapiBackend),
            Arc::new(libav_cpu::LibavCpuBackend),
        ];
        backends.sort_by_key(|b| std::cmp::Reverse(b.priority()));
        Self { backends }
    }

    /// Initialize the process-wide singleton with a custom backend set.
    /// Must be called before any call to `global()`. Panics if the
    /// registry was already initialized (i.e. `global()` was called
    /// first). Used by `rollio-encoder-x5` to inject the X5 backend.
    pub fn init_with(backends: Vec<Arc<dyn ColorEncoderBackend>>) {
        let mut sorted = backends;
        sorted.sort_by_key(|b| std::cmp::Reverse(b.priority()));
        let registry = Self { backends: sorted };
        REGISTRY.set(registry).expect(
            "ColorBackendRegistry::init_with called after registry was already initialized",
        );
    }

    pub fn backends(&self) -> &[Arc<dyn ColorEncoderBackend>] {
        &self.backends
    }

    /// Open a session for a color frame. `backend_hint` honours the
    /// project config: `Auto` walks the priority list; anything else
    /// routes directly and errors if unavailable.
    pub fn open(
        &self,
        codec: ColorCodec,
        backend_hint: EncoderBackend,
        params: &CodecSessionParams<'_>,
        first_frame: &OwnedFrame,
    ) -> Result<Box<dyn CodecSession>> {
        let input = first_frame.header.pixel_format;
        match backend_hint {
            EncoderBackend::Auto => {
                for backend in &self.backends {
                    if backend.available() && backend.supports(codec, input) {
                        return backend.open_session(params, first_frame);
                    }
                }
                Err(EncoderError::message(format!(
                    "no color backend available for codec={:?} input={:?}",
                    codec, input
                )))
            }
            specific => {
                let target = color_backend_id_from_config(specific)?;
                let backend = self
                    .backends
                    .iter()
                    .find(|b| b.id() == target)
                    .ok_or_else(|| {
                        EncoderError::message(format!(
                            "color backend {:?} not registered",
                            specific
                        ))
                    })?;
                if !backend.available() {
                    return Err(EncoderError::message(format!(
                        "color backend {} is not available on this host",
                        target.as_str()
                    )));
                }
                if !backend.supports(codec, input) {
                    return Err(EncoderError::message(format!(
                        "color backend {} does not support codec={:?} input={:?}",
                        target.as_str(),
                        codec,
                        input
                    )));
                }
                backend.open_session(params, first_frame)
            }
        }
    }
}

fn color_backend_id_from_config(value: EncoderBackend) -> Result<ColorBackendId> {
    match value {
        EncoderBackend::Cpu => Ok(ColorBackendId::Cpu),
        EncoderBackend::Nvidia => Ok(ColorBackendId::Nvidia),
        EncoderBackend::Vaapi => Ok(ColorBackendId::Vaapi),
        EncoderBackend::Passthrough => Ok(ColorBackendId::Passthrough),
        EncoderBackend::HorizonX5 => Ok(ColorBackendId::HorizonX5),
        EncoderBackend::Auto => Err(EncoderError::message(
            "color_backend_id_from_config: Auto is not a concrete backend",
        )),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};

    use rollio_types::config::{ChromaSubsampling, EncoderColorSpace};
    use rollio_types::messages::CameraFrameHeader;

    use super::*;
    use crate::media::EncodeMetrics;

    /// Programmable fake backend: lets each test wire in a specific
    /// (id, priority, available, supports) tuple and observe whether
    /// `open_session` was actually invoked.
    struct FakeBackend {
        id: ColorBackendId,
        priority: u32,
        available: bool,
        accepts: fn(ColorCodec, PixelFormat) -> bool,
        opens: AtomicU32,
    }

    impl FakeBackend {
        fn new(
            id: ColorBackendId,
            priority: u32,
            available: bool,
            accepts: fn(ColorCodec, PixelFormat) -> bool,
        ) -> Arc<Self> {
            Arc::new(Self {
                id,
                priority,
                available,
                accepts,
                opens: AtomicU32::new(0),
            })
        }

        fn open_count(&self) -> u32 {
            self.opens.load(Ordering::SeqCst)
        }
    }

    impl ColorEncoderBackend for FakeBackend {
        fn id(&self) -> ColorBackendId {
            self.id
        }
        fn priority(&self) -> u32 {
            self.priority
        }
        fn available(&self) -> bool {
            self.available
        }
        fn supports(&self, codec: ColorCodec, input: PixelFormat) -> bool {
            (self.accepts)(codec, input)
        }
        fn open_session(
            &self,
            _params: &CodecSessionParams<'_>,
            _first_frame: &OwnedFrame,
        ) -> Result<Box<dyn CodecSession>> {
            self.opens.fetch_add(1, Ordering::SeqCst);
            Ok(Box::new(FakeSession {
                metrics: EncodeMetrics::default(),
            }))
        }
    }

    struct FakeSession {
        metrics: EncodeMetrics,
    }

    impl CodecSession for FakeSession {
        fn encode(
            &mut self,
            _frame: &OwnedFrame,
            _sink: &mut dyn crate::codec::EncodedPacketSink,
        ) -> Result<()> {
            Ok(())
        }
        fn finish(self: Box<Self>, _sink: &mut dyn crate::codec::EncodedPacketSink) -> Result<()> {
            Ok(())
        }
        fn metrics(&self) -> &EncodeMetrics {
            &self.metrics
        }
        fn record_dropped(&mut self) {}
    }

    fn always(_: ColorCodec, _: PixelFormat) -> bool {
        true
    }
    fn never(_: ColorCodec, _: PixelFormat) -> bool {
        false
    }
    fn only_h264(c: ColorCodec, _: PixelFormat) -> bool {
        matches!(c, ColorCodec::H264)
    }

    fn make_frame() -> OwnedFrame {
        OwnedFrame {
            header: CameraFrameHeader {
                timestamp_us: 0,
                width: 16,
                height: 16,
                pixel_format: PixelFormat::Mjpeg,
                frame_index: 0,
            },
            payload: vec![0u8; 4],
        }
    }

    fn make_params() -> CodecSessionParams<'static> {
        CodecSessionParams {
            codec: EncoderCodec::H264,
            backend: EncoderBackend::Auto,
            fps: 30,
            crf: None,
            preset: None,
            tune: None,
            bit_depth: 8,
            chroma_subsampling: ChromaSubsampling::S420,
            color_space: EncoderColorSpace::Auto,
            process_id: "test",
            episode_index: 0,
            recording_start_us: 0,
            output_width: 16,
            output_height: 16,
            allow_rescale: false,
        }
    }

    fn registry_of(backends: Vec<Arc<dyn ColorEncoderBackend>>) -> ColorBackendRegistry {
        let mut sorted = backends;
        sorted.sort_by_key(|b| std::cmp::Reverse(b.priority()));
        ColorBackendRegistry { backends: sorted }
    }

    /// `Result<Box<dyn CodecSession>, _>::unwrap_err()` requires
    /// `Debug` on the Ok variant, which trait objects don't carry.
    /// This helper converts to a Result we can call `.unwrap_err()`
    /// on.
    fn err_of(r: Result<Box<dyn CodecSession>>) -> EncoderError {
        match r {
            Ok(_) => panic!("expected Err but got Ok"),
            Err(e) => e,
        }
    }

    #[test]
    fn default_set_priority_order_is_passthrough_nvidia_vaapi_cpu() {
        let r = ColorBackendRegistry::default_set();
        let ids: Vec<_> = r.backends().iter().map(|b| b.id()).collect();
        assert_eq!(
            ids,
            vec![
                ColorBackendId::Passthrough,
                ColorBackendId::Nvidia,
                ColorBackendId::Vaapi,
                ColorBackendId::Cpu,
            ]
        );
    }

    #[test]
    fn auto_walks_highest_priority_available_and_supporting() {
        let high = FakeBackend::new(ColorBackendId::Nvidia, 100, true, always);
        let low = FakeBackend::new(ColorBackendId::Cpu, 10, true, always);
        let r = registry_of(vec![high.clone(), low.clone()]);
        r.open(
            ColorCodec::H264,
            EncoderBackend::Auto,
            &make_params(),
            &make_frame(),
        )
        .expect("open should succeed");
        assert_eq!(high.open_count(), 1);
        assert_eq!(low.open_count(), 0);
    }

    #[test]
    fn auto_skips_unavailable_backends_and_walks_to_next() {
        let unavailable = FakeBackend::new(ColorBackendId::Nvidia, 100, false, always);
        let available = FakeBackend::new(ColorBackendId::Cpu, 10, true, always);
        let r = registry_of(vec![unavailable.clone(), available.clone()]);
        r.open(
            ColorCodec::H264,
            EncoderBackend::Auto,
            &make_params(),
            &make_frame(),
        )
        .expect("open should succeed via fallback");
        assert_eq!(unavailable.open_count(), 0);
        assert_eq!(available.open_count(), 1);
    }

    #[test]
    fn auto_skips_backends_that_dont_support_the_combo() {
        let h264_only = FakeBackend::new(ColorBackendId::Nvidia, 100, true, only_h264);
        let universal = FakeBackend::new(ColorBackendId::Cpu, 10, true, always);
        let r = registry_of(vec![h264_only.clone(), universal.clone()]);
        // Request HEVC: only the universal backend supports it.
        r.open(
            ColorCodec::H265,
            EncoderBackend::Auto,
            &make_params(),
            &make_frame(),
        )
        .expect("open should pick universal");
        assert_eq!(h264_only.open_count(), 0);
        assert_eq!(universal.open_count(), 1);
    }

    #[test]
    fn auto_errors_when_nothing_supports_combo() {
        let nope = FakeBackend::new(ColorBackendId::Cpu, 10, true, never);
        let r = registry_of(vec![nope]);
        let err = err_of(r.open(
            ColorCodec::H264,
            EncoderBackend::Auto,
            &make_params(),
            &make_frame(),
        ));
        assert!(
            err.to_string().contains("no color backend available"),
            "expected `no color backend available` in error, got: {err}"
        );
    }

    #[test]
    fn explicit_backend_routes_directly() {
        let nvidia = FakeBackend::new(ColorBackendId::Nvidia, 100, true, always);
        let cpu = FakeBackend::new(ColorBackendId::Cpu, 10, true, always);
        let r = registry_of(vec![nvidia.clone(), cpu.clone()]);
        // Explicit Cpu should land on Cpu even though Nvidia would
        // win under Auto.
        r.open(
            ColorCodec::H264,
            EncoderBackend::Cpu,
            &make_params(),
            &make_frame(),
        )
        .expect("explicit Cpu open should succeed");
        assert_eq!(nvidia.open_count(), 0);
        assert_eq!(cpu.open_count(), 1);
    }

    #[test]
    fn explicit_backend_errors_when_not_registered() {
        let cpu = FakeBackend::new(ColorBackendId::Cpu, 10, true, always);
        let r = registry_of(vec![cpu]);
        let err = err_of(r.open(
            ColorCodec::H264,
            EncoderBackend::Nvidia,
            &make_params(),
            &make_frame(),
        ));
        assert!(
            err.to_string().contains("not registered"),
            "expected `not registered`, got: {err}"
        );
    }

    #[test]
    fn explicit_backend_errors_when_unavailable() {
        let nvidia = FakeBackend::new(ColorBackendId::Nvidia, 100, false, always);
        let r = registry_of(vec![nvidia]);
        let err = err_of(r.open(
            ColorCodec::H264,
            EncoderBackend::Nvidia,
            &make_params(),
            &make_frame(),
        ));
        assert!(
            err.to_string().contains("not available"),
            "expected `not available`, got: {err}"
        );
    }

    #[test]
    fn explicit_backend_errors_when_unsupported_combo() {
        let nvidia = FakeBackend::new(ColorBackendId::Nvidia, 100, true, only_h264);
        let r = registry_of(vec![nvidia]);
        let err = err_of(r.open(
            ColorCodec::H265,
            EncoderBackend::Nvidia,
            &make_params(),
            &make_frame(),
        ));
        assert!(
            err.to_string().contains("does not support"),
            "expected `does not support`, got: {err}"
        );
    }

    #[test]
    fn color_codec_conversion_round_trips_color_codecs() {
        for c in [
            ColorCodec::H264,
            ColorCodec::H265,
            ColorCodec::Av1,
            ColorCodec::Mjpg,
        ] {
            let ec: EncoderCodec = c.into();
            let back = ColorCodec::try_from(ec).expect("color codec round-trip");
            assert_eq!(back, c);
        }
    }

    #[test]
    fn color_codec_conversion_rejects_rvl() {
        let err = ColorCodec::try_from(EncoderCodec::Rvl).unwrap_err();
        assert!(
            err.to_string().to_lowercase().contains("rvl"),
            "expected `rvl` in error, got: {err}"
        );
    }

    #[test]
    fn color_backend_id_from_config_rejects_auto() {
        assert!(color_backend_id_from_config(EncoderBackend::Auto).is_err());
        assert_eq!(
            color_backend_id_from_config(EncoderBackend::Cpu).unwrap(),
            ColorBackendId::Cpu
        );
        assert_eq!(
            color_backend_id_from_config(EncoderBackend::Nvidia).unwrap(),
            ColorBackendId::Nvidia
        );
        assert_eq!(
            color_backend_id_from_config(EncoderBackend::Vaapi).unwrap(),
            ColorBackendId::Vaapi
        );
        assert_eq!(
            color_backend_id_from_config(EncoderBackend::Passthrough).unwrap(),
            ColorBackendId::Passthrough
        );
    }
}
