use crate::error::{EncoderError, Result};
use ffmpeg_next as ffmpeg;
use rollio_types::config::{
    ChromaSubsampling, EncoderArtifactFormat, EncoderBackend, EncoderCapability,
    EncoderCapabilityDirection, EncoderCapabilityReport, EncoderCodec, EncoderColorSpace,
    EncoderImplementationFamily, EncoderRuntimeConfigV2,
};
use rollio_types::messages::{CameraFrameHeader, PixelFormat};
use rvl::{
    CodecKind as RvlCodecKind, DepthDecoder, DepthEncoder, EncodedFrame as RvlEncodedFrame,
    FrameKind as RvlFrameKind,
};
use std::ffi::CString;
use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::ptr;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

const RVL_MAGIC: &[u8; 4] = b"RVL1";

#[derive(Debug, Clone)]
pub struct OwnedFrame {
    pub header: CameraFrameHeader,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, Default)]
pub struct EncodeMetrics {
    pub frames: usize,
    pub raw_bytes: usize,
    pub encoded_bytes: usize,
    pub dropped_frames: usize,
    pub encode_time: Duration,
}

impl EncodeMetrics {
    pub fn record_frame(&mut self, raw_bytes: usize, encoded_bytes: usize, elapsed: Duration) {
        self.frames += 1;
        self.raw_bytes += raw_bytes;
        self.encoded_bytes += encoded_bytes;
        self.encode_time += elapsed;
    }

    pub fn compression_ratio(&self) -> f64 {
        self.raw_bytes as f64 / self.encoded_bytes.max(1) as f64
    }

    pub fn average_encode_ms(&self) -> f64 {
        self.encode_time.as_secs_f64() * 1_000.0 / self.frames.max(1) as f64
    }
}

#[derive(Debug, Clone)]
pub struct EncodedArtifact {
    pub path: PathBuf,
    pub codec: EncoderCodec,
    pub backend: EncoderBackend,
    pub artifact_format: EncoderArtifactFormat,
    pub width: u32,
    pub height: u32,
    pub metrics: EncodeMetrics,
}

#[derive(Debug, Clone, Default)]
pub struct DecodedArtifact {
    pub width: u32,
    pub height: u32,
    pub frame_count: usize,
    pub first_rgb_frame: Option<Vec<u8>>,
    pub last_rgb_frame: Option<Vec<u8>>,
    pub first_depth_frame: Option<Vec<u16>>,
    pub last_depth_frame: Option<Vec<u16>>,
}

/// Builds downsized RGB24 preview frames for the visualizer's preview tap.
///
/// The encoder owns one of these per camera channel and feeds every
/// incoming bus frame through it (throttled to the configured preview
/// fps). The output is RGB24 at the configured preview dimensions; the
/// visualizer never sees raw YUYV/MJPG bytes.
///
/// Decoding (MJPG) and downscaling (swscale source -> RGB24 small) are
/// done eagerly per frame; reuse with the codec session would be nice
/// but isn't necessary at typical preview rates (10..60 fps).
pub struct PreviewBuilder {
    output_width: u32,
    output_height: u32,
    /// Minimum interval between published preview frames, in microseconds.
    /// Calculated from the configured `preview_fps` upper bound.
    min_interval_us: u64,
    /// Last frame timestamp we actually published, in microseconds.
    /// Frames whose `header.timestamp_us - last_emit_us < min_interval_us`
    /// are skipped without decoding.
    last_emit_us: Option<u64>,
    /// Per-session MJPEG decoder; lazily initialized when the first MJPG
    /// frame arrives. Same logic as `LibavSession::mjpeg_decoder`.
    mjpeg_decoder: Option<ffmpeg::decoder::Video>,
    /// swscale context: source pixel format -> RGB24 at preview dims.
    /// Rebuilt if the source pixel format changes mid-session.
    scaler: Option<ffmpeg::software::scaling::context::Context>,
    /// Source pixel format the current scaler expects on its input.
    scaler_input_pixel: Option<ffmpeg::util::format::pixel::Pixel>,
    /// Source dims the current scaler expects (matches the camera's
    /// native dims; we don't expect them to change mid-session but
    /// rebuild the scaler defensively if they do).
    scaler_input_dims: Option<(u32, u32)>,
}

/// One preview frame ready to publish on the iceoryx2 preview tap.
pub struct BuiltPreview {
    pub width: u32,
    pub height: u32,
    pub timestamp_us: u64,
    pub frame_index: u64,
    pub rgb: Vec<u8>,
}

impl PreviewBuilder {
    pub fn new(output_width: u32, output_height: u32, preview_fps: u32) -> Self {
        let min_interval_us = if preview_fps == 0 {
            0
        } else {
            1_000_000 / u64::from(preview_fps)
        };
        Self {
            output_width,
            output_height,
            min_interval_us,
            last_emit_us: None,
            mjpeg_decoder: None,
            scaler: None,
            scaler_input_pixel: None,
            scaler_input_dims: None,
        }
    }

    pub fn output_width(&self) -> u32 {
        self.output_width
    }

    pub fn output_height(&self) -> u32 {
        self.output_height
    }

    /// Returns Ok(Some(...)) when the throttle says it's time to publish,
    /// Ok(None) when the frame is dropped to honour `preview_fps`, and an
    /// Err for unrecoverable decode/conversion failures.
    pub fn build(&mut self, frame: &OwnedFrame) -> Result<Option<BuiltPreview>> {
        let timestamp_us = frame.header.timestamp_us;
        if !self.is_emit_due(timestamp_us) {
            return Ok(None);
        }

        // Depth16 has no libav pixel format; produce a GRAY8 source frame
        // by mapping each depth sample to an 8-bit intensity (closer is
        // brighter, fixed 1 m reference matches the legacy visualizer
        // path so the preview doesn't flicker between frames). swscale
        // then expands GRAY8 -> RGB24 at preview dimensions, sharing the
        // normal scaler caching path.
        let source = if frame.header.pixel_format == PixelFormat::Depth16 {
            depth16_to_gray8_av_frame(frame)?
        } else {
            decode_or_copy_frame_to_av(frame, &mut self.mjpeg_decoder)?
        };
        let source_pixel = source.format();
        self.ensure_scaler(source_pixel, source.width(), source.height())?;

        let scaler = self
            .scaler
            .as_mut()
            .expect("scaler should be initialized after ensure_scaler");
        let mut rgb_frame = ffmpeg::frame::Video::empty();
        scaler.run(&source, &mut rgb_frame)?;

        let rgb = compact_rgb_frame(&rgb_frame);
        self.last_emit_us = Some(timestamp_us);
        Ok(Some(BuiltPreview {
            width: self.output_width,
            height: self.output_height,
            timestamp_us,
            frame_index: frame.header.frame_index,
            rgb,
        }))
    }

    fn is_emit_due(&self, timestamp_us: u64) -> bool {
        match self.last_emit_us {
            None => true,
            Some(last) if self.min_interval_us == 0 => timestamp_us >= last,
            Some(last) => {
                timestamp_us == 0
                    || timestamp_us < last
                    || timestamp_us - last >= self.min_interval_us
            }
        }
    }

    fn ensure_scaler(
        &mut self,
        source_pixel: ffmpeg::util::format::pixel::Pixel,
        source_width: u32,
        source_height: u32,
    ) -> Result<()> {
        let dims = (source_width, source_height);
        if self.scaler.is_some()
            && self.scaler_input_pixel == Some(source_pixel)
            && self.scaler_input_dims == Some(dims)
        {
            return Ok(());
        }
        let mut scaler = ffmpeg::software::scaling::context::Context::get(
            source_pixel,
            source_width,
            source_height,
            ffmpeg::util::format::pixel::Pixel::RGB24,
            self.output_width,
            self.output_height,
            ffmpeg::software::scaling::flag::Flags::FAST_BILINEAR,
        )?;
        // RGB24 has no concept of color range; tell swscale to interpret
        // the source according to its J-format flag (full range for MJPG,
        // limited range for plain YUV422/420). Without this, MJPG previews
        // would render slightly washed compared to YUYV previews.
        set_swscale_color_range_to_mpeg(
            &mut scaler,
            source_pixel,
            ffmpeg::util::format::pixel::Pixel::RGB24,
        )?;
        self.scaler = Some(scaler);
        self.scaler_input_pixel = Some(source_pixel);
        self.scaler_input_dims = Some(dims);
        Ok(())
    }
}

/// RealSense D435 Z16 depth uses a 0.001 m scale by default, so a raw
/// value of 1000 corresponds to ~1.0 m. Keeping the preview reference
/// fixed avoids frame-to-frame flicker from per-frame normalization.
const DEPTH16_PREVIEW_REFERENCE_RAW: u32 = 1000;

/// Build a single-plane GRAY8 AVFrame from a depth16 bus payload, with
/// the same depth->intensity mapping the legacy visualizer used (closer
/// = brighter, 0 = black for invalid samples). The PreviewBuilder then
/// runs swscale GRAY8 -> RGB24 at preview dimensions like any other
/// camera, so the visualizer stays format-agnostic.
fn depth16_to_gray8_av_frame(frame: &OwnedFrame) -> Result<ffmpeg::frame::Video> {
    let width = frame.header.width;
    let height = frame.header.height;
    let expected_len = (width as usize) * (height as usize) * 2;
    if expected_len == 0 {
        return Err(EncoderError::message(
            "depth16 preview requires non-empty frame dimensions",
        ));
    }
    if frame.payload.len() < expected_len {
        return Err(EncoderError::message(format!(
            "depth16 payload too short: expected at least {} bytes for {}x{}, got {}",
            expected_len,
            width,
            height,
            frame.payload.len()
        )));
    }
    let mut gray =
        ffmpeg::frame::Video::new(ffmpeg::util::format::pixel::Pixel::GRAY8, width, height);
    let stride = gray.stride(0);
    let dst = gray.data_mut(0);
    let depth_bytes = &frame.payload[..expected_len];
    for row in 0..height as usize {
        let src_row_offset = row * (width as usize) * 2;
        let dst_row_offset = row * stride;
        for col in 0..width as usize {
            let chunk_offset = src_row_offset + col * 2;
            let depth =
                u16::from_le_bytes([depth_bytes[chunk_offset], depth_bytes[chunk_offset + 1]])
                    as u32;
            let intensity = if depth == 0 {
                0u8
            } else {
                let clamped = depth.min(DEPTH16_PREVIEW_REFERENCE_RAW);
                (((DEPTH16_PREVIEW_REFERENCE_RAW - clamped) * 255) / DEPTH16_PREVIEW_REFERENCE_RAW)
                    as u8
            };
            dst[dst_row_offset + col] = intensity;
        }
    }
    Ok(gray)
}

/// Decode an MJPG bus frame into an `AVFrame`, or copy a raw frame
/// into one. Shared by `LibavSession::decode_or_copy_source` and
/// `PreviewBuilder::build`. The MJPEG decoder is borrowed from the
/// caller so each consumer keeps its own per-session state without
/// crossing thread boundaries.
fn decode_or_copy_frame_to_av(
    frame: &OwnedFrame,
    mjpeg_decoder: &mut Option<ffmpeg::decoder::Video>,
) -> Result<ffmpeg::frame::Video> {
    match frame.header.pixel_format {
        PixelFormat::Mjpeg => {
            if mjpeg_decoder.is_none() {
                let codec = ffmpeg::decoder::find(ffmpeg::codec::Id::MJPEG)
                    .ok_or_else(|| EncoderError::message("MJPEG decoder not available in libav"))?;
                let context = ffmpeg::codec::context::Context::new_with_codec(codec);
                *mjpeg_decoder = Some(context.decoder().video()?);
            }
            let decoder = mjpeg_decoder
                .as_mut()
                .expect("MJPEG decoder is initialized");
            let packet = ffmpeg::Packet::copy(&frame.payload);
            decoder.send_packet(&packet)?;
            let mut decoded = ffmpeg::frame::Video::empty();
            if decoder.receive_frame(&mut decoded).is_err() {
                return Err(EncoderError::message(
                    "MJPEG decoder did not produce a frame for one bus payload",
                ));
            }
            Ok(decoded)
        }
        other => {
            let source_pixel = pixel_format_for_libav(other)?;
            let mut source =
                ffmpeg::frame::Video::new(source_pixel, frame.header.width, frame.header.height);
            copy_frame_payload(&mut source, &frame.header, &frame.payload)?;
            Ok(source)
        }
    }
}

