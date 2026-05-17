//! VAAPI color encoder backend — full hardware path.
//!
//! Mirrors the NVIDIA backend's architecture (filter graph
//! [`ScaleGraph`] + `*_vaapi` encoder consuming VAAPI surfaces
//! directly) but with a simpler pipeline shape: VAAPI hardware
//! decoders aren't asynchronous like CUVID, and the Intel iGPU's
//! MJPEG decoder support is uneven across drivers, so we route
//! compressed input through SW decode + `hwupload_vaapi` instead of
//! a per-codec `*_vaapi` decoder. Raw input flows directly into
//! `hwupload_vaapi`.
//!
//! ```text
//! Raw  (RGB/YUYV/Gray8) ─► buffer (CPU) ─► hwupload ─► scale_vaapi ─► h264_vaapi
//! MJPG                  ─► SW mjpeg ─► YUV422P ─► buffer (CPU) ─► hwupload ─► … ─► h264_vaapi
//! ```
//!
//! Phase 1 of the backend trait refactor left this as a thin wrapper
//! around the legacy `LibavCodecSession` (CPU swscale + VAAPI encode);
//! Phase 4 (this) adds the full-HW path. If VAAPI isn't available on
//! the host the open_session falls back to LibavCodecSession so
//! `EncoderBackend::Vaapi` continues to mean "use VAAPI somehow"
//! even on hosts where the filter graph won't initialise.
//!
//! **Verification**: this file is compile-tested on the dev host
//! (NVIDIA only). End-to-end behavior is the next-session task on
//! an Intel iGPU host.

use std::ffi::CString;
use std::ptr;
use std::time::Instant;

use ffmpeg::ffi as f;
use ffmpeg::util::format::pixel::Pixel;
use ffmpeg_next as ffmpeg;
use rollio_types::config::{EncoderBackend, EncoderCodec};
use rollio_types::messages::{EncodedPacketHeader, EncodedPacketKind, PixelFormat};

use super::libav_cpu::{libav_codec_available, with_backend};
use super::{ColorBackendId, ColorCodec, ColorEncoderBackend};
use crate::backend::filter_graph::{HwAccel, InputResidency, ScaleGraph, ScaleGraphConfig};
use crate::codec::{
    encoded_codec_id, CodecSession, CodecSessionParams, EncodedPacketSink, LibavCodecSession,
    OwnedFrame,
};
use crate::error::{EncoderError, Result};
use crate::media::{
    build_codec_options, color_space_metadata, create_hw_device, select_encoder_name, AvBufferRef,
    EncodeMetrics,
};

pub struct LibavVaapiBackend;

impl ColorEncoderBackend for LibavVaapiBackend {
    fn id(&self) -> ColorBackendId {
        ColorBackendId::Vaapi
    }

    fn priority(&self) -> u32 {
        // Tried after Nvidia but before Cpu under `Auto`.
        50
    }

    fn available(&self) -> bool {
        libav_codec_available(ColorCodec::H264, EncoderBackend::Vaapi)
    }

    fn supports(&self, codec: ColorCodec, input: PixelFormat) -> bool {
        if !matches!(
            codec,
            ColorCodec::H264 | ColorCodec::H265 | ColorCodec::Av1 | ColorCodec::Mjpg
        ) {
            return false;
        }
        if !matches!(
            input,
            PixelFormat::Rgb24
                | PixelFormat::Bgr24
                | PixelFormat::Yuyv
                | PixelFormat::Mjpeg
                | PixelFormat::Gray8
        ) {
            return false;
        }
        libav_codec_available(codec, EncoderBackend::Vaapi)
    }

