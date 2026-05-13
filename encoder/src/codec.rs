//! Codec sessions: turn camera-native frames into encoded packets and
//! ship them through an [`EncodedPacketSink`].
//!
//! The runtime owns one `EncoderSession` per active recording or
//! preview stream. Sessions are stateful (ffmpeg encoder context,
//! per-session MJPEG decoder, etc.); the sink is borrowed by the
//! runtime and decides where the resulting packets go (recording
//! IPC, preview IPC, ...).

use crate::error::{EncoderError, Result};
use crate::media::{
    self, build_codec_options, color_space_metadata, create_hw_device, create_hw_frames_context,
    encoder_pixel_format, ensure_frame_compatibility, resolve_bit_depth,
    resolve_chroma_subsampling, scaled_pixel_format, select_encoder_name,
    set_swscale_color_range_to_mpeg, upload_hw_frame, validate_source_pixel_format, AvBufferRef,
    EncodeMetrics,
};
use crate::preview::decode_or_copy_frame_to_av;
use ffmpeg_next as ffmpeg;
use rollio_types::config::{
    container_for, ChromaSubsampling, ContainerKind, EncoderBackend, EncoderCodec,
    EncoderColorSpace, RecordingEncoderConfig,
};
use rollio_types::messages::{
    CameraFrameHeader, EncodedCodecId, EncodedPacketHeader, EncodedPacketKind, PixelFormat,
};
use rvl::{DepthEncoder, FrameKind as RvlFrameKind};
use std::time::Instant;

// ---------------------------------------------------------------------------
// OwnedFrame — owned copy of one bus payload + header
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct OwnedFrame {
    pub header: CameraFrameHeader,
    pub payload: Vec<u8>,
}

// ---------------------------------------------------------------------------
// Sink trait
// ---------------------------------------------------------------------------

/// Receives the codec session's output: one `Config` write at
/// session-open, one `Packet` write per encoded access unit, one
/// `EndOfStream` at session-finish. Sinks are responsible for
/// publishing on iceoryx2 (or, in tests, recording the calls).
///
/// Sinks are not `Send`: the iceoryx2 publishers they wrap use a
/// single-threaded arc-sync policy and must stay on the thread they
/// were created on. The encoder runtimes accommodate this by
/// constructing the sink inside the worker thread.
pub trait EncodedPacketSink {
    fn write_config(&mut self, header: EncodedPacketHeader, extradata: &[u8]) -> Result<()>;
    fn write_packet(&mut self, header: EncodedPacketHeader, payload: &[u8]) -> Result<()>;
    fn write_eos(&mut self, header: EncodedPacketHeader) -> Result<()>;
}

// ---------------------------------------------------------------------------
// CodecSession trait + dispatcher enum
// ---------------------------------------------------------------------------

/// Per-stream encoder session. The runtime calls `open` once at
/// `RecordingStart` (or preview reset), feeds frames via `encode`,
/// and finalizes via `finish`.
///
/// Sessions are not `Send`: the wrapped libavcodec encoder context
/// (and its scaler/hw-frame contexts) are not thread-safe. The
/// runtimes always create and drive a session on a single worker
/// thread.
pub trait CodecSession {
    fn encode(&mut self, frame: &OwnedFrame, sink: &mut dyn EncodedPacketSink) -> Result<()>;
    fn finish(self: Box<Self>, sink: &mut dyn EncodedPacketSink) -> Result<()>;
    fn metrics(&self) -> &EncodeMetrics;
    fn record_dropped(&mut self);
}

/// Owns the per-stream codec context for any currently-active session.
pub enum EncoderSession {
    Libav(Box<LibavCodecSession>),
    Mjpeg(Box<LibavCodecSession>),
    Rvl(Box<RvlCodecSession>),
    Passthrough(Box<PassthroughCodecSession>),
}

impl EncoderSession {
    pub fn encode(&mut self, frame: &OwnedFrame, sink: &mut dyn EncodedPacketSink) -> Result<()> {
        match self {
            Self::Libav(s) | Self::Mjpeg(s) => s.encode(frame, sink),
            Self::Rvl(s) => s.encode(frame, sink),
            Self::Passthrough(s) => s.encode(frame, sink),
        }
    }

    pub fn finish(self, sink: &mut dyn EncodedPacketSink) -> Result<()> {
        match self {
            Self::Libav(s) | Self::Mjpeg(s) => s.finish(sink),
            Self::Rvl(s) => s.finish(sink),
            Self::Passthrough(s) => s.finish(sink),
        }
    }

    pub fn metrics(&self) -> &EncodeMetrics {
        match self {
            Self::Libav(s) | Self::Mjpeg(s) => s.metrics(),
            Self::Rvl(s) => s.metrics(),
            Self::Passthrough(s) => s.metrics(),
        }
    }

    pub fn record_dropped(&mut self) {
        match self {
            Self::Libav(s) | Self::Mjpeg(s) => s.record_dropped(),
            Self::Rvl(s) => s.record_dropped(),
            Self::Passthrough(s) => s.record_dropped(),
        }
    }
}

/// Project-level inputs needed to open any codec session, mirroring
/// the recording- and preview-role-specific configs without coupling
/// the sessions to either runtime config struct directly.
pub struct CodecSessionParams<'a> {
    pub codec: EncoderCodec,
    pub backend: EncoderBackend,
    pub fps: u32,
    pub crf: Option<u8>,
    pub preset: Option<&'a str>,
    pub tune: Option<&'a str>,
    pub bit_depth: u8,
    pub chroma_subsampling: ChromaSubsampling,
    pub color_space: EncoderColorSpace,
    pub process_id: &'a str,
    pub episode_index: u32,
    pub recording_start_us: u64,
    /// Output dims. For recording sessions, equal to the camera's
    /// native dims. For preview sessions, equal to the configured
    /// preview dims; the codec session swscale-rescales arbitrary
    /// source dims to these output dims when `allow_rescale` is true.
    pub output_width: u32,
    pub output_height: u32,
    /// When true, accept frames whose source dims differ from
    /// `(output_width, output_height)` and downscale them via the
    /// session's swscale Context. Recording sessions set this to
    /// `false` so a mid-stream resize errors out (the muxer cannot
    /// deal with it). Preview-encoded sessions set it to `true`.
    pub allow_rescale: bool,
}