pub(crate) enum SessionEncoder {
    Libav(LibavSession),
    Rvl(RvlSession),
    /// H264 frames mux'd into MP4 without re-encoding. Selected by
    /// [`open_session`] when the first frame's `pixel_format == H264`.
    Passthrough(crate::passthrough::PassthroughSession),
}

pub(crate) struct LibavSession {
    config: EncoderRuntimeConfigV2,
    actual_backend: EncoderBackend,
    _codec_name: String,
    output_path: PathBuf,
    output: ffmpeg::format::context::Output,
    encoder: ffmpeg::encoder::Video,
    stream_index: usize,
    stream_time_base: ffmpeg::Rational,
    /// Source -> codec-input pixel format scaler. Lazily built on the first
    /// frame because for MJPG sources the actual source pixel format
    /// (typically `YUVJ422P` for D4xx-class JPEGs) is only known after
    /// decoding. For non-MJPG sources we still build it lazily for code
    /// uniformity, but it's set up on the very first frame.
    scaler: Option<ffmpeg::software::scaling::context::Context>,
    /// The pixel format the scaler currently expects on its input. None
    /// until the first frame; recreated if the source format changes
    /// mid-session (e.g. an MJPG decoder switching from 422 to 420).
    scaler_input_pixel: Option<ffmpeg::util::format::pixel::Pixel>,
    /// Bus pixel format (e.g. Yuyv, Mjpeg, Rgb24) recorded for
    /// diagnostics and future preview-tap reuse logic. The frame's own
    /// header drives the decode dispatch so we stay tolerant of a config
    /// that mid-session declared a different format than its first
    /// frame; this field stays around as a one-shot snapshot of "what
    /// the session was configured to expect".
    #[allow(dead_code)]
    bus_pixel_format: PixelFormat,
    /// Per-session MJPEG decoder. Lazily initialized for `Mjpeg` sources;
    /// `None` for everything else.
    mjpeg_decoder: Option<ffmpeg::decoder::Video>,
    scale_pixel: ffmpeg::util::format::pixel::Pixel,
    encoder_pixel: ffmpeg::util::format::pixel::Pixel,
    _hw_device: Option<AvBufferRef>,
    hw_frames: Option<AvBufferRef>,
    width: u32,
    height: u32,
    /// Episode anchor (UNIX-epoch microseconds) shared with every other
    /// recording artifact. The encoded MP4 has VFR PTS in microseconds:
    /// `pts_us = frame.header.timestamp_us - recording_start_us`.
    recording_start_us: u64,
    /// Last PTS we sent to the muxer (microseconds). MP4/MKV muxers reject
    /// non-strictly-increasing PTS, so frames whose computed PTS is
    /// `<= last_pts_us` are bumped to `last_pts_us + 1` and a one-time
    /// warning is logged.
    last_pts_us: Option<i64>,
    nonmonotonic_warning_logged: bool,
    metrics: EncodeMetrics,
}

pub(crate) struct RvlSession {
    config: EncoderRuntimeConfigV2,
    output_path: PathBuf,
    writer: BufWriter<File>,
    encoder: DepthEncoder,
    width: u32,
    height: u32,
    /// See `LibavSession::recording_start_us`. The RVL writer stores each
    /// frame's absolute `timestamp_us` inline; this anchor is unused today
    /// but kept on the session for forward-compatibility with relative
    /// timestamp variants.
    _recording_start_us: u64,
    _frame_len: usize,
    metrics: EncodeMetrics,
}

static FFMPEG_INITIALIZED: OnceLock<Result<()>> = OnceLock::new();

struct AvBufferRef {
    ptr: *mut ffmpeg::ffi::AVBufferRef,
}

impl AvBufferRef {
    fn new(ptr: *mut ffmpeg::ffi::AVBufferRef, context: &str) -> Result<Self> {
        if ptr.is_null() {
            return Err(EncoderError::message(format!(
                "{context}: received null AVBufferRef"
            )));
        }
        Ok(Self { ptr })
    }

    fn clone_raw(&self) -> Result<*mut ffmpeg::ffi::AVBufferRef> {
        let cloned = unsafe { ffmpeg::ffi::av_buffer_ref(self.ptr) };
        if cloned.is_null() {
            return Err(EncoderError::message("av_buffer_ref returned null"));
        }
        Ok(cloned)
    }

    fn as_ptr(&self) -> *mut ffmpeg::ffi::AVBufferRef {
        self.ptr
    }
}

impl Drop for AvBufferRef {
    fn drop(&mut self) {
        unsafe {
            ffmpeg::ffi::av_buffer_unref(&mut self.ptr);
        }
    }
}

pub fn ensure_ffmpeg_initialized() -> Result<()> {
    match FFMPEG_INITIALIZED.get_or_init(|| ffmpeg::init().map_err(Into::into)) {
        Ok(()) => Ok(()),
        Err(error) => Err(EncoderError::message(error.to_string())),
    }
}

pub fn probe_capabilities() -> Result<EncoderCapabilityReport> {
    ensure_ffmpeg_initialized()?;

    let mut codecs = Vec::new();
    // YUYV and MJPEG are now first-class encoder inputs. The encoder
    // either copies (YUYV) or runs libavcodec's MJPEG decoder, then
    // pushes the YUV frames straight into the codec — no camera-side
    // colorspace conversion required.
    let video_pixel_formats = &[
        PixelFormat::Rgb24,
        PixelFormat::Bgr24,
        // Gray8 is encoded by scaling to YUV420P first (chroma planes
        // are filled with neutral gray), which lets infrared cameras
        // share the video codec used for color streams.
        PixelFormat::Gray8,
        PixelFormat::Yuyv,
        PixelFormat::Mjpeg,
    ];
    codecs.extend(probe_video_capabilities(
        EncoderCodec::H264,
        &[
            EncoderBackend::Cpu,
            EncoderBackend::Nvidia,
            EncoderBackend::Vaapi,
        ],
        video_pixel_formats,
        &[EncoderArtifactFormat::Mp4],
    ));
    codecs.extend(probe_video_capabilities(
        EncoderCodec::H265,
        &[
            EncoderBackend::Cpu,
            EncoderBackend::Nvidia,
            EncoderBackend::Vaapi,
        ],
        video_pixel_formats,
        &[EncoderArtifactFormat::Mp4],
    ));
    codecs.extend(probe_video_capabilities(
        EncoderCodec::Av1,
        &[
            EncoderBackend::Cpu,
            EncoderBackend::Nvidia,
            EncoderBackend::Vaapi,
        ],
        video_pixel_formats,
        &[EncoderArtifactFormat::Mkv],
    ));
    codecs.push(EncoderCapability {
        codec: EncoderCodec::Rvl,
        implementation: EncoderImplementationFamily::Rvl,
        direction: EncoderCapabilityDirection::Encode,
        backend: EncoderBackend::Cpu,
        pixel_formats: vec![PixelFormat::Depth16],
        artifact_formats: vec![EncoderArtifactFormat::Rvl],
        available: true,
        codec_name: Some("rvl".to_string()),
        note: Some("pure Rust in-repo depth encoder".to_string()),
    });
    codecs.push(EncoderCapability {
        codec: EncoderCodec::Rvl,
        implementation: EncoderImplementationFamily::Rvl,
        direction: EncoderCapabilityDirection::Decode,
        backend: EncoderBackend::Cpu,
        pixel_formats: vec![PixelFormat::Depth16],
        artifact_formats: vec![EncoderArtifactFormat::Rvl],
        available: true,
        codec_name: Some("rvl".to_string()),
        note: Some("pure Rust in-repo depth decoder".to_string()),
    });

    Ok(EncoderCapabilityReport { codecs })
}

pub(crate) fn open_session(
    runtime: &EncoderRuntimeConfigV2,
    episode_index: u32,
    recording_start_us: u64,
    first_frame: &OwnedFrame,
) -> Result<SessionEncoder> {
    fs::create_dir_all(&runtime.output_dir)?;
    let path = Path::new(&runtime.output_dir).join(runtime.output_file_name(episode_index));
    // Passthrough is gated purely by the first frame's pixel_format. When
    // a UMI bridge republishes cora's H264 video onto iceoryx2, every
    // frame is already encoded; we mux it into MP4 instead of feeding it
    // back through ffmpeg's encoder. This sidesteps the operator's
    // configured `encoder.codec` (a one-shot warning is logged below if
    // the configuration disagrees).
    if first_frame.header.pixel_format == PixelFormat::H264 {
        if runtime.codec != EncoderCodec::H264 {
            eprintln!(
                "rollio-encoder: process_id={} configured codec={:?} but first frame is H264; \
                 using passthrough mux (the configured codec is ignored).",
                runtime.process_id, runtime.codec
            );
        }
        return Ok(SessionEncoder::Passthrough(
            crate::passthrough::PassthroughSession::new(
                runtime.clone(),
                path,
                recording_start_us,
                first_frame,
            )?,
        ));
    }
    match runtime.codec {
        EncoderCodec::Rvl => Ok(SessionEncoder::Rvl(RvlSession::new(
            runtime.clone(),
            path,
            recording_start_us,
            first_frame,
        )?)),
        EncoderCodec::H264 | EncoderCodec::H265 | EncoderCodec::Av1 => Ok(SessionEncoder::Libav(
            LibavSession::new(runtime.clone(), path, recording_start_us, first_frame)?,
        )),
    }
}

pub(crate) fn encode_frame(session: &mut SessionEncoder, frame: &OwnedFrame) -> Result<()> {
    match session {
        SessionEncoder::Libav(session) => session.encode_frame(frame),
        SessionEncoder::Rvl(session) => session.encode_frame(frame),
        SessionEncoder::Passthrough(session) => session.encode_frame(frame),
    }
}

pub(crate) fn finish_session(session: SessionEncoder) -> Result<EncodedArtifact> {
    match session {
        SessionEncoder::Libav(session) => session.finish(),
        SessionEncoder::Rvl(session) => session.finish(),
        SessionEncoder::Passthrough(session) => session.finish(),
    }
}

pub fn decode_artifact(path: &Path, codec: EncoderCodec) -> Result<DecodedArtifact> {
    decode_artifact_with_backend(path, codec, EncoderBackend::Cpu)
}

pub fn decode_artifact_with_backend(
    path: &Path,
    codec: EncoderCodec,
    backend: EncoderBackend,
) -> Result<DecodedArtifact> {
    match codec {
        EncoderCodec::Rvl => decode_rvl_artifact(path),
        EncoderCodec::H264 | EncoderCodec::H265 | EncoderCodec::Av1 => {
            decode_video_artifact(path, codec, backend)
        }
    }
}

pub(crate) fn record_dropped_frame(session: &mut SessionEncoder) {
    match session {
        SessionEncoder::Libav(session) => session.metrics.dropped_frames += 1,
        SessionEncoder::Rvl(session) => session.metrics.dropped_frames += 1,
        SessionEncoder::Passthrough(session) => {
            session.metrics_mut().dropped_frames =
                session.metrics_mut().dropped_frames.saturating_add(1)
        }
    }
}