    fn open_session(
        &self,
        params: &CodecSessionParams<'_>,
        first_frame: &OwnedFrame,
    ) -> Result<Box<dyn CodecSession>> {
        let pinned = with_backend(params, EncoderBackend::Vaapi);
        match VaapiHwSession::new(&pinned, first_frame) {
            Ok(session) => Ok(Box::new(session)),
            Err(err) => {
                eprintln!(
                    "rollio-encoder: VAAPI full-HW path init failed ({err}); \
                     falling back to libav swscale + VAAPI encode."
                );
                let legacy = LibavCodecSession::new(&pinned, first_frame)?;
                Ok(Box::new(legacy))
            }
        }
    }
}

/// Which CPU-side input the session is consuming. Compressed inputs
/// are decoded via libavcodec (SW); raw inputs are wrapped into an
/// AVFrame directly. Both paths feed `hwupload_vaapi` downstream.
enum InputStage {
    CpuDecode {
        decoder: ffmpeg::decoder::Video,
    },
    Raw {
        source_pixel: Pixel,
    },
}

struct Pipeline {
    input: InputStage,
    output: OutputStage,
}

struct OutputStage {
    filter: ScaleGraph,
    encoder: ffmpeg::encoder::Video,
    extradata: Vec<u8>,
}

pub(crate) struct VaapiHwSession {
    codec: EncoderCodec,
    width: u32,
    height: u32,
    #[allow(dead_code)]
    process_id: String,
    episode_index: u32,
    recording_start_us: u64,
    fps: u32,
    crf: Option<u8>,
    color_space: rollio_types::config::EncoderColorSpace,
    #[allow(dead_code)]
    vaapi_device: AvBufferRef,
    encoder_time_base: ffmpeg::Rational,
    pipeline: Option<Pipeline>,
    config_sent: bool,
    next_sequence: u64,
    last_pts_us: Option<i64>,
    nonmonotonic_warning_logged: bool,
    metrics: EncodeMetrics,
}

impl VaapiHwSession {
    pub(crate) fn new(
        params: &CodecSessionParams<'_>,
        first_frame: &OwnedFrame,
    ) -> Result<Self> {
        crate::media::ensure_ffmpeg_initialized()?;
        let vaapi_device = create_hw_device(EncoderBackend::Vaapi)?;

        let mut session = Self {
            codec: params.codec,
            width: params.output_width,
            height: params.output_height,
            process_id: params.process_id.to_string(),
            episode_index: params.episode_index,
            recording_start_us: params.recording_start_us,
            fps: params.fps,
            crf: params.crf,
            color_space: params.color_space,
            vaapi_device,
            encoder_time_base: ffmpeg::Rational(1, 1_000_000),
            pipeline: None,
            config_sent: false,
            next_sequence: 0,
            last_pts_us: None,
            nonmonotonic_warning_logged: false,
            metrics: EncodeMetrics::default(),
        };

        let pipeline = match input_strategy(first_frame.header.pixel_format) {
            InputStrategy::CpuDecode(source_codec) => {
                session.build_cpu_decode_pipeline(source_codec, first_frame)?
            }
            InputStrategy::Raw(source_pixel) => {
                session.build_raw_pipeline(source_pixel, first_frame)?
            }
        };
        session.pipeline = Some(pipeline);
        Ok(session)
    }

