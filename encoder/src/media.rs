use crate::error::{EncoderError, Result};
use ffmpeg_next as ffmpeg;
use rollio_types::config::{
    EncoderArtifactFormat, EncoderBackend, EncoderCapability, EncoderCapabilityDirection,
    EncoderCapabilityReport, EncoderCodec, EncoderImplementationFamily, EncoderRuntimeConfigV2,
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

pub(crate) enum SessionEncoder {
    Libav(LibavSession),
    Rvl(RvlSession),
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
    scaler: ffmpeg::software::scaling::context::Context,
    source_pixel: ffmpeg::util::format::pixel::Pixel,
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
    codecs.extend(probe_video_capabilities(
        EncoderCodec::H264,
        &[
            EncoderBackend::Cpu,
            EncoderBackend::Nvidia,
            EncoderBackend::Vaapi,
        ],
        // Gray8 is encoded by scaling to YUV420P first (chroma planes are
        // filled with neutral gray), which lets infrared cameras share the
        // video codec used for color streams.
        &[PixelFormat::Rgb24, PixelFormat::Bgr24, PixelFormat::Gray8],
        &[EncoderArtifactFormat::Mp4],
    ));
    codecs.extend(probe_video_capabilities(
        EncoderCodec::H265,
        &[
            EncoderBackend::Cpu,
            EncoderBackend::Nvidia,
            EncoderBackend::Vaapi,
        ],
        &[PixelFormat::Rgb24, PixelFormat::Bgr24, PixelFormat::Gray8],
        &[EncoderArtifactFormat::Mp4],
    ));
    codecs.extend(probe_video_capabilities(
        EncoderCodec::Av1,
        &[
            EncoderBackend::Cpu,
            EncoderBackend::Nvidia,
            EncoderBackend::Vaapi,
        ],
        &[PixelFormat::Rgb24, PixelFormat::Bgr24, PixelFormat::Gray8],
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
    }
}

pub(crate) fn finish_session(session: SessionEncoder) -> Result<EncodedArtifact> {
    match session {
        SessionEncoder::Libav(session) => session.finish(),
        SessionEncoder::Rvl(session) => session.finish(),
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

        let source_pixel = pixel_format_for_libav(first_frame.header.pixel_format)?;
        let scale_pixel = scaled_pixel_format(config.codec, actual_backend)?;
        let encoder_pixel = encoder_pixel_format(config.codec, actual_backend)?;
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

        let opened_encoder = encoder.open_as(codec)?;
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

        let scaler = ffmpeg::software::scaling::context::Context::get(
            source_pixel,
            first_frame.header.width,
            first_frame.header.height,
            scale_pixel,
            first_frame.header.width,
            first_frame.header.height,
            ffmpeg::software::scaling::flag::Flags::BILINEAR,
        )?;

        Ok(Self {
            config,
            actual_backend,
            _codec_name: codec_name.to_string(),
            output_path,
            output,
            encoder: opened_encoder,
            stream_index,
            stream_time_base,
            scaler,
            source_pixel,
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
        let mut source = ffmpeg::frame::Video::new(self.source_pixel, self.width, self.height);
        copy_frame_payload(&mut source, &frame.header, &frame.payload)?;
        source.set_pts(Some(pts_us));
        let mut converted = None;
        if self.source_pixel == self.scale_pixel {
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
            self.scaler.run(&source, &mut frame_to_scale)?;
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
                converted = Some(frame_to_scale);
            }
        }
        if let Some(frame_to_send) = converted.as_ref() {
            self.encoder.send_frame(frame_to_send)?;
        }
        self.last_pts_us = Some(pts_us);

        let before = self.metrics.encoded_bytes;
        self.receive_packets()?;
        let encoded_bytes = self.metrics.encoded_bytes - before;
        self.metrics
            .record_frame(frame.payload.len(), encoded_bytes, started.elapsed());
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
        EncoderBackend::Nvidia => {
            Path::new("/dev/nvidiactl").exists()
                || Path::new("/proc/driver/nvidia/version").exists()
        }
        EncoderBackend::Vaapi => {
            Path::new("/dev/dri/renderD128").exists() || Path::new("/dev/dri/card0").exists()
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

fn scaled_pixel_format(
    _codec: EncoderCodec,
    backend: EncoderBackend,
) -> Result<ffmpeg::util::format::pixel::Pixel> {
    let pixel = match backend {
        EncoderBackend::Cpu | EncoderBackend::Auto => ffmpeg::util::format::pixel::Pixel::YUV420P,
        EncoderBackend::Nvidia | EncoderBackend::Vaapi => ffmpeg::util::format::pixel::Pixel::NV12,
    };
    Ok(pixel)
}

fn encoder_pixel_format(
    _codec: EncoderCodec,
    backend: EncoderBackend,
) -> Result<ffmpeg::util::format::pixel::Pixel> {
    let pixel = match backend {
        EncoderBackend::Cpu | EncoderBackend::Auto => ffmpeg::util::format::pixel::Pixel::YUV420P,
        EncoderBackend::Nvidia => ffmpeg::util::format::pixel::Pixel::NV12,
        EncoderBackend::Vaapi => ffmpeg::util::format::pixel::Pixel::VAAPI,
    };
    Ok(pixel)
}

fn create_hw_device(backend: EncoderBackend) -> Result<AvBufferRef> {
    let device_type = backend_hw_device_type(backend)
        .ok_or_else(|| EncoderError::message("requested backend does not use a hardware device"))?;
    let mut device_ref = ptr::null_mut();
    let device_path = vaapi_device_path()
        .map(CString::new)
        .transpose()
        .map_err(|error| EncoderError::message(format!("invalid device path: {error}")))?;
    let device_name = if backend == EncoderBackend::Vaapi {
        device_path
            .as_ref()
            .map(|name| name.as_ptr())
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
        EncoderBackend::Cpu | EncoderBackend::Auto => return None,
    })
}

fn vaapi_device_path() -> Option<&'static str> {
    if Path::new("/dev/dri/renderD128").exists() {
        Some("/dev/dri/renderD128")
    } else if Path::new("/dev/dri/card0").exists() {
        Some("/dev/dri/card0")
    } else {
        None
    }
}

fn pixel_format_for_libav(pixel_format: PixelFormat) -> Result<ffmpeg::util::format::pixel::Pixel> {
    match pixel_format {
        PixelFormat::Rgb24 => Ok(ffmpeg::util::format::pixel::Pixel::RGB24),
        PixelFormat::Bgr24 => Ok(ffmpeg::util::format::pixel::Pixel::BGR24),
        PixelFormat::Gray8 => Ok(ffmpeg::util::format::pixel::Pixel::GRAY8),
        PixelFormat::Depth16 => Err(EncoderError::message(
            "depth16 frames are only supported via the RVL backend",
        )),
        PixelFormat::Yuyv | PixelFormat::Mjpeg => Err(EncoderError::message(
            "yuyv and mjpeg frames are not currently supported by the encoder runtime",
        )),
    }
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
        PixelFormat::Gray8 => 1,
        other => {
            return Err(EncoderError::message(format!(
                "unsupported libav source format: {:?}",
                other
            )))
        }
    };
    let row_bytes = header.width as usize * bytes_per_pixel;
    let stride = frame.stride(0);
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
}