impl LibavSession {
    fn new(
        config: EncoderRuntimeConfigV2,
        output_path: PathBuf,
        recording_start_us: u64,
        first_frame: &OwnedFrame,
    ) -> Result<Self> {
        ensure_ffmpeg_initialized()?;

        let actual_backend = resolve_backend(config.codec, config.backend);
        let codec_name = select_encoder_name(config.codec, actual_backend).ok_or_else(|| {
            EncoderError::message(format!(
                "encoder backend {:?} for {} is not available",
                actual_backend,
                config.codec.as_str()
            ))
        })?;

        // Reject unsupported source formats up front so we don't allocate
        // a half-initialized session if someone configures, e.g., a depth
        // stream on the libav path.
        validate_source_pixel_format(first_frame.header.pixel_format)?;
        let bus_pixel_format = first_frame.header.pixel_format;

        let chroma_subsampling = resolve_chroma_subsampling(
            codec_name,
            actual_backend,
            config.chroma_subsampling,
            &config.process_id,
        );
        let bit_depth = resolve_bit_depth(
            codec_name,
            actual_backend,
            chroma_subsampling,
            config.bit_depth,
            &config.process_id,
        );
        let scale_pixel =
            scaled_pixel_format(config.codec, actual_backend, chroma_subsampling, bit_depth)?;
        let encoder_pixel =
            encoder_pixel_format(config.codec, actual_backend, chroma_subsampling, bit_depth)?;
        let mut output = ffmpeg::format::output(&output_path)?;
        let codec = ffmpeg::encoder::find_by_name(codec_name)
            .ok_or_else(|| EncoderError::message(format!("encoder {codec_name} not found")))?;
        let global_header = output
            .format()
            .flags()
            .contains(ffmpeg::format::Flags::GLOBAL_HEADER);
        let fps = ffmpeg::Rational(config.fps as i32, 1);

        // Use a microsecond-resolution time base so per-frame VFR PTS
        // (computed as `frame.header.timestamp_us - recording_start_us`)
        // can be sent verbatim without rescale-loss. The MP4 muxer may
        // still rewrite the *stream* time base after `write_header`; the
        // rescue below captures the post-header value.
        let encoder_time_base = ffmpeg::Rational(1, 1_000_000);

        let mut encoder = ffmpeg::codec::context::Context::new_with_codec(codec)
            .encoder()
            .video()?;
        encoder.set_width(first_frame.header.width);
        encoder.set_height(first_frame.header.height);
        encoder.set_aspect_ratio(ffmpeg::Rational(1, 1));
        encoder.set_format(encoder_pixel);
        encoder.set_frame_rate(Some(fps));
        encoder.set_time_base(encoder_time_base);
        // Force TV-range (`MPEG`) output so MJPG (full-range YUVJ*),
        // YUYV (limited-range YUV422), and the legacy RGB->YUV420P paths
        // all produce visually identical encoded video. swscale's default
        // is to keep the source's range, which would let MJPG cameras
        // emit slightly-brighter clips than YUYV cameras in the same
        // project. The encoder-side declaration below tells x264/NVENC/
        // VAAPI to advertise MPEG range in the bitstream metadata; the
        // actual quantization to TV range happens in `set_swscale_color_range`
        // when we build the scaler on the first frame.
        unsafe {
            (*encoder.as_mut_ptr()).color_range = ffmpeg::ffi::AVColorRange::AVCOL_RANGE_MPEG;
        }
        // Optional color metadata: when configured, write color
        // primaries / transfer / matrix into the bitstream so downstream
        // players don't have to guess. Default `Auto` leaves the fields
        // at the libavcodec default (`UNSPECIFIED`), matching the
        // pre-config behaviour.
        if let Some((primaries, trc, space)) = color_space_metadata(config.color_space) {
            unsafe {
                (*encoder.as_mut_ptr()).color_primaries = primaries;
                (*encoder.as_mut_ptr()).color_trc = trc;
                (*encoder.as_mut_ptr()).colorspace = space;
            }
        }
        // Disable B-frames so encoded order == display order. With B-frames
        // enabled (libx264 default = 3) the encoder emits packets where
        // `packet.pts` (display time) is smaller than `packet.dts` (decode
        // time, monotonic in encoded order) for B-frames that come after
        // a P-frame in the bitstream. The MP4 muxer enforces `pts >= dts`
        // per packet and rejects the stream with
        // `pts (X) < dts (Y) in stream 0`. With CFR `next_pts++` the
        // muxer would compute consistent DTS shifts internally, but with
        // VFR microsecond PTS (Phase 5), libx264's reorder math leaves
        // some packets violating the invariant. Setting `max_b_frames=0`
        // gives us encoded-order == display-order, so DTS == PTS for
        // every packet and the muxer is happy. The compression hit is
        // ~5-10 % vs B-frames-enabled at the same bitrate, which we
        // accept for VFR correctness.
        encoder.set_max_b_frames(0);
        if global_header {
            encoder.set_flags(ffmpeg::codec::Flags::GLOBAL_HEADER);
        }

        let hw_device = match actual_backend {
            EncoderBackend::Vaapi => Some(create_hw_device(actual_backend)?),
            _ => None,
        };
        let hw_frames = match actual_backend {
            EncoderBackend::Vaapi => Some(create_hw_frames_context(
                hw_device.as_ref().expect("vaapi device should exist"),
                encoder_pixel,
                scale_pixel,
                first_frame.header.width,
                first_frame.header.height,
                config.queue_size.max(4) as i32,
            )?),
            _ => None,
        };
        if let Some(device) = &hw_device {
            unsafe {
                (*encoder.as_mut_ptr()).hw_device_ctx = device.clone_raw()?;
            }
        }
        if let Some(frames) = &hw_frames {
            unsafe {
                (*encoder.as_mut_ptr()).hw_frames_ctx = frames.clone_raw()?;
            }
        }

        let codec_options = build_codec_options(
            codec_name,
            actual_backend,
            config.crf,
            config.preset.as_deref(),
            config.tune.as_deref(),
        );
        let opened_encoder = encoder.open_as_with(codec, codec_options)?;
        let stream_index;
        {
            let mut stream = output.add_stream(codec)?;
            stream_index = stream.index();
            stream.set_time_base(encoder_time_base);
            stream.set_parameters(&opened_encoder);
            unsafe {
                (*stream.parameters().as_mut_ptr()).codec_tag = 0;
            }
        }
        output.write_header()?;
        // MP4/mov muxers may rewrite stream time_base during write_header (often to 1/15360).
        // Packet timestamps must be rescaled to the *post-header* stream time base or duration
        // collapses to near-zero in the container (playback looks like a single flash of frames).
        let stream_time_base = output
            .stream(stream_index)
            .ok_or_else(|| EncoderError::message("missing video stream after write_header"))?
            .time_base();

        Ok(Self {
            config,
            actual_backend,
            _codec_name: codec_name.to_string(),
            output_path,
            output,
            encoder: opened_encoder,
            stream_index,
            stream_time_base,
            scaler: None,
            scaler_input_pixel: None,
            bus_pixel_format,
            mjpeg_decoder: None,
            scale_pixel,
            encoder_pixel,
            _hw_device: hw_device,
            hw_frames,
            width: first_frame.header.width,
            height: first_frame.header.height,
            recording_start_us,
            last_pts_us: None,
            nonmonotonic_warning_logged: false,
            metrics: EncodeMetrics::default(),
        })
    }

    fn encode_frame(&mut self, frame: &OwnedFrame) -> Result<()> {
        ensure_frame_compatibility(&frame.header, self.width, self.height)?;

        // Compute the per-frame PTS in microseconds (encoder time base).
        // Frames whose capture timestamp is before the recording start are
        // dropped — they are in-flight from before the user pressed record
        // and would produce a negative PTS that the muxer rejects.
        let Some(pts_us) = self.compute_pts_us(frame.header.timestamp_us) else {
            // Pre-recording frame: record as a dropped frame for metrics
            // visibility and exit.
            self.metrics.dropped_frames = self.metrics.dropped_frames.saturating_add(1);
            return Ok(());
        };

        let started = Instant::now();
        maybe_test_encode_delay();

        // Step 1: produce a `source` AVFrame from the bus payload. For
        // MJPG sources we decode the JPEG once via libavcodec; for
        // YUYV/RGB/BGR/GRAY sources we copy the payload directly into a
        // freshly-allocated frame.
        let source = self.decode_or_copy_source(frame, pts_us)?;
        let source_pixel = source.format();

        // Step 2: ensure the scaler input format matches the actual
        // source format. If we're processing the very first frame, or
        // the MJPG decoder switched chroma subsampling mid-session,
        // (re)build the scaler.
        self.ensure_scaler(source_pixel)?;

        if source_pixel == self.scale_pixel {
            // Source already matches the codec's expected input; no
            // colorspace conversion needed.
            if self.uses_hw_frames() {
                let hw_frame = upload_hw_frame(
                    self.hw_frames
                        .as_ref()
                        .expect("hardware frame pool should exist"),
                    &source,
                    self.encoder_pixel,
                )?;
                self.encoder.send_frame(&hw_frame)?;
            } else {
                self.encoder.send_frame(&source)?;
            }
        } else {
            let mut frame_to_scale = ffmpeg::frame::Video::empty();
            self.scaler
                .as_mut()
                .expect("scaler should be initialized after ensure_scaler")
                .run(&source, &mut frame_to_scale)?;
            frame_to_scale.set_pts(Some(pts_us));
            if self.uses_hw_frames() {
                let hw_frame = upload_hw_frame(
                    self.hw_frames
                        .as_ref()
                        .expect("hardware frame pool should exist"),
                    &frame_to_scale,
                    self.encoder_pixel,
                )?;
                self.encoder.send_frame(&hw_frame)?;
            } else {
                self.encoder.send_frame(&frame_to_scale)?;
            }
        }
        self.last_pts_us = Some(pts_us);

        let before = self.metrics.encoded_bytes;
        self.receive_packets()?;
        let encoded_bytes = self.metrics.encoded_bytes - before;
        self.metrics
            .record_frame(frame.payload.len(), encoded_bytes, started.elapsed());
        Ok(())
    }

    /// Produce a decoded-or-copied `AVFrame` for one bus payload. For
    /// MJPG, runs the per-session libavcodec MJPEG decoder. For raw
    /// formats (RGB/BGR/Gray8/YUYV), copies the payload row-by-row into
    /// a fresh `AVFrame`. The returned frame already has its PTS set.
    fn decode_or_copy_source(
        &mut self,
        frame: &OwnedFrame,
        pts_us: i64,
    ) -> Result<ffmpeg::frame::Video> {
        let mut decoded = decode_or_copy_frame_to_av(frame, &mut self.mjpeg_decoder)?;
        if decoded.width() != self.width || decoded.height() != self.height {
            return Err(EncoderError::message(format!(
                "decoded {:?} dimensions {}x{} differ from configured {}x{}",
                frame.header.pixel_format,
                decoded.width(),
                decoded.height(),
                self.width,
                self.height
            )));
        }
        decoded.set_pts(Some(pts_us));
        Ok(decoded)
    }

    fn ensure_scaler(&mut self, source_pixel: ffmpeg::util::format::pixel::Pixel) -> Result<()> {
        if self.scaler_input_pixel == Some(source_pixel) && self.scaler.is_some() {
            return Ok(());
        }
        if source_pixel == self.scale_pixel {
            // No scaler needed when source already equals codec input;
            // record the format so we don't keep re-checking on every
            // frame.
            self.scaler_input_pixel = Some(source_pixel);
            self.scaler = None;
            return Ok(());
        }
        let mut scaler = ffmpeg::software::scaling::context::Context::get(
            source_pixel,
            self.width,
            self.height,
            self.scale_pixel,
            self.width,
            self.height,
            ffmpeg::software::scaling::flag::Flags::BILINEAR,
        )?;
        // Force the scaler to emit limited-range YUV regardless of the
        // source's range. This makes MJPG (full range) and YUYV (limited
        // range) cameras produce visually identical output.
        set_swscale_color_range_to_mpeg(&mut scaler, source_pixel, self.scale_pixel)?;
        self.scaler = Some(scaler);
        self.scaler_input_pixel = Some(source_pixel);
        Ok(())
    }

    /// Compute the next strictly-monotonic PTS in microseconds, relative to
    /// the recording start. Delegates to the free function `compute_pts_us`
    /// so the logic can be unit-tested without spinning up a live ffmpeg
    /// encoder.
    fn compute_pts_us(&mut self, frame_timestamp_us: u64) -> Option<i64> {
        compute_pts_us(
            frame_timestamp_us,
            self.recording_start_us,
            &mut self.last_pts_us,
            &mut self.nonmonotonic_warning_logged,
        )
    }

    fn receive_packets(&mut self) -> Result<()> {
        let mut packet = ffmpeg::Packet::empty();
        while self.encoder.receive_packet(&mut packet).is_ok() {
            packet.set_stream(self.stream_index);
            packet.rescale_ts(self.encoder.time_base(), self.stream_time_base);
            self.metrics.encoded_bytes += packet.size();
            packet.write_interleaved(&mut self.output)?;
        }
        Ok(())
    }