    fn build_cpu_decode_pipeline(
        &self,
        source_codec: ffmpeg::codec::Id,
        first_frame: &OwnedFrame,
    ) -> Result<Pipeline> {
        let decoder_codec = ffmpeg::decoder::find(source_codec).ok_or_else(|| {
            EncoderError::message(format!(
                "no software decoder registered for libav codec id {:?}",
                source_codec
            ))
        })?;
        let ctx = ffmpeg::codec::context::Context::new_with_codec(decoder_codec);
        let mut decoder = ctx.decoder().video()?;

        // Decode the first packet so we know the pixel format the SW
        // decoder produces (V4L2 MJPG typically lands in YUVJ422P).
        // Same relabel-J-to-non-J trick as the NVIDIA backend so
        // hwupload doesn't refuse the J variant and the filter graph
        // doesn't auto-insert a CPU swscale.
        let packet = ffmpeg::Packet::copy(&first_frame.payload);
        decoder.send_packet(&packet)?;
        let mut decoded = ffmpeg::frame::Video::empty();
        decoder
            .receive_frame(&mut decoded)
            .map_err(|e| EncoderError::message(format!("SW decoder failed on first packet: {e}")))?;
        let decoded_pixel = decoded.format();
        let source_pixel = match decoded_pixel {
            Pixel::YUVJ420P => Pixel::YUV420P,
            Pixel::YUVJ422P => Pixel::YUV422P,
            Pixel::YUVJ444P => Pixel::YUV444P,
            other => other,
        };
        unsafe {
            (*decoded.as_mut_ptr()).color_range = f::AVColorRange::AVCOL_RANGE_JPEG;
            (*decoded.as_mut_ptr()).format = f::AVPixelFormat::from(source_pixel) as i32;
        }

        let filter = ScaleGraph::build(ScaleGraphConfig {
            hw_accel: HwAccel::Vaapi,
            hw_device: &self.vaapi_device,
            residency: InputResidency::Cpu,
            src_width: first_frame.header.width,
            src_height: first_frame.header.height,
            src_pixel: source_pixel,
            src_hw_frames_ctx: None,
            dst_width: self.width,
            dst_height: self.height,
            dst_sw_format: Pixel::NV12,
            time_base: self.encoder_time_base,
        })?;

        let (encoder, extradata) = self.open_encoder(&filter)?;
        let mut output = OutputStage {
            filter,
            encoder,
            extradata,
        };
        decoded.set_pts(Some(0));
        output.filter.send_frame(&mut decoded)?;
        Ok(Pipeline {
            input: InputStage::CpuDecode { decoder },
            output,
        })
    }

    fn build_raw_pipeline(
        &self,
        source_pixel: Pixel,
        first_frame: &OwnedFrame,
    ) -> Result<Pipeline> {
        let filter = ScaleGraph::build(ScaleGraphConfig {
            hw_accel: HwAccel::Vaapi,
            hw_device: &self.vaapi_device,
            residency: InputResidency::Cpu,
            src_width: first_frame.header.width,
            src_height: first_frame.header.height,
            src_pixel: source_pixel,
            src_hw_frames_ctx: None,
            dst_width: self.width,
            dst_height: self.height,
            dst_sw_format: Pixel::NV12,
            time_base: self.encoder_time_base,
        })?;
        let (encoder, extradata) = self.open_encoder(&filter)?;
        Ok(Pipeline {
            input: InputStage::Raw { source_pixel },
            output: OutputStage {
                filter,
                encoder,
                extradata,
            },
        })
    }