impl<'a> CodecSessionParams<'a> {
    pub fn from_recording(
        cfg: &'a RecordingEncoderConfig,
        process_id: &'a str,
        episode_index: u32,
        recording_start_us: u64,
        camera_width: u32,
        camera_height: u32,
    ) -> Self {
        Self {
            codec: cfg.codec,
            backend: cfg.backend,
            fps: cfg.fps,
            crf: cfg.crf,
            preset: cfg.preset.as_deref(),
            tune: cfg.tune.as_deref(),
            bit_depth: cfg.bit_depth,
            chroma_subsampling: cfg.chroma_subsampling,
            color_space: cfg.color_space,
            process_id,
            episode_index,
            recording_start_us,
            output_width: camera_width,
            output_height: camera_height,
            allow_rescale: false,
        }
    }
}

/// Open the right session for the given codec + first frame.
pub fn open_session(
    params: CodecSessionParams<'_>,
    first_frame: &OwnedFrame,
) -> Result<EncoderSession> {
    match params.codec {
        EncoderCodec::Rvl => Ok(EncoderSession::Rvl(Box::new(RvlCodecSession::new(
            &params,
            first_frame,
        )?))),
        EncoderCodec::Mjpg => Ok(EncoderSession::Mjpeg(Box::new(LibavCodecSession::new(
            &params,
            first_frame,
        )?))),
        EncoderCodec::H264 | EncoderCodec::H265 | EncoderCodec::Av1 => Ok(EncoderSession::Libav(
            Box::new(LibavCodecSession::new(&params, first_frame)?),
        )),
    }
}

// ---------------------------------------------------------------------------
// LibavCodecSession (covers H264, H265, AV1, MJPG)
// ---------------------------------------------------------------------------

pub struct LibavCodecSession {
    codec: EncoderCodec,
    actual_backend: EncoderBackend,
    width: u32,
    height: u32,
    process_id: String,
    episode_index: u32,
    recording_start_us: u64,
    encoder: ffmpeg::encoder::Video,
    /// Encoder time base (microseconds for libav-side internal PTS).
    encoder_time_base: ffmpeg::Rational,
    /// Lazily-initialized swscale source -> codec input pixel format
    /// converter. Skipped when the source pixel format already
    /// matches the encoder input (e.g. YUYV input + YUV422P encoder).
    scaler: Option<ffmpeg::software::scaling::context::Context>,
    scaler_input_pixel: Option<ffmpeg::util::format::pixel::Pixel>,
    /// Cached source dims of the swscale Context. Rebuild whenever the
    /// camera resolution shifts mid-stream so the scaler keeps producing
    /// frames sized at `(self.width, self.height)`.
    scaler_source_dims: Option<(u32, u32)>,
    /// Whether this session was opened with `allow_rescale = true`. When
    /// true, source dims that differ from `(self.width, self.height)`
    /// trigger a swscale resize instead of a hard error.
    allow_rescale: bool,
    /// Per-session MJPEG decoder for MJPG camera inputs.
    mjpeg_decoder: Option<ffmpeg::decoder::Video>,
    scale_pixel: ffmpeg::util::format::pixel::Pixel,
    encoder_pixel: ffmpeg::util::format::pixel::Pixel,
    _hw_device: Option<AvBufferRef>,
    hw_frames: Option<AvBufferRef>,
    /// Codec extradata captured immediately after the encoder is
    /// opened. Sent on `Config` packets so the assembler / visualizer
    /// can configure their decoder/muxer.
    extradata: Vec<u8>,
    config_sent: bool,
    /// Strictly increasing per-stream sequence number (0-based).
    next_sequence: u64,
    /// Last PTS we sent to the sink (microseconds, encoder time base).
    /// Used for non-monotonic-timestamp detection.
    last_pts_us: Option<i64>,
    nonmonotonic_warning_logged: bool,
    metrics: EncodeMetrics,
}