    fn finish(mut self) -> Result<EncodedArtifact> {
        self.encoder.send_eof()?;
        self.receive_packets()?;
        self.output.write_trailer()?;
        self.metrics.encoded_bytes = fs::metadata(&self.output_path)?.len() as usize;
        Ok(EncodedArtifact {
            path: self.output_path,
            codec: self.config.codec,
            backend: self.actual_backend,
            artifact_format: self.config.resolved_artifact_format(),
            width: self.width,
            height: self.height,
            metrics: self.metrics,
        })
    }

    fn uses_hw_frames(&self) -> bool {
        self.hw_frames.is_some()
    }
}

impl RvlSession {
    fn new(
        config: EncoderRuntimeConfigV2,
        output_path: PathBuf,
        recording_start_us: u64,
        first_frame: &OwnedFrame,
    ) -> Result<Self> {
        if first_frame.header.pixel_format != PixelFormat::Depth16 {
            return Err(EncoderError::message(format!(
                "rvl requires depth16 frames, got {:?}",
                first_frame.header.pixel_format
            )));
        }
        let file = File::create(&output_path)?;
        let mut writer = BufWriter::new(file);
        writer.write_all(RVL_MAGIC)?;
        writer.write_all(&first_frame.header.width.to_le_bytes())?;
        writer.write_all(&first_frame.header.height.to_le_bytes())?;
        writer.write_all(&config.fps.to_le_bytes())?;

        let frame_len = (first_frame.header.width as usize) * (first_frame.header.height as usize);
        Ok(Self {
            config,
            output_path,
            writer,
            encoder: DepthEncoder::rvl(frame_len),
            width: first_frame.header.width,
            height: first_frame.header.height,
            _recording_start_us: recording_start_us,
            _frame_len: frame_len,
            metrics: EncodeMetrics::default(),
        })
    }

    fn encode_frame(&mut self, frame: &OwnedFrame) -> Result<()> {
        ensure_frame_compatibility(&frame.header, self.width, self.height)?;
        if frame.header.pixel_format != PixelFormat::Depth16 {
            return Err(EncoderError::message(
                "rvl session received non-depth16 frame",
            ));
        }

        let started = Instant::now();
        maybe_test_encode_delay();
        let depth_pixels = depth16_payload_to_vec(&frame.payload)?;
        let encoded = self.encoder.encode(&depth_pixels)?;
        self.writer
            .write_all(&frame.header.timestamp_us.to_le_bytes())?;
        self.writer
            .write_all(&frame.header.frame_index.to_le_bytes())?;
        self.writer
            .write_all(&(encoded.payload().len() as u32).to_le_bytes())?;
        self.writer.write_all(encoded.payload())?;
        self.metrics.record_frame(
            frame.payload.len(),
            encoded.payload().len(),
            started.elapsed(),
        );
        Ok(())
    }

    fn finish(mut self) -> Result<EncodedArtifact> {
        self.writer.flush()?;
        self.metrics.encoded_bytes = fs::metadata(&self.output_path)?.len() as usize;
        Ok(EncodedArtifact {
            path: self.output_path,
            codec: self.config.codec,
            backend: EncoderBackend::Cpu,
            artifact_format: self.config.resolved_artifact_format(),
            width: self.width,
            height: self.height,
            metrics: self.metrics,
        })
    }
}

fn probe_video_capabilities(
    codec: EncoderCodec,
    backends: &[EncoderBackend],
    pixel_formats: &[PixelFormat],
    artifact_formats: &[EncoderArtifactFormat],
) -> Vec<EncoderCapability> {
    let mut capabilities = Vec::new();
    for &backend in backends {
        let encode_name = select_encoder_name(codec, backend).map(ToOwned::to_owned);
        let decode_name = select_decoder_name(codec, backend).map(ToOwned::to_owned);

        capabilities.push(EncoderCapability {
            codec,
            implementation: EncoderImplementationFamily::Libav,
            direction: EncoderCapabilityDirection::Encode,
            backend,
            pixel_formats: pixel_formats.to_vec(),
            artifact_formats: artifact_formats.to_vec(),
            available: encode_name.is_some(),
            codec_name: encode_name.clone(),
            note: availability_note(backend, encode_name.is_some()),
        });
        capabilities.push(EncoderCapability {
            codec,
            implementation: EncoderImplementationFamily::Libav,
            direction: EncoderCapabilityDirection::Decode,
            backend,
            pixel_formats: pixel_formats.to_vec(),
            artifact_formats: artifact_formats.to_vec(),
            available: decode_name.is_some(),
            codec_name: decode_name.clone(),
            note: availability_note(backend, decode_name.is_some()),
        });
    }
    capabilities
}

fn availability_note(backend: EncoderBackend, available: bool) -> Option<String> {
    if available {
        match backend {
            EncoderBackend::Auto => Some("auto resolves to the best available backend".to_string()),
            EncoderBackend::Cpu => Some("software codec path".to_string()),
            EncoderBackend::Nvidia => {
                Some("requires CUDA/NVENC capable host libraries".to_string())
            }
            EncoderBackend::Vaapi => Some("requires VAAPI-capable host libraries".to_string()),
            EncoderBackend::Passthrough => {
                Some("mux pre-encoded H264 frames into MP4 without re-encoding".to_string())
            }
        }
    } else {
        None
    }
}

fn resolve_backend(codec: EncoderCodec, requested: EncoderBackend) -> EncoderBackend {
    if codec == EncoderCodec::Rvl {
        return EncoderBackend::Cpu;
    }
    if requested != EncoderBackend::Auto {
        return requested;
    }
    for candidate in [
        EncoderBackend::Nvidia,
        EncoderBackend::Vaapi,
        EncoderBackend::Cpu,
    ] {
        if select_encoder_name(codec, candidate).is_some() {
            return candidate;
        }
    }
    EncoderBackend::Cpu
}

fn select_encoder_name(codec: EncoderCodec, backend: EncoderBackend) -> Option<&'static str> {
    if !backend_is_usable(backend) {
        return None;
    }
    let candidates = match (codec, backend) {
        (EncoderCodec::H264, EncoderBackend::Cpu) => &["libx264", "h264"][..],
        (EncoderCodec::H264, EncoderBackend::Nvidia) => &["h264_nvenc"][..],
        (EncoderCodec::H264, EncoderBackend::Vaapi) => &["h264_vaapi"][..],
        (EncoderCodec::H265, EncoderBackend::Cpu) => &["libx265", "hevc"][..],
        (EncoderCodec::H265, EncoderBackend::Nvidia) => &["hevc_nvenc"][..],
        (EncoderCodec::H265, EncoderBackend::Vaapi) => &["hevc_vaapi"][..],
        (EncoderCodec::Av1, EncoderBackend::Cpu) => {
            &["libsvtav1", "librav1e", "libaom-av1", "av1"][..]
        }
        (EncoderCodec::Av1, EncoderBackend::Nvidia) => &["av1_nvenc"][..],
        (EncoderCodec::Av1, EncoderBackend::Vaapi) => &["av1_vaapi"][..],
        (EncoderCodec::Rvl, EncoderBackend::Cpu) => &["rvl"][..],
        _ => &[][..],
    };
    candidates
        .iter()
        .copied()
        .find(|candidate| codec_encoder_exists(candidate))
}

fn select_decoder_name(codec: EncoderCodec, backend: EncoderBackend) -> Option<&'static str> {
    if !backend_is_usable(backend) {
        return None;
    }
    let candidates = match (codec, backend) {
        (EncoderCodec::H264, EncoderBackend::Cpu) => &["h264"][..],
        (EncoderCodec::H264, EncoderBackend::Nvidia) => &["h264_cuvid"][..],
        (EncoderCodec::H264, EncoderBackend::Vaapi) => &["h264"][..],
        (EncoderCodec::H265, EncoderBackend::Cpu) => &["hevc"][..],
        (EncoderCodec::H265, EncoderBackend::Nvidia) => &["hevc_cuvid"][..],
        (EncoderCodec::H265, EncoderBackend::Vaapi) => &["hevc"][..],
        (EncoderCodec::Av1, EncoderBackend::Cpu) => &["av1"][..],
        (EncoderCodec::Av1, EncoderBackend::Nvidia) => &["av1_cuvid"][..],
        (EncoderCodec::Av1, EncoderBackend::Vaapi) => &["av1"][..],
        (EncoderCodec::Rvl, EncoderBackend::Cpu) => &["rvl"][..],
        _ => &[][..],
    };
    candidates.iter().copied().find(|name| {
        if *name == "rvl" {
            true
        } else {
            codec_decoder_exists(name)
        }
    })
}

fn backend_is_usable(backend: EncoderBackend) -> bool {
    match backend {
        EncoderBackend::Auto | EncoderBackend::Cpu => true,
        // Passthrough has no host requirement: it's a pure software mux.
        EncoderBackend::Passthrough => true,
        EncoderBackend::Nvidia => {
            Path::new("/dev/nvidiactl").exists()
                || Path::new("/proc/driver/nvidia/version").exists()
        }
        EncoderBackend::Vaapi => {
            // Any render node (not just renderD128) or legacy card0.
            fs::read_dir("/dev/dri")
                .map(|dir| {
                    dir.filter_map(|e| e.ok())
                        .any(|e| e.file_name().to_string_lossy().starts_with("renderD"))
                })
                .unwrap_or(false)
                || Path::new("/dev/dri/card0").exists()
        }
    }
}

fn codec_encoder_exists(name: &str) -> bool {
    codec_by_name(name, true)
}

fn codec_decoder_exists(name: &str) -> bool {
    codec_by_name(name, false)
}

fn codec_by_name(name: &str, encoder: bool) -> bool {
    let name = CString::new(name).expect("codec name should not contain NUL");
    unsafe {
        if encoder {
            !ffmpeg::ffi::avcodec_find_encoder_by_name(name.as_ptr()).is_null()
        } else {
            !ffmpeg::ffi::avcodec_find_decoder_by_name(name.as_ptr()).is_null()
        }
    }
}

/// Pixel format that swscale produces for the codec to consume.
///
/// CPU codecs take planar (`YUV420P` / `YUV422P` for 8-bit, `*P10LE`
/// for 10-bit) input; NVENC / VAAPI take semi-planar (`NV12` / `NV16`
/// for 8-bit, `P010LE` / `P210LE` for 10-bit). The chroma subsampling
/// is the project-wide setting from `[encoder] chroma_subsampling`,
/// narrowed down to `S420` when the resolved codec doesn't accept
/// 4:2:2 (see `resolve_chroma_subsampling`); the bit depth is the
/// project-wide setting from `[encoder] bit_depth`, narrowed to `8`
/// when the resolved codec doesn't accept 10-bit input (see
/// `resolve_bit_depth`).
fn scaled_pixel_format(
    _codec: EncoderCodec,
    backend: EncoderBackend,
    subsampling: ChromaSubsampling,
    bit_depth: u8,
) -> Result<ffmpeg::util::format::pixel::Pixel> {
    use ffmpeg::util::format::pixel::Pixel;
    let pixel = match (backend, subsampling, bit_depth) {
        (EncoderBackend::Cpu | EncoderBackend::Auto, ChromaSubsampling::S420, 8) => Pixel::YUV420P,
        (EncoderBackend::Cpu | EncoderBackend::Auto, ChromaSubsampling::S422, 8) => Pixel::YUV422P,
        (EncoderBackend::Cpu | EncoderBackend::Auto, ChromaSubsampling::S420, 10) => {
            Pixel::YUV420P10LE
        }
        (EncoderBackend::Cpu | EncoderBackend::Auto, ChromaSubsampling::S422, 10) => {
            Pixel::YUV422P10LE
        }
        (EncoderBackend::Nvidia | EncoderBackend::Vaapi, ChromaSubsampling::S420, 8) => Pixel::NV12,
        (EncoderBackend::Nvidia | EncoderBackend::Vaapi, ChromaSubsampling::S422, 8) => Pixel::NV16,
        (EncoderBackend::Nvidia | EncoderBackend::Vaapi, ChromaSubsampling::S420, 10) => {
            Pixel::P010LE
        }
        (EncoderBackend::Nvidia | EncoderBackend::Vaapi, ChromaSubsampling::S422, 10) => {
            Pixel::P210LE
        }
        (_, _, depth) => {
            return Err(EncoderError::message(format!(
                "unsupported bit_depth {depth} (must be 8 or 10); upstream validation should have rejected this"
            )));
        }
    };
    Ok(pixel)
}