    fn open_encoder(
        &self,
        filter: &ScaleGraph,
    ) -> Result<(ffmpeg::encoder::Video, Vec<u8>)> {
        let codec_name = select_encoder_name(self.codec, EncoderBackend::Vaapi).ok_or_else(|| {
            EncoderError::message(format!(
                "no VAAPI encoder available for {}",
                self.codec.as_str()
            ))
        })?;
        let codec = ffmpeg::encoder::find_by_name(codec_name)
            .ok_or_else(|| EncoderError::message(format!("encoder `{codec_name}` not found")))?;
        let fps = ffmpeg::Rational(self.fps as i32, 1);
        let mut encoder = ffmpeg::codec::context::Context::new_with_codec(codec)
            .encoder()
            .video()?;
        encoder.set_width(self.width);
        encoder.set_height(self.height);
        encoder.set_aspect_ratio(ffmpeg::Rational(1, 1));
        #[cfg(feature = "ffmpeg_5_1")]
        {
            encoder.set_format(Pixel::VAAPI);
        }
        #[cfg(not(feature = "ffmpeg_5_1"))]
        {
            encoder.set_format(Pixel::NV12);
        }
        encoder.set_frame_rate(Some(fps));
        encoder.set_time_base(self.encoder_time_base);
        unsafe {
            (*encoder.as_mut_ptr()).color_range = f::AVColorRange::AVCOL_RANGE_MPEG;
        }
        if let Some((primaries, trc, space)) = color_space_metadata(self.color_space) {
            unsafe {
                (*encoder.as_mut_ptr()).color_primaries = primaries;
                (*encoder.as_mut_ptr()).color_trc = trc;
                (*encoder.as_mut_ptr()).colorspace = space;
            }
        }
        encoder.set_max_b_frames(0);
        encoder.set_flags(ffmpeg::codec::Flags::GLOBAL_HEADER);
        unsafe {
            (*encoder.as_mut_ptr()).hw_device_ctx = f::av_buffer_ref(self.vaapi_device.as_ptr());
            (*encoder.as_mut_ptr()).hw_frames_ctx = filter.clone_output_hw_frames_ctx()?;
        }
        let codec_options =
            build_codec_options(codec_name, EncoderBackend::Vaapi, self.crf, None, None);
        let opened = encoder.open_as_with(codec, codec_options).map_err(|err| {
            EncoderError::message(format!(
                "VAAPI open_as_with(`{codec_name}`) failed for {}x{}@{}: {err}",
                self.width, self.height, self.fps,
            ))
        })?;
        let extradata = unsafe {
            let ptr = (*opened.as_ptr()).extradata;
            let len = (*opened.as_ptr()).extradata_size as usize;
            if ptr.is_null() || len == 0 {
                Vec::new()
            } else {
                std::slice::from_raw_parts(ptr, len).to_vec()
            }
        };
        Ok((opened, extradata))
    }

    fn ensure_config_sent(&mut self, sink: &mut dyn EncodedPacketSink) -> Result<()> {
        if self.config_sent {
            return Ok(());
        }
        let extradata: &[u8] = match self.pipeline.as_ref() {
            Some(p) => &p.output.extradata,
            None => return Ok(()),
        };
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
            payload_len: extradata.len() as u32,
        };
        self.next_sequence += 1;
        sink.write_config(header, extradata)?;
        self.config_sent = true;
        Ok(())
    }

    fn push_input_and_collect(
        &mut self,
        frame: &OwnedFrame,
        pts_us: i64,
    ) -> Result<Vec<ffmpeg::frame::Video>> {
        let pipeline = self
            .pipeline
            .as_mut()
            .ok_or_else(|| EncoderError::message("pipeline not initialised"))?;
        let mut out: Vec<ffmpeg::frame::Video> = Vec::new();
        match &mut pipeline.input {
            InputStage::CpuDecode { decoder } => {
                let packet = ffmpeg::Packet::copy(&frame.payload);
                decoder.send_packet(&packet)?;
                loop {
                    let mut decoded = ffmpeg::frame::Video::empty();
                    if decoder.receive_frame(&mut decoded).is_err() {
                        break;
                    }
                    let relabel = match decoded.format() {
                        Pixel::YUVJ420P => Some(Pixel::YUV420P),
                        Pixel::YUVJ422P => Some(Pixel::YUV422P),
                        Pixel::YUVJ444P => Some(Pixel::YUV444P),
                        _ => None,
                    };
                    if let Some(target) = relabel {
                        unsafe {
                            (*decoded.as_mut_ptr()).color_range = f::AVColorRange::AVCOL_RANGE_JPEG;
                            (*decoded.as_mut_ptr()).format =
                                f::AVPixelFormat::from(target) as i32;
                        }
                    }
                    decoded.set_pts(Some(pts_us));
                    out.push(decoded);
                }
            }
            InputStage::Raw { source_pixel } => {
                let mut av = ffmpeg::frame::Video::new(
                    *source_pixel,
                    frame.header.width,
                    frame.header.height,
                );
                crate::media::copy_frame_payload(&mut av, &frame.header, &frame.payload)?;
                av.set_pts(Some(pts_us));
                out.push(av);
            }
        }
        Ok(out)
    }

    fn drain_filter_and_encode(
        &mut self,
        frame: &OwnedFrame,
        sink: &mut dyn EncodedPacketSink,
    ) -> Result<()> {
        let pipeline = self
            .pipeline
            .as_mut()
            .ok_or_else(|| EncoderError::message("pipeline not initialised"))?;
        loop {
            let mut scaled = ffmpeg::frame::Video::empty();
            match pipeline.output.filter.receive_frame(&mut scaled)? {
                Some(()) => {
                    pipeline.output.encoder.send_frame(&scaled)?;
                    drop(scaled);
                }
                None => break,
            }
        }
        drain_encoder_packets(
            &mut pipeline.output.encoder,
            self.codec,
            self.width,
            self.height,
            frame,
            self.episode_index,
            self.encoder_time_base,
            &mut self.next_sequence,
            &mut self.metrics,
            sink,
        )?;
        Ok(())
    }
}