impl LibavCodecSession {
    fn new(params: &CodecSessionParams<'_>, first_frame: &OwnedFrame) -> Result<Self> {
        media::ensure_ffmpeg_initialized()?;

        let actual_backend = media::resolve_backend(params.codec, params.backend);
        let codec_name = select_encoder_name(params.codec, actual_backend).ok_or_else(|| {
            EncoderError::message(format!(
                "encoder backend {:?} for {} is not available",
                actual_backend,
                params.codec.as_str()
            ))
        })?;

        validate_source_pixel_format(first_frame.header.pixel_format)?;

        let chroma_subsampling = resolve_chroma_subsampling(
            codec_name,
            actual_backend,
            params.chroma_subsampling,
            params.process_id,
        );
        let bit_depth = resolve_bit_depth(
            codec_name,
            actual_backend,
            chroma_subsampling,
            params.bit_depth,
            params.process_id,
        );
        let scale_pixel =
            scaled_pixel_format(params.codec, actual_backend, chroma_subsampling, bit_depth)?;
        let encoder_pixel =
            encoder_pixel_format(params.codec, actual_backend, chroma_subsampling, bit_depth)?;

        let codec = ffmpeg::encoder::find_by_name(codec_name)
            .ok_or_else(|| EncoderError::message(format!("encoder {codec_name} not found")))?;
        let fps = ffmpeg::Rational(params.fps as i32, 1);
        let encoder_time_base = ffmpeg::Rational(1, 1_000_000);

        let mut encoder = ffmpeg::codec::context::Context::new_with_codec(codec)
            .encoder()
            .video()?;
        encoder.set_width(params.output_width);
        encoder.set_height(params.output_height);
        encoder.set_aspect_ratio(ffmpeg::Rational(1, 1));
        encoder.set_format(encoder_pixel);
        encoder.set_frame_rate(Some(fps));
        encoder.set_time_base(encoder_time_base);
        unsafe {
            (*encoder.as_mut_ptr()).color_range = ffmpeg::ffi::AVColorRange::AVCOL_RANGE_MPEG;
        }
        if let Some((primaries, trc, space)) = color_space_metadata(params.color_space) {
            unsafe {
                (*encoder.as_mut_ptr()).color_primaries = primaries;
                (*encoder.as_mut_ptr()).color_trc = trc;
                (*encoder.as_mut_ptr()).colorspace = space;
            }
        }
        // `max_b_frames = 0` keeps encoded order == display order so
        // the assembler-side muxer's PTS/DTS handling stays trivial.
        encoder.set_max_b_frames(0);
        // Always request global header so the codec extradata appears
        // in `extradata` immediately after `open_as`. Without this
        // some codecs (libx264 in baseline mode) leave the SPS/PPS
        // inline at every keyframe instead.
        encoder.set_flags(ffmpeg::codec::Flags::GLOBAL_HEADER);

        let hw_device = match actual_backend {
            EncoderBackend::Vaapi => Some(create_hw_device(actual_backend)?),
            _ => None,
        };
        let hw_frames = match actual_backend {
            EncoderBackend::Vaapi => Some(create_hw_frames_context(
                hw_device.as_ref().expect("vaapi device should exist"),
                encoder_pixel,
                scale_pixel,
                params.output_width,
                params.output_height,
                4,
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
            params.crf,
            params.preset,
            params.tune,
        );
        let opened_encoder = encoder
            .open_as_with(codec, codec_options)
            .map_err(|err| {
                let hint = match actual_backend {
                    EncoderBackend::Nvidia => " (NVENC enforces per-codec minimum dimensions \
                        — H.264 ~145x49 on Turing+ and ~256x128 on older Maxwell/Pascal silicon, \
                        HEVC ~129x33, AV1 ~160x64 on Ada+ — and width/height alignment constraints; \
                        for small preview streams set `[encoder.preview] backend = \"cpu\"` to use libx264 instead)",
                    EncoderBackend::Vaapi => " (VAAPI imposes its own width/height alignment, \
                        commonly multiples of 16; try `backend = \"cpu\"` if the driver rejects the configured dims)",
                    _ => "",
                };
                EncoderError::message(format!(
                    "failed to open encoder {codec_name} (codec={}, backend={:?}, \
                     resolution={}x{}, fps={}, bit_depth={}, chroma={:?}): {err}{hint}",
                    params.codec.as_str(),
                    actual_backend,
                    params.output_width,
                    params.output_height,
                    params.fps,
                    bit_depth,
                    chroma_subsampling,
                ))
            })?;

        // Capture extradata before any frame is sent. Codecs without
        // extradata (e.g. MJPG) leave the slice empty.
        let extradata = unsafe {
            let ptr = (*opened_encoder.as_ptr()).extradata;
            let len = (*opened_encoder.as_ptr()).extradata_size as usize;
            if ptr.is_null() || len == 0 {
                Vec::new()
            } else {
                std::slice::from_raw_parts(ptr, len).to_vec()
            }
        };

        Ok(Self {
            codec: params.codec,
            actual_backend,
            width: params.output_width,
            height: params.output_height,
            process_id: params.process_id.to_string(),
            episode_index: params.episode_index,
            recording_start_us: params.recording_start_us,
            encoder: opened_encoder,
            encoder_time_base,
            scaler: None,
            scaler_input_pixel: None,
            scaler_source_dims: None,
            allow_rescale: params.allow_rescale,
            mjpeg_decoder: None,
            scale_pixel,
            encoder_pixel,
            _hw_device: hw_device,
            hw_frames,
            extradata,
            config_sent: false,
            next_sequence: 0,
            last_pts_us: None,
            nonmonotonic_warning_logged: false,
            metrics: EncodeMetrics::default(),
        })
    }

    fn ensure_config_sent(&mut self, sink: &mut dyn EncodedPacketSink) -> Result<()> {
        if self.config_sent {
            return Ok(());
        }
        let header = EncodedPacketHeader {
            kind: EncodedPacketKind::Config,
            codec: encoded_codec_id(self.codec),
            flags: 0,
            width: self.width,
            height: self.height,
            pixel_format: PixelFormat::Rgb24,
            _reserved0: 0,
            time_base_num: self.encoder_time_base.numerator() as u32,
            time_base_den: self.encoder_time_base.denominator() as u32,
            pts_us: 0,
            dts_us: 0,
            duration_us: 0,
            sequence_number: self.next_sequence,
            source_timestamp_us: self.recording_start_us,
            source_frame_index: 0,
            episode_index: self.episode_index,
            payload_len: self.extradata.len() as u32,
        };
        self.next_sequence += 1;
        sink.write_config(header, &self.extradata)?;
        self.config_sent = true;
        Ok(())
    }

    fn ensure_scaler(
        &mut self,
        source_pixel: ffmpeg::util::format::pixel::Pixel,
        source_width: u32,
        source_height: u32,
    ) -> Result<()> {
        // Fast path: source pixel format matches the encoder's scale
        // pixel format AND source dims already equal our output dims —
        // no scaler needed at all (e.g. YUYV input + YUV422P encoder
        // when the camera and encoder agree on resolution).
        let dims_match = source_width == self.width && source_height == self.height;
        if source_pixel == self.scale_pixel && dims_match {
            self.scaler_input_pixel = Some(source_pixel);
            self.scaler_source_dims = Some((source_width, source_height));
            self.scaler = None;
            return Ok(());
        }
        // Reuse the cached scaler when both pixel format and source
        // dims are unchanged — this is the common case once the stream
        // has stabilized.
        if self.scaler_input_pixel == Some(source_pixel)
            && self.scaler_source_dims == Some((source_width, source_height))
            && self.scaler.is_some()
        {
            return Ok(());
        }
        let mut scaler = ffmpeg::software::scaling::context::Context::get(
            source_pixel,
            source_width,
            source_height,
            self.scale_pixel,
            self.width,
            self.height,
            ffmpeg::software::scaling::flag::Flags::BILINEAR,
        )?;
        set_swscale_color_range_to_mpeg(&mut scaler, source_pixel, self.scale_pixel)?;
        self.scaler = Some(scaler);
        self.scaler_input_pixel = Some(source_pixel);
        self.scaler_source_dims = Some((source_width, source_height));
        Ok(())
    }

    fn uses_hw_frames(&self) -> bool {
        self.hw_frames.is_some()
    }

    fn drain_packets(
        &mut self,
        frame: &OwnedFrame,
        sink: &mut dyn EncodedPacketSink,
    ) -> Result<usize> {
        let mut packet = ffmpeg::Packet::empty();
        let mut packets_emitted = 0usize;
        while self.encoder.receive_packet(&mut packet).is_ok() {
            let pts = packet.pts().unwrap_or(0);
            let dts = packet.dts().unwrap_or(pts);
            let duration = packet.duration();
            let mut header = EncodedPacketHeader {
                kind: EncodedPacketKind::Packet,
                codec: encoded_codec_id(self.codec),
                flags: 0,
                width: self.width,
                height: self.height,
                pixel_format: frame.header.pixel_format,
                _reserved0: 0,
                time_base_num: self.encoder_time_base.numerator() as u32,
                time_base_den: self.encoder_time_base.denominator() as u32,
                pts_us: pts,
                dts_us: dts,
                duration_us: duration,
                sequence_number: self.next_sequence,
                source_timestamp_us: frame.header.timestamp_us,
                source_frame_index: frame.header.frame_index,
                episode_index: self.episode_index,
                payload_len: packet.size() as u32,
            };
            header.set_keyframe(packet.is_key());
            self.next_sequence += 1;
            self.metrics.encoded_bytes += packet.size();
            sink.write_packet(header, packet.data().unwrap_or(&[]))?;
            packets_emitted += 1;
        }
        Ok(packets_emitted)
    }
}

impl CodecSession for LibavCodecSession {
    fn encode(&mut self, frame: &OwnedFrame, sink: &mut dyn EncodedPacketSink) -> Result<()> {
        ensure_frame_compatibility(&frame.header, self.width, self.height, self.allow_rescale)?;
        self.ensure_config_sent(sink)?;

        let pts_us = match crate::media::compute_pts_us(
            frame.header.timestamp_us,
            self.recording_start_us,
            &mut self.last_pts_us,
            &mut self.nonmonotonic_warning_logged,
        ) {
            Some(value) => value,
            None => {
                self.metrics.dropped_frames = self.metrics.dropped_frames.saturating_add(1);
                return Ok(());
            }
        };

        let started = Instant::now();
        let mut source = decode_or_copy_frame_to_av(frame, &mut self.mjpeg_decoder)?;
        let source_pixel = source.format();
        let source_width = source.width();
        let source_height = source.height();
        if !self.allow_rescale && (source_width != self.width || source_height != self.height) {
            return Err(EncoderError::message(format!(
                "decoded {:?} dimensions {}x{} differ from configured {}x{}",
                frame.header.pixel_format, source_width, source_height, self.width, self.height
            )));
        }
        source.set_pts(Some(pts_us));

        self.ensure_scaler(source_pixel, source_width, source_height)?;

        // Take the no-scale fast path only when source pixel format
        // matches the encoder scale pixel format AND source dims
        // already match our output dims. Otherwise — including the
        // dim-rescale case for preview-encoded sessions — push the
        // frame through swscale.
        let no_scale_needed = source_pixel == self.scale_pixel
            && source_width == self.width
            && source_height == self.height;
        if no_scale_needed {
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

        let before = self.metrics.encoded_bytes;
        self.drain_packets(frame, sink)?;
        let encoded_bytes = self.metrics.encoded_bytes - before;
        self.metrics
            .record_frame(frame.payload.len(), encoded_bytes, started.elapsed());
        Ok(())
    }

    fn finish(mut self: Box<Self>, sink: &mut dyn EncodedPacketSink) -> Result<()> {
        self.encoder.send_eof()?;
        // Drain any pending packets. Synthesize a sentinel `OwnedFrame`
        // so the helper can attach reasonable per-packet metadata.
        let sentinel = OwnedFrame {
            header: CameraFrameHeader::default(),
            payload: Vec::new(),
        };
        self.drain_packets(&sentinel, sink)?;
        let header = EncodedPacketHeader {
            kind: EncodedPacketKind::EndOfStream,
            codec: encoded_codec_id(self.codec),
            flags: 0,
            width: self.width,
            height: self.height,
            pixel_format: PixelFormat::Rgb24,
            _reserved0: 0,
            time_base_num: self.encoder_time_base.numerator() as u32,
            time_base_den: self.encoder_time_base.denominator() as u32,
            pts_us: self.last_pts_us.unwrap_or(0),
            dts_us: self.last_pts_us.unwrap_or(0),
            duration_us: 0,
            sequence_number: self.next_sequence,
            source_timestamp_us: 0,
            source_frame_index: 0,
            episode_index: self.episode_index,
            payload_len: 0,
        };
        self.next_sequence += 1;
        sink.write_eos(header)?;
        let _ = self.actual_backend; // suppress dead_code in release builds
        let _ = &self.process_id;
        Ok(())
    }

    fn metrics(&self) -> &EncodeMetrics {
        &self.metrics
    }

    fn record_dropped(&mut self) {
        self.metrics.dropped_frames = self.metrics.dropped_frames.saturating_add(1);
    }
}

// ---------------------------------------------------------------------------
// RvlCodecSession
// ---------------------------------------------------------------------------

const RVL_MAGIC: &[u8; 4] = b"RVL1";

pub struct RvlCodecSession {
    width: u32,
    height: u32,
    fps: u32,
    episode_index: u32,
    recording_start_us: u64,
    encoder: DepthEncoder,
    config_sent: bool,
    next_sequence: u64,
    metrics: EncodeMetrics,
}

impl RvlCodecSession {
    fn new(params: &CodecSessionParams<'_>, first_frame: &OwnedFrame) -> Result<Self> {
        if first_frame.header.pixel_format != PixelFormat::Depth16 {
            return Err(EncoderError::message(format!(
                "rvl requires depth16 frames, got {:?}",
                first_frame.header.pixel_format
            )));
        }
        let frame_len = (first_frame.header.width as usize) * (first_frame.header.height as usize);
        Ok(Self {
            width: first_frame.header.width,
            height: first_frame.header.height,
            fps: params.fps,
            episode_index: params.episode_index,
            recording_start_us: params.recording_start_us,
            encoder: DepthEncoder::rvl(frame_len),
            config_sent: false,
            next_sequence: 0,
            metrics: EncodeMetrics::default(),
        })
    }

    /// Build the RVL container preamble (magic + width + height + fps)
    /// used as `Config` extradata. The assembler-side muxer
    /// concatenates this with the per-frame packet payloads to
    /// reproduce the legacy `.rvl` byte layout.
    fn config_extradata(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(RVL_MAGIC.len() + 12);
        buf.extend_from_slice(RVL_MAGIC);
        buf.extend_from_slice(&self.width.to_le_bytes());
        buf.extend_from_slice(&self.height.to_le_bytes());
        buf.extend_from_slice(&self.fps.to_le_bytes());
        buf
    }

    fn ensure_config_sent(&mut self, sink: &mut dyn EncodedPacketSink) -> Result<()> {
        if self.config_sent {
            return Ok(());
        }
        let extradata = self.config_extradata();
        let header = EncodedPacketHeader {
            kind: EncodedPacketKind::Config,
            codec: EncodedCodecId::Rvl,
            flags: 0,
            width: self.width,
            height: self.height,
            pixel_format: PixelFormat::Depth16,
            _reserved0: 0,
            time_base_num: 1,
            time_base_den: 1_000_000,
            pts_us: 0,
            dts_us: 0,
            duration_us: 0,
            sequence_number: self.next_sequence,
            source_timestamp_us: self.recording_start_us,
            source_frame_index: 0,
            episode_index: self.episode_index,
            payload_len: extradata.len() as u32,
        };
        self.next_sequence += 1;
        sink.write_config(header, &extradata)?;
        self.config_sent = true;
        Ok(())
    }
}

impl CodecSession for RvlCodecSession {
    fn encode(&mut self, frame: &OwnedFrame, sink: &mut dyn EncodedPacketSink) -> Result<()> {
        ensure_frame_compatibility(&frame.header, self.width, self.height, false)?;
        if frame.header.pixel_format != PixelFormat::Depth16 {
            return Err(EncoderError::message(
                "rvl session received non-depth16 frame",
            ));
        }
        self.ensure_config_sent(sink)?;

        let started = Instant::now();
        let depth_pixels = depth16_payload_to_vec(&frame.payload)?;
        let encoded = self.encoder.encode(&depth_pixels)?;
        let payload = encoded.payload();

        // RVL framing in the packet payload mirrors today's file
        // layout: [ts_us, frame_index, payload_len, payload].
        let mut packet_payload = Vec::with_capacity(8 + 8 + 4 + payload.len());
        packet_payload.extend_from_slice(&frame.header.timestamp_us.to_le_bytes());
        packet_payload.extend_from_slice(&frame.header.frame_index.to_le_bytes());
        packet_payload.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        packet_payload.extend_from_slice(payload);

        let pts_us = (frame
            .header
            .timestamp_us
            .saturating_sub(self.recording_start_us)) as i64;
        let mut header = EncodedPacketHeader {
            kind: EncodedPacketKind::Packet,
            codec: EncodedCodecId::Rvl,
            flags: 0,
            width: self.width,
            height: self.height,
            pixel_format: PixelFormat::Depth16,
            _reserved0: 0,
            time_base_num: 1,
            time_base_den: 1_000_000,
            pts_us,
            dts_us: pts_us,
            duration_us: 0,
            sequence_number: self.next_sequence,
            source_timestamp_us: frame.header.timestamp_us,
            source_frame_index: frame.header.frame_index,
            episode_index: self.episode_index,
            payload_len: packet_payload.len() as u32,
        };
        // RVL is intra-only / lossless — every frame is a keyframe.
        header.set_keyframe(matches!(encoded.kind(), RvlFrameKind::Key));
        if !matches!(encoded.kind(), RvlFrameKind::Key) {
            // Belt and suspenders: even if rvl ever returns Delta we
            // still tag every packet as a keyframe to keep the
            // assembler's recovery semantics consistent with today.
            header.set_keyframe(true);
        }
        self.next_sequence += 1;
        self.metrics.encoded_bytes += packet_payload.len();
        sink.write_packet(header, &packet_payload)?;

        self.metrics
            .record_frame(frame.payload.len(), packet_payload.len(), started.elapsed());
        Ok(())
    }

    fn finish(mut self: Box<Self>, sink: &mut dyn EncodedPacketSink) -> Result<()> {
        let header = EncodedPacketHeader {
            kind: EncodedPacketKind::EndOfStream,
            codec: EncodedCodecId::Rvl,
            flags: 0,
            width: self.width,
            height: self.height,
            pixel_format: PixelFormat::Depth16,
            _reserved0: 0,
            time_base_num: 1,
            time_base_den: 1_000_000,
            pts_us: 0,
            dts_us: 0,
            duration_us: 0,
            sequence_number: self.next_sequence,
            source_timestamp_us: 0,
            source_frame_index: 0,
            episode_index: self.episode_index,
            payload_len: 0,
        };
        self.next_sequence += 1;
        sink.write_eos(header)?;
        Ok(())
    }

    fn metrics(&self) -> &EncodeMetrics {
        &self.metrics
    }

    fn record_dropped(&mut self) {
        self.metrics.dropped_frames = self.metrics.dropped_frames.saturating_add(1);
    }
}

// ---------------------------------------------------------------------------
// PassthroughCodecSession (stub)
// ---------------------------------------------------------------------------

pub struct PassthroughCodecSession {
    metrics: EncodeMetrics,
}

impl PassthroughCodecSession {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self {
            metrics: EncodeMetrics::default(),
        }
    }
}

impl Default for PassthroughCodecSession {
    fn default() -> Self {
        Self::new()
    }
}

impl CodecSession for PassthroughCodecSession {
    fn encode(&mut self, _frame: &OwnedFrame, _sink: &mut dyn EncodedPacketSink) -> Result<()> {
        Err(EncoderError::message(
            "PassthroughCodecSession not yet implemented (no current camera publishes encoded frames)",
        ))
    }

    fn finish(self: Box<Self>, _sink: &mut dyn EncodedPacketSink) -> Result<()> {
        Err(EncoderError::message(
            "PassthroughCodecSession not yet implemented",
        ))
    }

    fn metrics(&self) -> &EncodeMetrics {
        &self.metrics
    }

    fn record_dropped(&mut self) {
        self.metrics.dropped_frames = self.metrics.dropped_frames.saturating_add(1);
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

pub fn encoded_codec_id(codec: EncoderCodec) -> EncodedCodecId {
    match codec {
        EncoderCodec::H264 => EncodedCodecId::H264,
        EncoderCodec::H265 => EncodedCodecId::H265,
        EncoderCodec::Av1 => EncodedCodecId::Av1,
        EncoderCodec::Mjpg => EncodedCodecId::Mjpg,
        EncoderCodec::Rvl => EncodedCodecId::Rvl,
    }
}

pub fn container_for_codec(codec: EncoderCodec) -> ContainerKind {
    container_for(codec)
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

#[cfg(test)]
mod tests {
    use super::*;

    /// In-memory sink used by codec session unit tests. Records every
    /// `write_*` call so tests can assert on the produced packet
    /// stream without spinning up iceoryx2 services.
    pub struct MockSink {
        pub calls: Vec<MockSinkCall>,
    }

    #[derive(Debug, Clone)]
    pub enum MockSinkCall {
        Config {
            header: EncodedPacketHeader,
            extradata: Vec<u8>,
        },
        Packet {
            header: EncodedPacketHeader,
            payload: Vec<u8>,
        },
        Eos {
            header: EncodedPacketHeader,
        },
    }

    impl MockSink {
        pub fn new() -> Self {
            Self { calls: Vec::new() }
        }
    }

    impl EncodedPacketSink for MockSink {
        fn write_config(&mut self, header: EncodedPacketHeader, extradata: &[u8]) -> Result<()> {
            self.calls.push(MockSinkCall::Config {
                header,
                extradata: extradata.to_vec(),
            });
            Ok(())
        }

        fn write_packet(&mut self, header: EncodedPacketHeader, payload: &[u8]) -> Result<()> {
            self.calls.push(MockSinkCall::Packet {
                header,
                payload: payload.to_vec(),
            });
            Ok(())
        }

        fn write_eos(&mut self, header: EncodedPacketHeader) -> Result<()> {
            self.calls.push(MockSinkCall::Eos { header });
            Ok(())
        }
    }

    fn make_rgb_frame(width: u32, height: u32, frame_index: u64) -> OwnedFrame {
        let mut payload = vec![0u8; width as usize * height as usize * 3];
        for y in 0..height as usize {
            for x in 0..width as usize {
                let offset = (y * width as usize + x) * 3;
                payload[offset] = ((x as u64 + frame_index * 3) % 256) as u8;
                payload[offset + 1] = ((y as u64 * 2 + frame_index * 5) % 256) as u8;
                payload[offset + 2] = (((x + y) as u64 + frame_index * 7) % 256) as u8;
            }
        }
        OwnedFrame {
            header: CameraFrameHeader {
                timestamp_us: 1_000_000 + frame_index * 33_333,
                width,
                height,
                pixel_format: PixelFormat::Rgb24,
                frame_index,
            },
            payload,
        }
    }

    fn make_depth_frame(width: u32, height: u32, frame_index: u64) -> OwnedFrame {
        let pixels = (0..width * height)
            .map(|i| ((i as u64 + frame_index * 17) % 4096 + 300) as u16)
            .collect::<Vec<_>>();
        let mut payload = Vec::with_capacity(pixels.len() * 2);
        for v in pixels {
            payload.extend_from_slice(&v.to_le_bytes());
        }
        OwnedFrame {
            header: CameraFrameHeader {
                timestamp_us: 1_000_000 + frame_index * 33_333,
                width,
                height,
                pixel_format: PixelFormat::Depth16,
                frame_index,
            },
            payload,
        }
    }

    #[test]
    fn libav_h264_session_emits_config_then_packets_then_eos_in_order() {
        if select_encoder_name(EncoderCodec::H264, EncoderBackend::Cpu).is_none() {
            eprintln!("skipping: cpu h264 path unavailable");
            return;
        }
        media::ensure_ffmpeg_initialized().expect("ffmpeg init");

        let width = 64;
        let height = 48;
        let frames: Vec<_> = (0..6).map(|i| make_rgb_frame(width, height, i)).collect();
        let params = CodecSessionParams {
            codec: EncoderCodec::H264,
            backend: EncoderBackend::Cpu,
            fps: 30,
            crf: Some(28),
            preset: Some("ultrafast"),
            tune: None,
            bit_depth: 8,
            chroma_subsampling: ChromaSubsampling::S420,
            color_space: EncoderColorSpace::Auto,
            process_id: "test.session.h264",
            episode_index: 1,
            recording_start_us: frames[0].header.timestamp_us,
            output_width: width,
            output_height: height,
            allow_rescale: false,
        };
        let session = open_session(params, &frames[0]).expect("open session");
        let mut session = session;
        let mut sink = MockSink::new();
        for frame in &frames {
            session.encode(frame, &mut sink).expect("encode frame");
        }
        session.finish(&mut sink).expect("finish session");

        // The first call must be `Config` carrying non-empty extradata
        // (SPS/PPS for libx264 with global header), the last must be
        // `EndOfStream`, and every `Packet` in between must have a
        // strictly increasing sequence number.
        assert!(matches!(sink.calls[0], MockSinkCall::Config { .. }));
        if let MockSinkCall::Config { extradata, .. } = &sink.calls[0] {
            assert!(
                !extradata.is_empty(),
                "h264 SPS/PPS extradata must be present"
            );
        }
        let mut last_seq = sink.calls[0].sequence();
        let mut packets = 0usize;
        for call in sink.calls.iter().skip(1) {
            assert_eq!(
                call.sequence(),
                last_seq + 1,
                "sequence numbers must be strictly monotonic"
            );
            last_seq = call.sequence();
            if matches!(call, MockSinkCall::Packet { .. }) {
                packets += 1;
            }
        }
        assert!(
            packets >= 1,
            "at least one encoded packet should be emitted"
        );
        assert!(matches!(
            sink.calls.last().unwrap(),
            MockSinkCall::Eos { .. }
        ));
    }

    #[test]
    fn rvl_session_emits_config_then_packets_then_eos_with_keyframe_flag() {
        let width = 32;
        let height = 24;
        let frames: Vec<_> = (0..4).map(|i| make_depth_frame(width, height, i)).collect();
        let params = CodecSessionParams {
            codec: EncoderCodec::Rvl,
            backend: EncoderBackend::Cpu,
            fps: 30,
            crf: None,
            preset: None,
            tune: None,
            bit_depth: 8,
            chroma_subsampling: ChromaSubsampling::S420,
            color_space: EncoderColorSpace::Auto,
            process_id: "test.session.rvl",
            episode_index: 5,
            recording_start_us: frames[0].header.timestamp_us,
            output_width: width,
            output_height: height,
            allow_rescale: false,
        };
        let session = open_session(params, &frames[0]).expect("open rvl session");
        let mut session = session;
        let mut sink = MockSink::new();
        for frame in &frames {
            session.encode(frame, &mut sink).expect("encode rvl frame");
        }
        session.finish(&mut sink).expect("finish rvl session");

        assert!(matches!(sink.calls[0], MockSinkCall::Config { .. }));
        if let MockSinkCall::Config { extradata, header } = &sink.calls[0] {
            assert_eq!(&extradata[0..4], b"RVL1");
            assert_eq!(header.episode_index, 5);
        }
        let mut packet_count = 0usize;
        for call in sink.calls.iter().skip(1) {
            if let MockSinkCall::Packet { header, payload } = call {
                assert!(header.is_keyframe(), "every RVL packet is a keyframe");
                assert!(!payload.is_empty());
                packet_count += 1;
            }
        }
        assert_eq!(packet_count, frames.len());
        assert!(matches!(
            sink.calls.last().unwrap(),
            MockSinkCall::Eos { .. }
        ));
    }

    #[test]
    fn passthrough_session_errors_until_implemented() {
        let mut session: Box<dyn CodecSession> = Box::new(PassthroughCodecSession::new());
        let mut sink = MockSink::new();
        let frame = make_rgb_frame(2, 2, 0);
        let err = session.encode(&frame, &mut sink).expect_err("should error");
        assert!(err.to_string().contains("not yet implemented"));
    }

    /// Phase 1 (Bug B): preview-encoded sessions opened with
    /// `allow_rescale = true` must accept camera-native frames whose
    /// dims differ from the configured output dims and downscale them
    /// internally via swscale. Before the fix, `LibavCodecSession::encode`
    /// returned an error on every frame whose dims did not match.
    #[test]
    fn libav_session_rescales_when_source_dims_exceed_output() {
        if select_encoder_name(EncoderCodec::H264, EncoderBackend::Cpu).is_none() {
            eprintln!("skipping: cpu h264 path unavailable");
            return;
        }
        media::ensure_ffmpeg_initialized().expect("ffmpeg init");

        // Source frames are 64x48 RGB24 (camera-native); session output
        // is 32x24 (preview dims).
        let frames: Vec<_> = (0..4).map(|i| make_rgb_frame(64, 48, i)).collect();
        let params = CodecSessionParams {
            codec: EncoderCodec::H264,
            backend: EncoderBackend::Cpu,
            fps: 30,
            crf: Some(28),
            preset: Some("ultrafast"),
            tune: None,
            bit_depth: 8,
            chroma_subsampling: ChromaSubsampling::S420,
            color_space: EncoderColorSpace::Auto,
            process_id: "test.session.h264.rescale",
            episode_index: 1,
            recording_start_us: frames[0].header.timestamp_us,
            output_width: 32,
            output_height: 24,
            allow_rescale: true,
        };
        let mut session = open_session(params, &frames[0]).expect("open session");
        let mut sink = MockSink::new();
        for frame in &frames {
            session
                .encode(frame, &mut sink)
                .expect("encode rescaled frame");
        }
        session.finish(&mut sink).expect("finish session");

        // Config must carry SPS/PPS sized for the output (32x24), and at
        // least one Packet must reach the sink.
        match &sink.calls[0] {
            MockSinkCall::Config { header, extradata } => {
                assert_eq!(header.width, 32);
                assert_eq!(header.height, 24);
                assert!(!extradata.is_empty(), "h264 extradata must be present");
            }
            other => panic!("first call should be Config, got {other:?}"),
        }
        let packets: Vec<_> = sink
            .calls
            .iter()
            .filter(|c| matches!(c, MockSinkCall::Packet { .. }))
            .collect();
        assert!(
            !packets.is_empty(),
            "preview-encoded session with rescale should emit ≥1 packet, got {} calls",
            sink.calls.len()
        );
        for call in &packets {
            if let MockSinkCall::Packet { header, .. } = call {
                assert_eq!(header.width, 32);
                assert_eq!(header.height, 24);
            }
        }
    }

    /// Phase 1 (Bug B): when source dims change mid-stream, the
    /// session must rebuild its swscale Context with the new source
    /// dims. The cached `(scaler_input_pixel, scaler_source_dims)`
    /// pair is the sentinel — when source dims change, the scaler
    /// must be replaced so the produced AVFrame is still
    /// `(self.width, self.height)`.
    #[test]
    fn libav_session_rebuilds_scaler_on_source_dim_change() {
        if select_encoder_name(EncoderCodec::H264, EncoderBackend::Cpu).is_none() {
            eprintln!("skipping: cpu h264 path unavailable");
            return;
        }
        media::ensure_ffmpeg_initialized().expect("ffmpeg init");

        let frame_a = make_rgb_frame(64, 48, 0);
        let frame_b = make_rgb_frame(48, 36, 1);
        let params = CodecSessionParams {
            codec: EncoderCodec::H264,
            backend: EncoderBackend::Cpu,
            fps: 30,
            crf: Some(28),
            preset: Some("ultrafast"),
            tune: None,
            bit_depth: 8,
            chroma_subsampling: ChromaSubsampling::S420,
            color_space: EncoderColorSpace::Auto,
            process_id: "test.session.h264.dimchange",
            episode_index: 2,
            recording_start_us: frame_a.header.timestamp_us,
            output_width: 32,
            output_height: 24,
            allow_rescale: true,
        };
        let mut session = open_session(params, &frame_a).expect("open session");
        let mut sink = MockSink::new();
        session
            .encode(&frame_a, &mut sink)
            .expect("encode 64x48 frame");
        session
            .encode(&frame_b, &mut sink)
            .expect("encode 48x36 frame");
        session.finish(&mut sink).expect("finish session");

        // Both frames must produce packets sized at the output dims.
        let packet_dims: Vec<(u32, u32)> = sink
            .calls
            .iter()
            .filter_map(|c| match c {
                MockSinkCall::Packet { header, .. } => Some((header.width, header.height)),
                _ => None,
            })
            .collect();
        assert!(
            !packet_dims.is_empty(),
            "expected at least one packet across both frames"
        );
        for (w, h) in &packet_dims {
            assert_eq!((*w, *h), (32, 24));
        }
    }

    /// Phase 1 guard (Bug B does not regress recording): recording
    /// sessions are opened with `allow_rescale = false` (via
    /// `from_recording`), and a mid-stream dim change must still
    /// produce the historical "frame dimensions changed during
    /// recording" error.
    #[test]
    fn recording_session_still_rejects_dim_change() {
        if select_encoder_name(EncoderCodec::H264, EncoderBackend::Cpu).is_none() {
            eprintln!("skipping: cpu h264 path unavailable");
            return;
        }
        media::ensure_ffmpeg_initialized().expect("ffmpeg init");

        let frame_a = make_rgb_frame(64, 48, 0);
        let frame_b = make_rgb_frame(32, 24, 1);
        let params = CodecSessionParams {
            codec: EncoderCodec::H264,
            backend: EncoderBackend::Cpu,
            fps: 30,
            crf: Some(28),
            preset: Some("ultrafast"),
            tune: None,
            bit_depth: 8,
            chroma_subsampling: ChromaSubsampling::S420,
            color_space: EncoderColorSpace::Auto,
            process_id: "test.session.h264.recording",
            episode_index: 3,
            recording_start_us: frame_a.header.timestamp_us,
            // Recording dims = camera-native dims of the first frame.
            output_width: frame_a.header.width,
            output_height: frame_a.header.height,
            allow_rescale: false,
        };
        let mut session = open_session(params, &frame_a).expect("open session");
        let mut sink = MockSink::new();
        session
            .encode(&frame_a, &mut sink)
            .expect("first frame should encode");
        let err = session
            .encode(&frame_b, &mut sink)
            .expect_err("dim change must error in recording mode");
        assert!(
            err.to_string()
                .contains("frame dimensions changed during recording"),
            "expected dim-change error, got: {err}"
        );
    }

    impl MockSinkCall {
        pub fn sequence(&self) -> u64 {
            match self {
                MockSinkCall::Config { header, .. }
                | MockSinkCall::Packet { header, .. }
                | MockSinkCall::Eos { header } => header.sequence_number,
            }
        }
    }
}