fn encoder_pixel_format(
    codec: EncoderCodec,
    backend: EncoderBackend,
    subsampling: ChromaSubsampling,
    bit_depth: u8,
) -> Result<ffmpeg::util::format::pixel::Pixel> {
    use ffmpeg::util::format::pixel::Pixel;
    // VAAPI uploads from any sw input format end up in a hwaccel
    // surface; the actual chroma / bit layout is set on the
    // hw_frames_ctx sw_format, which we point at `scaled_pixel_format`.
    if backend == EncoderBackend::Vaapi {
        return Ok(Pixel::VAAPI);
    }
    // For CPU + NVENC, the encoder input format mirrors the swscale
    // output format chosen above.
    scaled_pixel_format(codec, backend, subsampling, bit_depth)
}

/// Pick the chroma subsampling we'll actually use for this encoder
/// session. Starts from the project-wide config and falls back to
/// `S420` if the resolved encoder doesn't advertise 4:2:2 input. Logs a
/// one-line stderr warning when a fallback happens so the operator
/// knows their `chroma_subsampling = "422"` setting was downgraded.
fn resolve_chroma_subsampling(
    codec_name: &str,
    backend: EncoderBackend,
    requested: ChromaSubsampling,
    process_id: &str,
) -> ChromaSubsampling {
    if requested == ChromaSubsampling::S420 {
        return ChromaSubsampling::S420;
    }
    // For VAAPI we need to check the hwaccel sw_format; the codec's
    // pix_fmts list itself only contains `Pixel::VAAPI`. Conservatively
    // assume 4:2:2 is unsupported on VAAPI unless / until we add a
    // proper feature probe — most consumer Intel iGPUs only do 4:2:0.
    if backend == EncoderBackend::Vaapi {
        eprintln!(
            "rollio-encoder: process={process_id} downgrading chroma_subsampling to 4:2:0 \
             (vaapi 4:2:2 input is not currently supported by this encoder build)"
        );
        return ChromaSubsampling::S420;
    }
    let Some(codec) = ffmpeg::encoder::find_by_name(codec_name) else {
        return ChromaSubsampling::S420;
    };
    let Ok(codec_video) = codec.video() else {
        return ChromaSubsampling::S420;
    };
    let Some(formats) = codec_video.formats() else {
        // Codec didn't advertise any pix_fmts — be conservative and
        // stick with the universally-supported 4:2:0.
        return ChromaSubsampling::S420;
    };
    let wanted = match backend {
        EncoderBackend::Cpu | EncoderBackend::Auto => ffmpeg::util::format::pixel::Pixel::YUV422P,
        EncoderBackend::Nvidia => ffmpeg::util::format::pixel::Pixel::NV16,
        EncoderBackend::Vaapi => unreachable!("vaapi handled above"),
        EncoderBackend::Passthrough => unreachable!("passthrough sessions never reach LibavSession"),
    };
    let supports_422 = formats.into_iter().any(|fmt| fmt == wanted);
    if supports_422 {
        ChromaSubsampling::S422
    } else {
        eprintln!(
            "rollio-encoder: process={process_id} downgrading chroma_subsampling to 4:2:0 \
             (codec={codec_name} does not advertise {wanted:?} as a supported input format)"
        );
        ChromaSubsampling::S420
    }
}

/// Pick the codec input bit depth we'll actually use. Starts from the
/// project-wide config and falls back to `8` when the resolved encoder
/// doesn't advertise the matching 10-bit pixel format. Logs a one-line
/// stderr warning when a fallback happens so the operator knows their
/// `bit_depth = 10` setting was downgraded (typically because the
/// host's `libx264` is the 8-bit-only build, or because the hardware
/// encoder doesn't support 10-bit input on this generation).
fn resolve_bit_depth(
    codec_name: &str,
    backend: EncoderBackend,
    subsampling: ChromaSubsampling,
    requested: u8,
    process_id: &str,
) -> u8 {
    if requested == 8 {
        return 8;
    }
    if requested != 10 {
        eprintln!(
            "rollio-encoder: process={process_id} unexpected bit_depth={requested}; \
             falling back to 8-bit (only 8 and 10 are supported)"
        );
        return 8;
    }
    if backend == EncoderBackend::Vaapi {
        eprintln!(
            "rollio-encoder: process={process_id} downgrading bit_depth to 8 \
             (vaapi 10-bit input is not currently wired up in this encoder build)"
        );
        return 8;
    }
    // NVENC's H.264 encoder is hard-coded to 8-bit YUV 4:2:0 in NVIDIA's
    // hardware: 10-bit is only available on `hevc_nvenc` and `av1_nvenc`.
    // libavcodec's `nvenc.c` shares one pix_fmts list across all three
    // NVENC codecs, so `h264_nvenc` (incorrectly) advertises `P010` as a
    // supported input. Trusting that list opens the encoder happily and
    // then NVENC fails at the first frame with `CreateInputBuffer failed:
    // invalid param (8)`. Downgrade up front so the silent fallback
    // mirrors the VAAPI handling above.
    if codec_name == "h264_nvenc" {
        eprintln!(
            "rollio-encoder: process={process_id} downgrading bit_depth to 8 \
             (codec=h264_nvenc does not support 10-bit encoding; \
             use video_codec = \"h265\" or \"av1\" for 10-bit on NVIDIA)"
        );
        return 8;
    }
    let Some(codec) = ffmpeg::encoder::find_by_name(codec_name) else {
        return 8;
    };
    let Ok(codec_video) = codec.video() else {
        return 8;
    };
    let Some(formats) = codec_video.formats() else {
        return 8;
    };
    let wanted = match (backend, subsampling) {
        (EncoderBackend::Cpu | EncoderBackend::Auto, ChromaSubsampling::S420) => {
            ffmpeg::util::format::pixel::Pixel::YUV420P10LE
        }
        (EncoderBackend::Cpu | EncoderBackend::Auto, ChromaSubsampling::S422) => {
            ffmpeg::util::format::pixel::Pixel::YUV422P10LE
        }
        (EncoderBackend::Nvidia, ChromaSubsampling::S420) => {
            ffmpeg::util::format::pixel::Pixel::P010LE
        }
        (EncoderBackend::Nvidia, ChromaSubsampling::S422) => {
            ffmpeg::util::format::pixel::Pixel::P210LE
        }
        (EncoderBackend::Vaapi, _) => unreachable!("vaapi handled above"),
        (EncoderBackend::Passthrough, _) => {
            unreachable!("passthrough sessions never reach LibavSession")
        }
    };
    if formats.into_iter().any(|fmt| fmt == wanted) {
        10
    } else {
        eprintln!(
            "rollio-encoder: process={process_id} downgrading bit_depth to 8 \
             (codec={codec_name} does not advertise {wanted:?} as a supported input format; \
             check that the host has the 10-bit build of libx264/libx265)"
        );
        8
    }
}

/// Map a configured `EncoderColorSpace` to the three ffi codec context
/// fields (`color_primaries`, `color_trc`, `colorspace`). Returns
/// `None` for `Auto` so the caller leaves the fields at the libavcodec
/// default of `UNSPECIFIED`.
fn color_space_metadata(
    color_space: EncoderColorSpace,
) -> Option<(
    ffmpeg::ffi::AVColorPrimaries,
    ffmpeg::ffi::AVColorTransferCharacteristic,
    ffmpeg::ffi::AVColorSpace,
)> {
    use ffmpeg::ffi::AVColorPrimaries::*;
    use ffmpeg::ffi::AVColorSpace::*;
    use ffmpeg::ffi::AVColorTransferCharacteristic::*;
    match color_space {
        EncoderColorSpace::Auto => None,
        EncoderColorSpace::Bt709Limited => {
            Some((AVCOL_PRI_BT709, AVCOL_TRC_BT709, AVCOL_SPC_BT709))
        }
        EncoderColorSpace::Bt601Limited => Some((
            // BT.601 NTSC primaries (smpte170m); use BT.470BG primaries
            // for PAL if a PAL-specific source ever shows up. The
            // SMPTE170M / BT.601 transfer matches both.
            AVCOL_PRI_SMPTE170M,
            AVCOL_TRC_SMPTE170M,
            AVCOL_SPC_SMPTE170M,
        )),
    }
}

/// Build the codec options dictionary that's passed to
/// `open_as_with(codec, opts)`. Translates our portable `crf` /
/// `preset` / `tune` knobs into the right per-encoder names (e.g.
/// NVENC uses `cq` instead of `crf` and needs `rc=vbr` to honour it).
fn build_codec_options(
    codec_name: &str,
    backend: EncoderBackend,
    crf: Option<u8>,
    preset: Option<&str>,
    tune: Option<&str>,
) -> ffmpeg::Dictionary<'static> {
    let mut opts = ffmpeg::Dictionary::new();
    if let Some(preset) = preset {
        opts.set("preset", preset);
    }
    if let Some(tune) = tune {
        opts.set("tune", tune);
    }
    if let Some(crf) = crf {
        let crf_str = crf.to_string();
        match (codec_name, backend) {
            // NVENC has no CRF; map to constant-quality with VBR rate
            // control. Setting `rc` ensures `cq` is actually honoured
            // instead of being silently overridden by the default
            // bitrate-target rate control.
            (
                "h264_nvenc" | "hevc_nvenc" | "av1_nvenc",
                EncoderBackend::Nvidia | EncoderBackend::Auto,
            ) => {
                opts.set("rc", "vbr");
                opts.set("cq", &crf_str);
            }
            // VAAPI uses constant-QP; set rc_mode + qp.
            (_, EncoderBackend::Vaapi) => {
                opts.set("rc_mode", "CQP");
                opts.set("qp", &crf_str);
            }
            // x264 / x265 / svtav1 / librav1e / libaom-av1 all accept
            // `crf` as a private codec option.
            _ => {
                opts.set("crf", &crf_str);
            }
        }
    }
    opts
}

fn create_hw_device(backend: EncoderBackend) -> Result<AvBufferRef> {
    let device_type = backend_hw_device_type(backend)
        .ok_or_else(|| EncoderError::message("requested backend does not use a hardware device"))?;
    let mut device_ref = ptr::null_mut();
    // Hold CString so pointers passed to av_hwdevice_ctx_create stay valid.
    let _vaapi_cstring: Option<CString> = if backend == EncoderBackend::Vaapi {
        vaapi_device_cstring()?
    } else {
        None
    };
    let device_name = if backend == EncoderBackend::Vaapi {
        _vaapi_cstring
            .as_ref()
            .map(|c| c.as_ptr())
            .unwrap_or(ptr::null())
    } else {
        ptr::null()
    };
    let error = unsafe {
        ffmpeg::ffi::av_hwdevice_ctx_create(
            &mut device_ref,
            device_type,
            device_name,
            ptr::null_mut(),
            0,
        )
    };
    if error < 0 {
        if backend == EncoderBackend::Vaapi {
            // Common case: ROLLIO picked renderD128 but that node is NVIDIA DRM while
            // the operator wants libva+Intel/AMD. Help them fix config or DRI.
            return Err(EncoderError::message(format!(
                "VAAPI hardware init failed: {}. \
                 On hybrid Intel+NVIDIA (or AMD+NVIDIA) systems the first render node is often the discrete GPU, \
                 which is not a usable libva target for h264_vaapi. \
                 Set video_backend = \"nvidia\" for NVENC, or set ROLLIO_VAAPI_DRI to an Intel/AMD node (e.g. /dev/dri/renderD129). \
                 On NVIDIA-only hardware VAAPI encode is not available; use the nvidia backend.",
                ffmpeg::Error::from(error)
            )));
        }
        return Err(ffmpeg::Error::from(error).into());
    }
    AvBufferRef::new(device_ref, "create hardware device")
}