impl CodecSession for VaapiHwSession {
    fn encode(
        &mut self,
        frame: &OwnedFrame,
        sink: &mut dyn EncodedPacketSink,
    ) -> Result<()> {
        let started = Instant::now();
        let pts_us = match crate::media::compute_pts_us(
            frame.header.timestamp_us,
            self.recording_start_us,
            &mut self.last_pts_us,
            &mut self.nonmonotonic_warning_logged,
        ) {
            Some(v) => v,
            None => return Ok(()),
        };
        let decoded = self.push_input_and_collect(frame, pts_us)?;
        for mut d in decoded {
            let pipeline = self
                .pipeline
                .as_mut()
                .ok_or_else(|| EncoderError::message("pipeline not initialised"))?;
            pipeline.output.filter.send_frame(&mut d)?;
        }
        self.ensure_config_sent(sink)?;
        self.drain_filter_and_encode(frame, sink)?;
        self.metrics.encode_time = self.metrics.encode_time.saturating_add(started.elapsed());
        Ok(())
    }

    fn finish(mut self: Box<Self>, sink: &mut dyn EncodedPacketSink) -> Result<()> {
        if let Some(pipeline) = self.pipeline.as_mut() {
            pipeline.output.encoder.send_eof()?;
            let synth = OwnedFrame {
                header: rollio_types::messages::CameraFrameHeader {
                    timestamp_us: self.recording_start_us,
                    width: self.width,
                    height: self.height,
                    pixel_format: PixelFormat::Rgb24,
                    frame_index: 0,
                },
                payload: Vec::new(),
            };
            drain_encoder_packets(
                &mut pipeline.output.encoder,
                self.codec,
                self.width,
                self.height,
                &synth,
                self.episode_index,
                self.encoder_time_base,
                &mut self.next_sequence,
                &mut self.metrics,
                sink,
            )?;
        }
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
            pts_us: 0,
            dts_us: 0,
            duration_us: 0,
            sequence_number: self.next_sequence,
            source_timestamp_us: self.recording_start_us,
            source_frame_index: 0,
            episode_index: self.episode_index,
            payload_len: 0,
        };
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

enum InputStrategy {
    CpuDecode(ffmpeg::codec::Id),
    Raw(Pixel),
}

fn input_strategy(pix: PixelFormat) -> InputStrategy {
    match pix {
        PixelFormat::Mjpeg => InputStrategy::CpuDecode(ffmpeg::codec::Id::MJPEG),
        PixelFormat::Rgb24 => InputStrategy::Raw(Pixel::RGB24),
        PixelFormat::Bgr24 => InputStrategy::Raw(Pixel::BGR24),
        PixelFormat::Yuyv => InputStrategy::Raw(Pixel::YUYV422),
        PixelFormat::Gray8 => InputStrategy::Raw(Pixel::GRAY8),
        // ColorEncoderBackend::supports() rejects H264AnnexB and
        // Depth16 before we reach this match, but cover them as Raw
        // fallbacks so the type system stays exhaustive.
        PixelFormat::H264AnnexB => InputStrategy::Raw(Pixel::GRAY8),
        PixelFormat::Depth16 => InputStrategy::Raw(Pixel::GRAY8),
        PixelFormat::Nv12 => InputStrategy::Raw(Pixel::NV12),
    }
}

#[allow(clippy::too_many_arguments)]
fn drain_encoder_packets(
    encoder: &mut ffmpeg::encoder::Video,
    codec: EncoderCodec,
    width: u32,
    height: u32,
    frame: &OwnedFrame,
    episode_index: u32,
    encoder_time_base: ffmpeg::Rational,
    next_sequence: &mut u64,
    metrics: &mut EncodeMetrics,
    sink: &mut dyn EncodedPacketSink,
) -> Result<()> {
    let mut packet = ffmpeg::Packet::empty();
    while encoder.receive_packet(&mut packet).is_ok() {
        let pts = packet.pts().unwrap_or(0);
        let dts = packet.dts().unwrap_or(pts);
        let duration = packet.duration();
        let mut header = EncodedPacketHeader {
            kind: EncodedPacketKind::Packet,
            codec: encoded_codec_id(codec),
            flags: 0,
            width,
            height,
            pixel_format: frame.header.pixel_format,
            _reserved0: 0,
            time_base_num: encoder_time_base.numerator() as u32,
            time_base_den: encoder_time_base.denominator() as u32,
            pts_us: pts,
            dts_us: dts,
            duration_us: duration,
            sequence_number: *next_sequence,
            source_timestamp_us: frame.header.timestamp_us,
            source_frame_index: frame.header.frame_index,
            episode_index,
            payload_len: packet.size() as u32,
        };
        header.set_keyframe(packet.is_key());
        *next_sequence += 1;
        metrics.encoded_bytes = metrics.encoded_bytes.saturating_add(packet.size());
        sink.write_packet(header, packet.data().unwrap_or(&[]))?;
    }
    Ok(())
}

#[allow(dead_code)]
fn _silence_imports() {
    let _ = CString::new("").unwrap();
    let _ = ptr::null::<u8>();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vaapi_input_strategy_mjpeg_uses_sw_decoder() {
        match input_strategy(PixelFormat::Mjpeg) {
            InputStrategy::CpuDecode(id) => assert_eq!(id, ffmpeg::codec::Id::MJPEG),
            _ => panic!("MJPEG must route via CpuDecode for VAAPI (no mjpeg_vaapi)"),
        }
    }

    #[test]
    fn vaapi_input_strategy_raw_inputs_use_correct_pixel_format() {
        let cases = [
            (PixelFormat::Rgb24, Pixel::RGB24),
            (PixelFormat::Bgr24, Pixel::BGR24),
            (PixelFormat::Yuyv, Pixel::YUYV422),
            (PixelFormat::Gray8, Pixel::GRAY8),
        ];
        for (input, expected) in cases {
            match input_strategy(input) {
                InputStrategy::Raw(pix) => {
                    assert_eq!(pix, expected, "for input {input:?}")
                }
                _ => panic!("{input:?} should route via Raw for VAAPI"),
            }
        }
    }

    #[test]
    fn vaapi_backend_priority_is_50() {
        let b = LibavVaapiBackend;
        assert_eq!(b.priority(), 50);
        assert_eq!(b.id(), ColorBackendId::Vaapi);
    }

    #[test]
    fn vaapi_backend_rejects_rvl_and_h264_annexb_inputs() {
        let b = LibavVaapiBackend;
        // Even if libavcodec advertises h264_vaapi as available,
        // H264AnnexB / Depth16 must not route through the color
        // backend. (RVL isn't in ColorCodec at all so we test inputs.)
        assert!(!b.supports(ColorCodec::H264, PixelFormat::H264AnnexB));
        assert!(!b.supports(ColorCodec::H264, PixelFormat::Depth16));
    }
}