fn create_hw_frames_context(
    device: &AvBufferRef,
    hw_format: ffmpeg::util::format::pixel::Pixel,
    sw_format: ffmpeg::util::format::pixel::Pixel,
    width: u32,
    height: u32,
    initial_pool_size: i32,
) -> Result<AvBufferRef> {
    let frames_ref = unsafe { ffmpeg::ffi::av_hwframe_ctx_alloc(device.as_ptr()) };
    let frames_ref = AvBufferRef::new(frames_ref, "allocate hardware frames context")?;
    unsafe {
        let context = (*frames_ref.as_ptr()).data as *mut ffmpeg::ffi::AVHWFramesContext;
        if context.is_null() {
            return Err(EncoderError::message(
                "hardware frames context pointer was null",
            ));
        }
        (*context).format = hw_format.into();
        (*context).sw_format = sw_format.into();
        (*context).width = width as i32;
        (*context).height = height as i32;
        (*context).initial_pool_size = initial_pool_size;
        let result = ffmpeg::ffi::av_hwframe_ctx_init(frames_ref.as_ptr());
        if result < 0 {
            return Err(ffmpeg::Error::from(result).into());
        }
    }
    Ok(frames_ref)
}

fn upload_hw_frame(
    hw_frames: &AvBufferRef,
    sw_frame: &ffmpeg::frame::Video,
    hw_format: ffmpeg::util::format::pixel::Pixel,
) -> Result<ffmpeg::frame::Video> {
    let mut hw_frame = ffmpeg::frame::Video::empty();
    hw_frame.set_format(hw_format);
    hw_frame.set_width(sw_frame.width());
    hw_frame.set_height(sw_frame.height());
    hw_frame.set_pts(sw_frame.pts());
    unsafe {
        (*hw_frame.as_mut_ptr()).hw_frames_ctx = hw_frames.clone_raw()?;
        let result =
            ffmpeg::ffi::av_hwframe_get_buffer(hw_frames.as_ptr(), hw_frame.as_mut_ptr(), 0);
        if result < 0 {
            return Err(ffmpeg::Error::from(result).into());
        }
        let result =
            ffmpeg::ffi::av_hwframe_transfer_data(hw_frame.as_mut_ptr(), sw_frame.as_ptr(), 0);
        if result < 0 {
            return Err(ffmpeg::Error::from(result).into());
        }
    }
    Ok(hw_frame)
}

fn backend_hw_device_type(backend: EncoderBackend) -> Option<ffmpeg::ffi::AVHWDeviceType> {
    Some(match backend {
        EncoderBackend::Nvidia => ffmpeg::ffi::AVHWDeviceType::AV_HWDEVICE_TYPE_CUDA,
        EncoderBackend::Vaapi => ffmpeg::ffi::AVHWDeviceType::AV_HWDEVICE_TYPE_VAAPI,
        EncoderBackend::Cpu | EncoderBackend::Auto | EncoderBackend::Passthrough => return None,
    })
}

/// DRM vendor id for **NVIDIA** (discrete on most laptops / workstations).
const DRM_VENDOR_NVIDIA: &str = "0x10de";

/// Path to DRI `renderD*` for FFmpeg VAAPI. Using `/dev/dri/renderD128` whenever it
/// exists is wrong: on many systems that is the **NVIDIA** node; `h264_vaapi` uses
/// **libva** (Intel/AMD), which then fails with *No VA display found* when pointed
/// at an NVIDIA render device.
///
/// 1. `ROLLIO_VAAPI_DRI` if set to an existing path (e.g. `/dev/dri/renderD129`)
/// 2. First `renderD*` in numeric order that is **not** an NVIDIA PCI device
/// 3. `None` so `av_hwdevice_ctx_create` gets a `NULL` device and libva can try defaults
///    (may still fail headless; override is preferred).
fn vaapi_device_cstring() -> Result<Option<CString>> {
    if let Ok(over) = std::env::var("ROLLIO_VAAPI_DRI") {
        let p = over.trim();
        if !p.is_empty() {
            if !Path::new(p).exists() {
                return Err(EncoderError::message(format!(
                    "ROLLIO_VAAPI_DRI={p:?} does not exist"
                )));
            }
            return Ok(Some(CString::new(p).map_err(|e| {
                EncoderError::message(format!("invalid ROLLIO_VAAPI_DRI: {e}"))
            })?));
        }
    }
    if let Some(path) = first_non_nvidia_render_node_path() {
        return Ok(Some(CString::new(path).map_err(|e| {
            EncoderError::message(format!("invalid VAAPI DRI path: {e}"))
        })?));
    }
    Ok(None)
}

/// Sorted list of `renderD*` basenames (e.g. `renderD128`, `renderD129`).
fn list_drm_render_d_nodes() -> Vec<String> {
    let Ok(dir) = fs::read_dir("/dev/dri") else {
        return Vec::new();
    };
    let mut names: Vec<String> = dir
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("renderD") && n.len() > 7)
        .collect();
    names.sort_by_key(|n| n[7..].parse::<u32>().unwrap_or(u32::MAX));
    names
}

fn drm_device_vendor_id(drm_name: &str) -> Option<String> {
    let path = format!("/sys/class/drm/{drm_name}/device/vendor");
    fs::read_to_string(&path).ok().map(|s| s.trim().to_string())
}

fn is_nvidia_drm_node(drm_name: &str) -> bool {
    drm_device_vendor_id(drm_name).is_some_and(|v| v.eq_ignore_ascii_case(DRM_VENDOR_NVIDIA))
}

/// Picks a render node that libva+FFmpeg can use: skip NVIDIA DRM (NVENC is separate).
fn first_non_nvidia_render_node_path() -> Option<String> {
    for name in list_drm_render_d_nodes() {
        if is_nvidia_drm_node(&name) {
            eprintln!(
                "rollio-encoder: skipping {name} for VAAPI (NVIDIA DRM; use video_backend = \"nvidia\" for NVENC, \
                 or set ROLLIO_VAAPI_DRI to an Intel/AMD render node)"
            );
            continue;
        }
        let path = format!("/dev/dri/{name}");
        if Path::new(&path).exists() {
            eprintln!("rollio-encoder: VAAPI using {path} (set ROLLIO_VAAPI_DRI to override)");
            return Some(path);
        }
    }
    eprintln!(
        "rollio-encoder: no non-NVIDIA DRI render node found for VAAPI; \
         trying libva default (NULL). Set ROLLIO_VAAPI_DRI if encoding fails."
    );
    None
}

fn pixel_format_for_libav(pixel_format: PixelFormat) -> Result<ffmpeg::util::format::pixel::Pixel> {
    match pixel_format {
        PixelFormat::Rgb24 => Ok(ffmpeg::util::format::pixel::Pixel::RGB24),
        PixelFormat::Bgr24 => Ok(ffmpeg::util::format::pixel::Pixel::BGR24),
        PixelFormat::Gray8 => Ok(ffmpeg::util::format::pixel::Pixel::GRAY8),
        PixelFormat::Yuyv => Ok(ffmpeg::util::format::pixel::Pixel::YUYV422),
        PixelFormat::Mjpeg => Err(EncoderError::message(
            "MJPEG frames are decoded via libav's MJPEG decoder, not via a direct AVFrame copy; \
             this code path should not be reached",
        )),
        PixelFormat::Depth16 => Err(EncoderError::message(
            "depth16 frames are only supported via the RVL backend",
        )),
        PixelFormat::H264 => Err(EncoderError::message(
            "H264 frames are routed to the passthrough mux; this code path should not be reached",
        )),
    }
}

/// Validate that a bus pixel format is a supported libav source. The
/// per-pixel-format sanity checks live here so `LibavSession::new` can
/// reject bad configs before allocating ffmpeg handles.
fn validate_source_pixel_format(pixel_format: PixelFormat) -> Result<()> {
    match pixel_format {
        PixelFormat::Rgb24
        | PixelFormat::Bgr24
        | PixelFormat::Gray8
        | PixelFormat::Yuyv
        | PixelFormat::Mjpeg => Ok(()),
        PixelFormat::Depth16 => Err(EncoderError::message(
            "depth16 frames are only supported via the RVL backend",
        )),
        PixelFormat::H264 => Err(EncoderError::message(
            "H264 frames are routed to the passthrough mux, not the libav encoder",
        )),
    }
}

/// Tell swscale to emit limited-range YUV (TV range, 16..235 / 16..240).
/// MJPEG decoders produce full-range YUVJ frames; without this the encoder
/// would propagate full range while we declare MPEG range on the encoder
/// context, and players that honor the metadata would render brighter
/// than expected. We force quantization to limited range here so MJPG,
/// YUYV, and the legacy RGB->YUV420P paths all yield visually identical
/// encoded output.
fn set_swscale_color_range_to_mpeg(
    scaler: &mut ffmpeg::software::scaling::context::Context,
    source_pixel: ffmpeg::util::format::pixel::Pixel,
    scale_pixel: ffmpeg::util::format::pixel::Pixel,
) -> Result<()> {
    use ffmpeg::ffi as f;
    // BT.601 vs BT.709 doesn't matter for the range-flip we want here;
    // both are accurate enough at standard webcam resolutions and
    // matches what swscale would've picked by default.
    let table = unsafe { f::sws_getCoefficients(f::SWS_CS_ITU601) };
    let src_full_range = matches!(
        source_pixel,
        ffmpeg::util::format::pixel::Pixel::YUVJ420P
            | ffmpeg::util::format::pixel::Pixel::YUVJ422P
            | ffmpeg::util::format::pixel::Pixel::YUVJ444P
    ) as i32;
    let dst_full_range = matches!(
        scale_pixel,
        ffmpeg::util::format::pixel::Pixel::YUVJ420P
            | ffmpeg::util::format::pixel::Pixel::YUVJ422P
            | ffmpeg::util::format::pixel::Pixel::YUVJ444P
    ) as i32;
    let result = unsafe {
        f::sws_setColorspaceDetails(
            scaler.as_mut_ptr(),
            table,
            src_full_range,
            table,
            dst_full_range,
            0,
            65_536,
            65_536,
        )
    };
    if result < 0 {
        return Err(EncoderError::message(format!(
            "sws_setColorspaceDetails failed (rc={result})"
        )));
    }
    Ok(())
}

fn ensure_frame_compatibility(header: &CameraFrameHeader, width: u32, height: u32) -> Result<()> {
    if header.width != width || header.height != height {
        return Err(EncoderError::message(format!(
            "frame dimensions changed during recording: expected {}x{}, got {}x{}",
            width, height, header.width, header.height
        )));
    }
    Ok(())
}

fn copy_frame_payload(
    frame: &mut ffmpeg::frame::Video,
    header: &CameraFrameHeader,
    payload: &[u8],
) -> Result<()> {
    let bytes_per_pixel = match header.pixel_format {
        PixelFormat::Rgb24 | PixelFormat::Bgr24 => 3,
        PixelFormat::Yuyv => 2,
        PixelFormat::Gray8 => 1,
        other => {
            return Err(EncoderError::message(format!(
                "unsupported libav source format for direct AVFrame copy: {:?}",
                other
            )))
        }
    };
    let row_bytes = header.width as usize * bytes_per_pixel;
    let stride = frame.stride(0);
    let expected_bytes = row_bytes * header.height as usize;
    if payload.len() < expected_bytes {
        return Err(EncoderError::message(format!(
            "{:?} payload too short: expected at least {} bytes for {}x{}, got {}",
            header.pixel_format,
            expected_bytes,
            header.width,
            header.height,
            payload.len()
        )));
    }
    for row in 0..header.height as usize {
        let src_offset = row * row_bytes;
        let dst_offset = row * stride;
        frame.data_mut(0)[dst_offset..dst_offset + row_bytes]
            .copy_from_slice(&payload[src_offset..src_offset + row_bytes]);
    }
    Ok(())
}

fn depth16_payload_to_vec(payload: &[u8]) -> Result<Vec<u16>> {
    if !payload.len().is_multiple_of(2) {
        return Err(EncoderError::message(format!(
            "depth16 payload must have even length, got {}",
            payload.len()
        )));
    }
    Ok(payload
        .chunks_exact(2)
        .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
        .collect())
}

fn decode_video_artifact(
    path: &Path,
    codec: EncoderCodec,
    backend: EncoderBackend,
) -> Result<DecodedArtifact> {
    ensure_ffmpeg_initialized()?;
    let mut input = ffmpeg::format::input(path)?;
    let stream = input
        .streams()
        .best(ffmpeg::media::Type::Video)
        .ok_or_else(|| EncoderError::message("video stream not found"))?;
    let stream_index = stream.index();
    let mut hw_device = None;
    let mut decoder = if backend == EncoderBackend::Cpu || backend == EncoderBackend::Auto {
        let context = ffmpeg::codec::context::Context::from_parameters(stream.parameters())?;
        context.decoder().video()?
    } else {
        let decoder_name = select_decoder_name(codec, backend).ok_or_else(|| {
            EncoderError::message(format!(
                "decoder backend {:?} for {} is not available",
                backend,
                codec.as_str()
            ))
        })?;
        let decoder_codec = ffmpeg::decoder::find_by_name(decoder_name)
            .ok_or_else(|| EncoderError::message(format!("decoder {decoder_name} not found")))?;
        let mut context = ffmpeg::codec::context::Context::new_with_codec(decoder_codec);
        context.set_parameters(stream.parameters())?;
        if let Some(_device_type) = backend_hw_device_type(backend) {
            let device = create_hw_device(backend)?;
            unsafe {
                (*context.as_mut_ptr()).hw_device_ctx = device.clone_raw()?;
            }
            hw_device = Some(device);
        }
        context.decoder().open_as(decoder_codec)?.video()?
    };
    let mut scaler = None;
    let mut summary = DecodedArtifact {
        width: decoder.width(),
        height: decoder.height(),
        ..DecodedArtifact::default()
    };

    for (packet_stream, packet) in input.packets() {
        if packet_stream.index() != stream_index {
            continue;
        }
        decoder.send_packet(&packet)?;
        drain_decoder(&mut decoder, &mut scaler, &mut summary)?;
    }
    decoder.send_eof()?;
    drain_decoder(&mut decoder, &mut scaler, &mut summary)?;
    drop(hw_device);
    Ok(summary)
}

fn drain_decoder(
    decoder: &mut ffmpeg::decoder::Video,
    scaler: &mut Option<ffmpeg::software::scaling::context::Context>,
    summary: &mut DecodedArtifact,
) -> Result<()> {
    let mut decoded = ffmpeg::frame::Video::empty();
    while decoder.receive_frame(&mut decoded).is_ok() {
        if is_hardware_pixel(decoded.format()) {
            let mut sw_frame = ffmpeg::frame::Video::new(
                decoder_sw_pixel(decoder),
                decoded.width(),
                decoded.height(),
            );
            unsafe {
                let result = ffmpeg::ffi::av_hwframe_transfer_data(
                    sw_frame.as_mut_ptr(),
                    decoded.as_ptr(),
                    0,
                );
                if result < 0 {
                    return Err(ffmpeg::Error::from(result).into());
                }
            }
            process_decoded_frame(&sw_frame, scaler, summary)?;
        } else {
            process_decoded_frame(&decoded, scaler, summary)?;
        }
    }
    Ok(())
}

fn process_decoded_frame(
    frame: &ffmpeg::frame::Video,
    scaler: &mut Option<ffmpeg::software::scaling::context::Context>,
    summary: &mut DecodedArtifact,
) -> Result<()> {
    if scaler.is_none() {
        *scaler = Some(ffmpeg::software::scaling::context::Context::get(
            frame.format(),
            frame.width(),
            frame.height(),
            ffmpeg::util::format::pixel::Pixel::RGB24,
            frame.width(),
            frame.height(),
            ffmpeg::software::scaling::flag::Flags::BILINEAR,
        )?);
    }
    let mut rgb = ffmpeg::frame::Video::empty();
    scaler
        .as_mut()
        .expect("scaler should be initialized")
        .run(frame, &mut rgb)?;
    let bytes = compact_rgb_frame(&rgb);
    if summary.first_rgb_frame.is_none() {
        summary.first_rgb_frame = Some(bytes.clone());
    }
    summary.last_rgb_frame = Some(bytes);
    summary.frame_count += 1;
    Ok(())
}

fn is_hardware_pixel(pixel: ffmpeg::util::format::pixel::Pixel) -> bool {
    matches!(
        pixel,
        ffmpeg::util::format::pixel::Pixel::CUDA | ffmpeg::util::format::pixel::Pixel::VAAPI
    )
}

fn decoder_sw_pixel(decoder: &ffmpeg::decoder::Video) -> ffmpeg::util::format::pixel::Pixel {
    unsafe { ffmpeg::util::format::pixel::Pixel::from((*decoder.as_ptr()).sw_pix_fmt) }
}

fn compact_rgb_frame(frame: &ffmpeg::frame::Video) -> Vec<u8> {
    let row_bytes = frame.width() as usize * 3;
    let stride = frame.stride(0);
    let mut output = vec![0u8; row_bytes * frame.height() as usize];
    for row in 0..frame.height() as usize {
        let src_offset = row * stride;
        let dst_offset = row * row_bytes;
        output[dst_offset..dst_offset + row_bytes]
            .copy_from_slice(&frame.data(0)[src_offset..src_offset + row_bytes]);
    }
    output
}

fn maybe_test_encode_delay() {
    let delay_ms = std::env::var("ROLLIO_ENCODER_TEST_ENCODE_DELAY_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0);
    if delay_ms > 0 {
        std::thread::sleep(Duration::from_millis(delay_ms));
    }
}

fn decode_rvl_artifact(path: &Path) -> Result<DecodedArtifact> {
    let mut reader = BufReader::new(File::open(path)?);
    let mut magic = [0u8; 4];
    reader.read_exact(&mut magic)?;
    if &magic != RVL_MAGIC {
        return Err(EncoderError::message(format!(
            "invalid RVL stream magic in {}",
            path.display()
        )));
    }
    let width = read_u32(&mut reader)?;
    let height = read_u32(&mut reader)?;
    let _fps = read_u32(&mut reader)?;
    let frame_len = (width as usize) * (height as usize);
    let mut decoder = DepthDecoder::rvl(frame_len);
    let mut summary = DecodedArtifact {
        width,
        height,
        ..DecodedArtifact::default()
    };

    loop {
        let Some(_timestamp_us) = read_optional_u64(&mut reader)? else {
            break;
        };
        let _frame_index = read_u64(&mut reader)?;
        let payload_len = read_u32(&mut reader)? as usize;
        let mut payload = vec![0u8; payload_len];
        reader.read_exact(&mut payload)?;
        let frame = RvlEncodedFrame::new(RvlCodecKind::Rvl, RvlFrameKind::Key, frame_len, payload);
        let decoded = decoder.decode(&frame)?;
        if summary.first_depth_frame.is_none() {
            summary.first_depth_frame = Some(decoded.clone());
        }
        summary.last_depth_frame = Some(decoded);
        summary.frame_count += 1;
    }
    Ok(summary)
}

fn read_u32<R: Read>(reader: &mut R) -> Result<u32> {
    let mut bytes = [0u8; 4];
    reader.read_exact(&mut bytes)?;
    Ok(u32::from_le_bytes(bytes))
}

fn read_u64<R: Read>(reader: &mut R) -> Result<u64> {
    let mut bytes = [0u8; 8];
    reader.read_exact(&mut bytes)?;
    Ok(u64::from_le_bytes(bytes))
}

fn read_optional_u64<R: Read>(reader: &mut R) -> Result<Option<u64>> {
    let mut bytes = [0u8; 8];
    match reader.read_exact(&mut bytes) {
        Ok(()) => Ok(Some(u64::from_le_bytes(bytes))),
        Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => Ok(None),
        Err(error) => Err(error.into()),
    }
}

/// Free-function variant of `LibavSession::compute_pts_us` so the PTS
/// computation can be unit-tested without spinning up a libav encoder.
///
/// Semantics:
/// * `frame_timestamp_us` is the camera's UNIX-epoch wall-clock timestamp.
/// * Returns `None` for pre-recording frames (frame timestamp older than
///   the recording-start anchor); the caller drops them.
/// * Returns the strictly-monotonic PTS in microseconds otherwise. If the
///   camera publishes the same or an earlier timestamp twice in a row, the
///   PTS is bumped by one microsecond to keep the MP4 muxer happy and a
///   one-shot warning is logged via `nonmonotonic_warned`.
fn compute_pts_us(
    frame_timestamp_us: u64,
    recording_start_us: u64,
    last_pts_us: &mut Option<i64>,
    nonmonotonic_warned: &mut bool,
) -> Option<i64> {
    let raw_pts =
        i64::try_from(frame_timestamp_us).ok()? - i64::try_from(recording_start_us).ok()?;
    if raw_pts < 0 {
        return None;
    }
    let pts = match *last_pts_us {
        Some(last) if raw_pts <= last => {
            if !*nonmonotonic_warned {
                *nonmonotonic_warned = true;
                eprintln!(
                    "rollio-encoder: warning: non-monotonic frame timestamp \
                     (raw={} us, last={} us); bumping by 1 us to keep the \
                     muxer happy. Subsequent occurrences are silenced.",
                    raw_pts, last
                );
            }
            last + 1
        }
        _ => raw_pts,
    };
    *last_pts_us = Some(pts);
    Some(pts)
}

#[cfg(test)]
mod pts_tests {
    use super::*;

    #[test]
    fn pre_recording_frame_returns_none_and_does_not_advance_pts() {
        let mut last_pts_us = None;
        let mut warned = false;
        let result = compute_pts_us(999_900, 1_000_000, &mut last_pts_us, &mut warned);
        assert_eq!(result, None);
        assert_eq!(last_pts_us, None);
        assert!(!warned);
    }

    #[test]
    fn typical_increasing_timestamps_yield_relative_us_pts() {
        let mut last_pts_us = None;
        let mut warned = false;
        // Three frames at 1 ms, 34 ms, 67 ms past start.
        assert_eq!(
            compute_pts_us(1_001_000, 1_000_000, &mut last_pts_us, &mut warned),
            Some(1_000)
        );
        assert_eq!(
            compute_pts_us(1_034_000, 1_000_000, &mut last_pts_us, &mut warned),
            Some(34_000)
        );
        assert_eq!(
            compute_pts_us(1_067_000, 1_000_000, &mut last_pts_us, &mut warned),
            Some(67_000)
        );
        assert!(!warned);
    }

    #[test]
    fn duplicate_or_out_of_order_timestamp_is_bumped_by_one_us() {
        let mut last_pts_us = None;
        let mut warned = false;
        // First frame: PTS=10ms.
        assert_eq!(
            compute_pts_us(1_010_000, 1_000_000, &mut last_pts_us, &mut warned),
            Some(10_000)
        );
        // Second frame has same timestamp -> bumped to 10_001.
        assert_eq!(
            compute_pts_us(1_010_000, 1_000_000, &mut last_pts_us, &mut warned),
            Some(10_001)
        );
        assert!(warned);
        // Third frame older than the bumped PTS -> bumped to 10_002.
        assert_eq!(
            compute_pts_us(1_005_000, 1_000_000, &mut last_pts_us, &mut warned),
            Some(10_002)
        );
    }

    #[test]
    fn first_frame_at_recording_start_yields_zero_pts() {
        let mut last_pts_us = None;
        let mut warned = false;
        assert_eq!(
            compute_pts_us(1_000_000, 1_000_000, &mut last_pts_us, &mut warned),
            Some(0)
        );
        assert_eq!(last_pts_us, Some(0));
    }

    #[test]
    fn validate_source_pixel_format_accepts_supported_inputs() {
        for pf in [
            PixelFormat::Rgb24,
            PixelFormat::Bgr24,
            PixelFormat::Gray8,
            PixelFormat::Yuyv,
            PixelFormat::Mjpeg,
        ] {
            validate_source_pixel_format(pf)
                .unwrap_or_else(|err| panic!("{pf:?} should validate: {err}"));
        }
    }

    #[test]
    fn validate_source_pixel_format_rejects_depth16() {
        let err = validate_source_pixel_format(PixelFormat::Depth16)
            .expect_err("depth16 must be rejected on the libav path");
        assert!(
            err.to_string().contains("depth16"),
            "unexpected error message: {err}"
        );
    }

    #[test]
    fn pixel_format_for_libav_maps_yuyv_to_yuyv422() {
        assert_eq!(
            pixel_format_for_libav(PixelFormat::Yuyv).expect("yuyv should map"),
            ffmpeg::util::format::pixel::Pixel::YUYV422,
        );
    }

    #[test]
    fn preview_builder_throttles_to_preview_fps() {
        // 10 fps preview rate -> 100_000 us interval. Frames spaced 50_000
        // us apart should drop every other one.
        ensure_ffmpeg_initialized().expect("ffmpeg init");
        let mut builder = PreviewBuilder::new(8, 8, 10);

        // First frame at t=0 always emits.
        let frame0 = make_rgb_frame(8, 8, 0);
        let preview0 = builder.build(&frame0).expect("first frame should build");
        assert!(preview0.is_some(), "first frame should always emit");

        // Second frame at t=50_000 (50ms later) is below the 100ms
        // throttle interval and must drop.
        let mut frame1 = make_rgb_frame(8, 8, 0);
        frame1.header.timestamp_us = 50_000;
        frame1.header.frame_index = 1;
        let preview1 = builder
            .build(&frame1)
            .expect("second frame call should not error");
        assert!(
            preview1.is_none(),
            "frame within throttle interval must drop"
        );

        // Third frame at t=100_000 (100ms later) is exactly at the
        // throttle interval and must emit.
        let mut frame2 = make_rgb_frame(8, 8, 0);
        frame2.header.timestamp_us = 100_000;
        frame2.header.frame_index = 2;
        let preview2 = builder.build(&frame2).expect("third frame should build");
        assert!(preview2.is_some(), "frame at throttle boundary should emit");
    }

    #[test]
    fn scaled_pixel_format_picks_planar_yuv422_for_cpu_when_requested() {
        let pix = scaled_pixel_format(
            EncoderCodec::H264,
            EncoderBackend::Cpu,
            ChromaSubsampling::S422,
            8,
        )
        .expect("cpu+422 should resolve");
        assert_eq!(pix, ffmpeg::util::format::pixel::Pixel::YUV422P);
    }

    #[test]
    fn scaled_pixel_format_keeps_yuv420p_for_cpu_when_s420() {
        let pix = scaled_pixel_format(
            EncoderCodec::H264,
            EncoderBackend::Cpu,
            ChromaSubsampling::S420,
            8,
        )
        .expect("cpu+420 should resolve");
        assert_eq!(pix, ffmpeg::util::format::pixel::Pixel::YUV420P);
    }

    #[test]
    fn scaled_pixel_format_picks_nv16_for_nvidia_422() {
        let pix = scaled_pixel_format(
            EncoderCodec::H264,
            EncoderBackend::Nvidia,
            ChromaSubsampling::S422,
            8,
        )
        .expect("nvidia+422 should resolve");
        assert_eq!(pix, ffmpeg::util::format::pixel::Pixel::NV16);
    }

    #[test]
    fn scaled_pixel_format_picks_yuv422p10le_for_cpu_when_10_bit() {
        let pix = scaled_pixel_format(
            EncoderCodec::H264,
            EncoderBackend::Cpu,
            ChromaSubsampling::S422,
            10,
        )
        .expect("cpu+422+10 should resolve");
        assert_eq!(pix, ffmpeg::util::format::pixel::Pixel::YUV422P10LE);
    }

    #[test]
    fn scaled_pixel_format_picks_p010le_for_nvidia_when_10_bit_420() {
        let pix = scaled_pixel_format(
            EncoderCodec::H264,
            EncoderBackend::Nvidia,
            ChromaSubsampling::S420,
            10,
        )
        .expect("nvidia+420+10 should resolve");
        assert_eq!(pix, ffmpeg::util::format::pixel::Pixel::P010LE);
    }

    #[test]
    fn resolve_bit_depth_passes_through_8_unchanged() {
        let resolved = resolve_bit_depth(
            "libx264",
            EncoderBackend::Cpu,
            ChromaSubsampling::S422,
            8,
            "test",
        );
        assert_eq!(resolved, 8);
    }

    #[test]
    fn resolve_bit_depth_downgrades_unknown_value() {
        let resolved = resolve_bit_depth(
            "libx264",
            EncoderBackend::Cpu,
            ChromaSubsampling::S422,
            12,
            "test",
        );
        assert_eq!(resolved, 8);
    }

    #[test]
    fn resolve_bit_depth_always_downgrades_for_vaapi() {
        let resolved = resolve_bit_depth(
            "h264_vaapi",
            EncoderBackend::Vaapi,
            ChromaSubsampling::S420,
            10,
            "test",
        );
        assert_eq!(resolved, 8);
    }

    /// NVENC H.264 hardware does not support 10-bit encoding even though
    /// libavcodec's shared NVENC pix_fmts list advertises `P010`. Without
    /// the explicit downgrade, opening the encoder succeeds and the very
    /// first frame fails with `CreateInputBuffer failed: invalid param`.
    #[test]
    fn resolve_bit_depth_always_downgrades_for_h264_nvenc() {
        let resolved = resolve_bit_depth(
            "h264_nvenc",
            EncoderBackend::Nvidia,
            ChromaSubsampling::S420,
            10,
            "test",
        );
        assert_eq!(resolved, 8);
    }

    #[test]
    fn build_codec_options_maps_crf_to_cq_for_nvenc() {
        let opts = build_codec_options(
            "h264_nvenc",
            EncoderBackend::Nvidia,
            Some(20),
            Some("p5"),
            None,
        );
        assert_eq!(opts.get("cq"), Some("20"));
        assert_eq!(opts.get("rc"), Some("vbr"));
        assert_eq!(opts.get("preset"), Some("p5"));
        assert_eq!(opts.get("crf"), None);
    }

    #[test]
    fn build_codec_options_maps_crf_to_qp_for_vaapi() {
        let opts = build_codec_options("h264_vaapi", EncoderBackend::Vaapi, Some(22), None, None);
        assert_eq!(opts.get("qp"), Some("22"));
        assert_eq!(opts.get("rc_mode"), Some("CQP"));
    }

    #[test]
    fn build_codec_options_uses_crf_for_libx264() {
        let opts = build_codec_options(
            "libx264",
            EncoderBackend::Cpu,
            Some(18),
            Some("slow"),
            Some("film"),
        );
        assert_eq!(opts.get("crf"), Some("18"));
        assert_eq!(opts.get("preset"), Some("slow"));
        assert_eq!(opts.get("tune"), Some("film"));
    }

    #[test]
    fn build_codec_options_emits_nothing_when_all_unset() {
        let opts = build_codec_options("libx264", EncoderBackend::Cpu, None, None, None);
        assert_eq!(opts.get("crf"), None);
        assert_eq!(opts.get("preset"), None);
        assert_eq!(opts.get("tune"), None);
    }

    #[test]
    fn color_space_metadata_maps_bt709_correctly() {
        let mapped = color_space_metadata(EncoderColorSpace::Bt709Limited)
            .expect("bt709 should produce metadata");
        assert_eq!(mapped.0, ffmpeg::ffi::AVColorPrimaries::AVCOL_PRI_BT709);
        assert_eq!(
            mapped.1,
            ffmpeg::ffi::AVColorTransferCharacteristic::AVCOL_TRC_BT709
        );
        assert_eq!(mapped.2, ffmpeg::ffi::AVColorSpace::AVCOL_SPC_BT709);
    }

    #[test]
    fn color_space_metadata_returns_none_for_auto() {
        assert!(color_space_metadata(EncoderColorSpace::Auto).is_none());
    }

    #[test]
    fn resolve_chroma_subsampling_downgrades_for_unsupported_codec() {
        ensure_ffmpeg_initialized().expect("ffmpeg init");
        // libsvtav1 only supports 4:2:0, so requesting 4:2:2 must
        // downgrade. If libsvtav1 isn't built into this ffmpeg, the
        // codec lookup fails and we still return S420.
        let resolved = resolve_chroma_subsampling(
            "libsvtav1",
            EncoderBackend::Cpu,
            ChromaSubsampling::S422,
            "test-process",
        );
        assert_eq!(resolved, ChromaSubsampling::S420);
    }

    #[test]
    fn resolve_chroma_subsampling_passes_through_s420_unchanged() {
        let resolved = resolve_chroma_subsampling(
            "libx264",
            EncoderBackend::Cpu,
            ChromaSubsampling::S420,
            "test-process",
        );
        assert_eq!(resolved, ChromaSubsampling::S420);
    }

    #[test]
    fn resolve_chroma_subsampling_keeps_s422_for_libx264_cpu() {
        ensure_ffmpeg_initialized().expect("ffmpeg init");
        // libx264 supports YUV422P natively. The session would happily
        // ingest 4:2:2 input on this codec/backend pair.
        let resolved = resolve_chroma_subsampling(
            "libx264",
            EncoderBackend::Cpu,
            ChromaSubsampling::S422,
            "test-process",
        );
        assert_eq!(resolved, ChromaSubsampling::S422);
    }

    #[test]
    fn resolve_chroma_subsampling_always_downgrades_for_vaapi() {
        // VAAPI 4:2:2 input is currently not wired up; conservative
        // fallback so a misconfigured project doesn't fail mid-encode.
        let resolved = resolve_chroma_subsampling(
            "h264_vaapi",
            EncoderBackend::Vaapi,
            ChromaSubsampling::S422,
            "test-process",
        );
        assert_eq!(resolved, ChromaSubsampling::S420);
    }

    #[test]
    fn preview_builder_outputs_rgb24_at_target_dimensions() {
        ensure_ffmpeg_initialized().expect("ffmpeg init");
        // High preview_fps so throttling never blocks us; we just want to
        // verify the output dims and channel count.
        let mut builder = PreviewBuilder::new(16, 12, 1000);

        let frame = make_rgb_frame(64, 48, 1);
        let preview = builder
            .build(&frame)
            .expect("build should succeed")
            .expect("first frame should emit");
        assert_eq!(preview.width, 16);
        assert_eq!(preview.height, 12);
        // 16 x 12 x 3 channels.
        assert_eq!(preview.rgb.len(), 16 * 12 * 3);
    }

    /// Regression: depth16 frames have no libav pixel format, so the
    /// PreviewBuilder used to error and the visualizer's depth tile went
    /// dark forever. The encoder now does the depth->grayscale RGB
    /// conversion on its preview tap path.
    #[test]
    fn preview_builder_handles_depth16_frames() {
        ensure_ffmpeg_initialized().expect("ffmpeg init");
        let mut builder = PreviewBuilder::new(16, 16, 1000);
        let width = 32u32;
        let height = 32u32;
        // Linear depth ramp from 0 to 1024; values >= 1000 clamp to 0
        // intensity (far / out of preview range), value 0 stays 0
        // (invalid sample), and values in [1, 999] map to a non-zero
        // intensity.
        let mut payload = Vec::with_capacity((width * height * 2) as usize);
        for i in 0..(width * height) as u32 {
            let depth = (i % 1024) as u16;
            payload.extend_from_slice(&depth.to_le_bytes());
        }
        let frame = OwnedFrame {
            header: CameraFrameHeader {
                timestamp_us: 0,
                width,
                height,
                pixel_format: PixelFormat::Depth16,
                frame_index: 0,
            },
            payload,
        };

        let preview = builder
            .build(&frame)
            .expect("depth16 preview should not error")
            .expect("depth16 frame should emit");
        assert_eq!(preview.width, 16);
        assert_eq!(preview.height, 16);
        assert_eq!(preview.rgb.len(), 16 * 16 * 3);
        // The middle of the source ramp has values around ~500 -> ~127
        // intensity, so at least one preview pixel should be neither 0
        // nor 255.
        assert!(
            preview.rgb.iter().any(|&v| v > 0 && v < 255),
            "depth preview should contain mid-range intensity values"
        );
    }

    fn make_rgb_frame(width: u32, height: u32, frame_index: u64) -> OwnedFrame {
        let pixels = (width as usize) * (height as usize);
        let mut payload = Vec::with_capacity(pixels * 3);
        for i in 0..pixels {
            // Cheap repeating colour so tests are deterministic.
            payload.extend_from_slice(&[
                (i % 256) as u8,
                ((i + 64) % 256) as u8,
                ((i + 128) % 256) as u8,
            ]);
        }
        OwnedFrame {
            header: CameraFrameHeader {
                timestamp_us: 0,
                width,
                height,
                pixel_format: PixelFormat::Rgb24,
                frame_index,
            },
            payload,
        }
    }
}
