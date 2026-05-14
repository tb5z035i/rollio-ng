//! NVIDIA color encoder backend — full hardware-accelerated path.
//!
//! Pipeline:
//!
//! ```text
//! Compressed bytes (MJPG, H.264) ─► *_cuvid decoder ─► CUDA AVFrame
//!                                                            │
//!                                                            ▼
//!                                       buffer (CUDA) ─► scale_cuda ─► buffersink
//!                                                                          │
//!                                                                          ▼
//! Raw frame  (RGB/YUYV/Gray8)  ─► buffer (CPU) ─► hwupload ─► scale_cuda ─► …
//!                                                                          │
//!                                                                          ▼
//!                                                                    h264_nvenc / hevc_nvenc /
//!                                                                    av1_nvenc with
//!                                                                    hw_frames_ctx wired to
//!                                                                    the filter sink → bytes
//! ```
//!
//! Frames stay in VRAM from the decoder (or hwupload) through to the
//! encoder; the CPU only handles the iceoryx2 payload, the eventual
//! H.264 packet, and the libav API glue.
//!
//! Lazy initialisation: the encoder context's `hw_frames_ctx` has to
//! come from the filter graph's sink, which in turn requires the
//! source's `hw_frames_ctx` (Cuvid case) which only exists after the
//! first packet has been decoded. So the session opens the decoder
//! eagerly, defers filter-graph + encoder construction until the
//! first decoded frame is in hand, and proceeds normally from there.

use std::ffi::CString;
use std::ptr;
use std::time::Instant;

use ffmpeg_next as ffmpeg;
use ffmpeg::ffi as f;
use ffmpeg::util::format::pixel::Pixel;
use rollio_types::config::{EncoderBackend, EncoderCodec};
use rollio_types::messages::{
    EncodedPacketHeader, EncodedPacketKind, PixelFormat,
};

use super::libav_cpu::{libav_codec_available, with_backend};
use super::{ColorBackendId, ColorCodec, ColorEncoderBackend};
use crate::backend::bsf::Mjpeg2JpegBsf;
use crate::backend::filter_graph::{InputResidency, ScaleGraph, ScaleGraphConfig};
use crate::codec::{
    encoded_codec_id, CodecSession, CodecSessionParams, EncodedPacketSink, LibavCodecSession,
    OwnedFrame,
};
use crate::error::{EncoderError, Result};
use crate::media::{
    build_codec_options, color_space_metadata, create_hw_device, select_encoder_name,
    AvBufferRef, EncodeMetrics,
};

pub struct LibavNvidiaBackend;

impl ColorEncoderBackend for LibavNvidiaBackend {
    fn id(&self) -> ColorBackendId {
        ColorBackendId::Nvidia
    }

    fn priority(&self) -> u32 {
        100
    }

    fn available(&self) -> bool {
        libav_codec_available(ColorCodec::H264, EncoderBackend::Nvidia)
    }

    fn supports(&self, codec: ColorCodec, input: PixelFormat) -> bool {
        if !matches!(
            codec,
            ColorCodec::H264 | ColorCodec::H265 | ColorCodec::Av1 | ColorCodec::Mjpg
        ) {
            return false;
        }
        // We accept the same input set as the CPU backend. Cuvid handles
        // MJPG/H264AnnexB; hwupload handles raw RGB/YUYV/Gray8/BGR.
        if !matches!(
            input,
            PixelFormat::Rgb24
                | PixelFormat::Bgr24
                | PixelFormat::Yuyv
                | PixelFormat::Mjpeg
                | PixelFormat::Gray8
                | PixelFormat::H264AnnexB
        ) {
            return false;
        }
        libav_codec_available(codec, EncoderBackend::Nvidia)
    }

    fn open_session(
        &self,
        params: &CodecSessionParams<'_>,
        first_frame: &OwnedFrame,
    ) -> Result<Box<dyn CodecSession>> {
        let pinned = with_backend(params, EncoderBackend::Nvidia);
        // Try the full HW pipeline first. If anything along the
        // CUDA / filter-graph / NVENC path fails (older driver, no
        // scale_cuda in libavfilter, etc.) we fall back to the
        // existing LibavCodecSession path that does CPU swscale +
        // NVENC. This preserves correctness on hosts where the HW
        // pipeline can't initialise for any reason.
        match NvidiaCudaSession::new(&pinned, first_frame) {
            Ok(session) => Ok(Box::new(session)),
            Err(err) => {
                eprintln!(
                    "rollio-encoder: NVIDIA full-HW path init failed ({err}); \
                     falling back to libav swscale + NVENC."
                );
                let legacy = LibavCodecSession::new(&pinned, first_frame)?;
                Ok(Box::new(legacy))
            }
        }
    }
}

/// Whether the encoder receives frames from a Cuvid decoder
/// (compressed input) or directly via hwupload (raw input).
enum InputStage {
    Cuvid {
        decoder: ffmpeg::decoder::Video,
        /// Bitstream filter applied to each packet *before* it
        /// reaches `mjpeg_cuvid`. Used to inject a standard JFIF DHT
        /// segment into V4L2 / UVC MJPG payloads (which omit it to
        /// save bandwidth) so the cuvid path will accept them. None
        /// for H.264 / HEVC / AV1, where the decoder consumes Annex B
        /// directly.
        bsf: Option<Mjpeg2JpegBsf>,
    },
    /// CPU-side libav decoder feeding decoded AVFrames into the
    /// filter graph (which then `hwupload`s to CUDA + `scale_cuda`).
    /// Fallback for MJPG variants the cuvid + `mjpeg2jpeg` chain
    /// still can't ingest (e.g. cameras with damaged SOF markers).
    CpuDecode {
        decoder: ffmpeg::decoder::Video,
    },
    Raw {
        source_pixel: Pixel,
    },
}

/// Per-stream NVIDIA HW pipeline. `output` is split off because for
/// the Cuvid path it depends on `hw_frames_ctx` extracted from the
/// first *decoded* frame, and the cuvid family has a ~3-packet
/// pipeline depth (the parser is asynchronous). So the input stage
/// runs until the first frame falls out, and only then can we build
/// the filter graph + NVENC encoder. For the Raw and CpuDecode paths
/// the output stage is built eagerly in `NvidiaCudaSession::new`.
struct Pipeline {
    input: InputStage,
    output: Option<OutputStage>,
}

struct OutputStage {
    filter: ScaleGraph,
    encoder: ffmpeg::encoder::Video,
    extradata: Vec<u8>,
}

pub(crate) struct NvidiaCudaSession {
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
    /// Owned CUDA device. Lives for the session lifetime; cloned for
    /// the encoder, the decoder (if Cuvid), and the filter graph
    /// (hwupload, raw path).
    cuda_device: AvBufferRef,
    encoder_time_base: ffmpeg::Rational,
    pipeline: Option<Pipeline>,
    config_sent: bool,
    next_sequence: u64,
    last_pts_us: Option<i64>,
    nonmonotonic_warning_logged: bool,
    metrics: EncodeMetrics,
}

impl NvidiaCudaSession {
    pub(crate) fn new(
        params: &CodecSessionParams<'_>,
        first_frame: &OwnedFrame,
    ) -> Result<Self> {
        crate::media::ensure_ffmpeg_initialized()?;

        // Eagerly create the CUDA device so a host without CUDA
        // surfaces (driver missing, no /dev/nvidiactl, …) fails fast
        // with a clear error rather than progressing to NVENC and
        // erroring there.
        let cuda_device = create_hw_device(EncoderBackend::Nvidia)?;

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
            cuda_device,
            encoder_time_base: ffmpeg::Rational(1, 1_000_000),
            pipeline: None,
            config_sent: false,
            next_sequence: 0,
            last_pts_us: None,
            nonmonotonic_warning_logged: false,
            metrics: EncodeMetrics::default(),
        };

        // Build the pipeline eagerly. Any failure (cuvid can't decode
        // the source, filter graph init fails, NVENC open fails)
        // surfaces here so `LibavNvidiaBackend::open_session` can fall
        // back to the legacy CPU-swscale+NVENC path cleanly. Without
        // this, the failure would fire on every subsequent `encode()`
        // and the runtime would burn CPU re-opening the decoder.
        //
        // For MJPG specifically we try the cuvid+`mjpeg2jpeg` chain
        // first (full GPU decode → scale → NVENC). If `mjpeg_cuvid`
        // refuses the bitstream even after the BSF injects DHT
        // tables, we fall back to SW MJPEG decode + hwupload — still
        // GPU for scale/encode, just CPU for the JPEG entropy
        // decode. That's a much better fallback than going all the
        // way to legacy CPU swscale.
        let pipeline = match input_strategy(first_frame.header.pixel_format) {
            InputStrategy::Cuvid(source_codec) => {
                session.build_cuvid_pipeline(source_codec, first_frame, None)?
            }
            InputStrategy::CuvidWithMjpegBsf(source_codec) => {
                // Try cuvid + mjpeg2jpeg first (full GPU MJPEG
                // pipeline). If mjpeg_cuvid still rejects the
                // bitstream (e.g. damaged SOF), the error propagates
                // up to `LibavNvidiaBackend::open_session` which
                // falls back to the legacy CPU-swscale + NVENC
                // path. The output stage is built lazily on the
                // first decoded frame (cuvid is async with a ~3
                // packet pipeline depth), so failures from the
                // decoder show up during `encode()` rather than
                // here.
                let bsf = Mjpeg2JpegBsf::new()?;
                session.build_cuvid_pipeline(source_codec, first_frame, Some(bsf))?
            }
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

    fn build_cuvid_pipeline(
        &self,
        source_codec: ffmpeg::codec::Id,
        first_frame: &OwnedFrame,
        bsf: Option<Mjpeg2JpegBsf>,
    ) -> Result<Pipeline> {
        let decoder_name = cuvid_decoder_name(source_codec)?;
        let decoder_filter = ffmpeg::decoder::find_by_name(decoder_name).ok_or_else(|| {
            EncoderError::message(format!(
                "decoder `{decoder_name}` not registered in libavcodec"
            ))
        })?;
        let mut ctx = ffmpeg::codec::context::Context::new_with_codec(decoder_filter);
        unsafe {
            (*ctx.as_mut_ptr()).hw_device_ctx = f::av_buffer_ref(self.cuda_device.as_ptr());
            // ffmpeg-CLI fills width/height from the demuxer's stream
            // parameters before avcodec_open2; we set them from the
            // camera frame header. mjpeg_cuvid uses them to size its
            // CUDA surface pool.
            (*ctx.as_mut_ptr()).width = first_frame.header.width as i32;
            (*ctx.as_mut_ptr()).height = first_frame.header.height as i32;
        }
        let decoder = ctx.decoder().video()?;

        // CUVID is asynchronous. cuvidParseVideoData queues each
        // packet and the decoded surface only appears after the
        // parser has enough lookahead (empirically ~3 packets for
        // mjpeg_cuvid). We therefore cannot extract `hw_frames_ctx`
        // synchronously here from a first decoded frame. Instead we
        // defer the filter graph + NVENC construction until the first
        // decoded frame actually emerges inside the `encode()` loop.
        // See `Pipeline::output` + `build_cuda_output_from_frame`.
        let _ = source_codec;
        Ok(Pipeline {
            input: InputStage::Cuvid { decoder, bsf },
            output: None,
        })
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

        // Decode the first frame so we know what pixel format the
        // decoder produces (MJPEG typically lands in YUVJ422P for
        // V4L2 cams). We then relabel J-range pixel formats to their
        // MPEG-range equivalents *while stamping `color_range =
        // AVCOL_RANGE_JPEG` on the frame*. The bytes are identical;
        // the relabel just stops `hwupload_cuda` from refusing to
        // touch the J variant and the filter graph auto-inserting a
        // CPU swscale conversion — which would erase the whole point
        // of moving to the HW path.
        let packet = ffmpeg::Packet::copy(&first_frame.payload);
        decoder.send_packet(&packet)?;
        let mut decoded = ffmpeg::frame::Video::empty();
        decoder.receive_frame(&mut decoded).map_err(|e| {
            EncoderError::message(format!(
                "SW decoder failed on first packet: {e}"
            ))
        })?;
        let decoded_pixel = decoded.format();
        let source_pixel = match decoded_pixel {
            Pixel::YUVJ420P => Pixel::YUV420P,
            Pixel::YUVJ422P => Pixel::YUV422P,
            Pixel::YUVJ444P => Pixel::YUV444P,
            other => other,
        };
        // Stamp the range hint on the frame so the GPU stages
        // downstream still know to interpret bytes as full-range.
        // scale_cuda then maps 0–255 → 16–235 NV12 correctly during
        // the YUV → NV12 conversion.
        unsafe {
            (*decoded.as_mut_ptr()).color_range = f::AVColorRange::AVCOL_RANGE_JPEG;
            (*decoded.as_mut_ptr()).format =
                f::AVPixelFormat::from(source_pixel) as i32;
        }

        let filter = ScaleGraph::build(ScaleGraphConfig {
            hw_device: &self.cuda_device,
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

        let mut output = OutputStage { filter, encoder, extradata };
        // Seed the filter with the first decoded frame; PTS gets
        // (re)set inside drain_filter_and_encode based on the
        // recording-start anchor.
        decoded.set_pts(Some(0));
        output.filter.send_frame(&mut decoded)?;
        Ok(Pipeline {
            input: InputStage::CpuDecode { decoder },
            output: Some(output),
        })
    }

    fn build_raw_pipeline(
        &self,
        source_pixel: Pixel,
        first_frame: &OwnedFrame,
    ) -> Result<Pipeline> {
        let filter = ScaleGraph::build(ScaleGraphConfig {
            hw_device: &self.cuda_device,
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
            output: Some(OutputStage { filter, encoder, extradata }),
        })
    }

    fn open_encoder(
        &self,
        filter: &ScaleGraph,
    ) -> Result<(ffmpeg::encoder::Video, Vec<u8>)> {
        let codec_name = select_encoder_name(self.codec, EncoderBackend::Nvidia).ok_or_else(
            || {
                EncoderError::message(format!(
                    "no NVENC encoder available for {}",
                    self.codec.as_str()
                ))
            },
        )?;
        let codec = ffmpeg::encoder::find_by_name(codec_name).ok_or_else(|| {
            EncoderError::message(format!("encoder `{codec_name}` not found"))
        })?;
        let fps = ffmpeg::Rational(self.fps as i32, 1);
        let mut encoder = ffmpeg::codec::context::Context::new_with_codec(codec)
            .encoder()
            .video()?;
        encoder.set_width(self.width);
        encoder.set_height(self.height);
        encoder.set_aspect_ratio(ffmpeg::Rational(1, 1));
        // NVENC consumes CUDA AVFrames directly when hw_frames_ctx is
        // wired up; the encoder format is the hardware-side format.
        encoder.set_format(Pixel::CUDA);
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

        // Wire the CUDA hw_device_ctx (NVENC validates this) AND
        // hw_frames_ctx from the filter sink (the actual GPU surface
        // pool the encoder reads from).
        unsafe {
            (*encoder.as_mut_ptr()).hw_device_ctx = f::av_buffer_ref(self.cuda_device.as_ptr());
            (*encoder.as_mut_ptr()).hw_frames_ctx = filter.clone_output_hw_frames_ctx()?;
        }

        let codec_options = build_codec_options(
            codec_name,
            EncoderBackend::Nvidia,
            self.crf,
            None,
            None,
        );
        let opened = encoder.open_as_with(codec, codec_options).map_err(|err| {
            EncoderError::message(format!(
                "NVENC open_as_with(`{codec_name}`) failed for {}x{}@{}: {err}",
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
        // The output stage (which owns extradata) is built lazily on
        // the first decoded frame for the Cuvid path. Until then we
        // have no SPS/PPS to publish, so skip; encode() retries on
        // every call.
        let extradata: &[u8] = match self.pipeline.as_ref().and_then(|p| p.output.as_ref()) {
            Some(o) => &o.extradata,
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

    fn drain_filter_and_encode(
        &mut self,
        frame: &OwnedFrame,
        sink: &mut dyn EncodedPacketSink,
    ) -> Result<()> {
        let pipeline = self
            .pipeline
            .as_mut()
            .ok_or_else(|| EncoderError::message("pipeline not initialised"))?;
        let output = match pipeline.output.as_mut() {
            Some(o) => o,
            None => return Ok(()), // Cuvid warmup; nothing to drain yet.
        };
        // Pull every frame the filter has ready; for each, send to
        // encoder; then drain encoder packets.
        loop {
            let mut scaled = ffmpeg::frame::Video::empty();
            match output.filter.receive_frame(&mut scaled)? {
                Some(()) => {
                    output.encoder.send_frame(&scaled)?;
                    drop(scaled);
                }
                None => break,
            }
        }
        drain_encoder_packets(
            &mut output.encoder,
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

    /// Build the output stage (filter graph + NVENC) using the
    /// `hw_frames_ctx` from a freshly-decoded CUDA frame. Called the
    /// first time the cuvid path produces a frame.
    fn build_cuvid_output_for_frame(
        &self,
        decoded: &ffmpeg::frame::Video,
    ) -> Result<OutputStage> {
        let hw_frames_ctx = unsafe { (*decoded.as_ptr()).hw_frames_ctx };
        if hw_frames_ctx.is_null() {
            return Err(EncoderError::message(
                "first decoded cuvid frame has no hw_frames_ctx; cannot build NVENC pipeline",
            ));
        }
        let src_width = decoded.width();
        let src_height = decoded.height();
        let filter = ScaleGraph::build(ScaleGraphConfig {
            hw_device: &self.cuda_device,
            residency: InputResidency::Cuda,
            src_width,
            src_height,
            src_pixel: Pixel::CUDA,
            src_hw_frames_ctx: Some(hw_frames_ctx),
            dst_width: self.width,
            dst_height: self.height,
            dst_sw_format: Pixel::NV12,
            time_base: self.encoder_time_base,
        })?;
        let (encoder, extradata) = self.open_encoder(&filter)?;
        Ok(OutputStage { filter, encoder, extradata })
    }

    /// Push the camera packet/frame into the decoder (Cuvid /
    /// CpuDecode) or build the AVFrame directly (Raw), and collect
    /// any decoded frames that come out. For the Cuvid path this
    /// may be empty for the first ~3 calls because CUVID is async
    /// with a small packet pipeline depth.
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
            InputStage::Cuvid { decoder, bsf } => {
                let mut packet = ffmpeg::Packet::copy(&frame.payload);
                if let Some(bsf) = bsf.as_mut() {
                    bsf.filter(&mut packet)?;
                }
                decoder.send_packet(&packet)?;
                loop {
                    let mut decoded = ffmpeg::frame::Video::empty();
                    if decoder.receive_frame(&mut decoded).is_err() {
                        break;
                    }
                    decoded.set_pts(Some(pts_us));
                    out.push(decoded);
                }
            }
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
}

impl CodecSession for NvidiaCudaSession {
    fn encode(
        &mut self,
        frame: &OwnedFrame,
        sink: &mut dyn EncodedPacketSink,
    ) -> Result<()> {
        // Phase 1: push input into the decoder/upload stage and
        // collect any decoded frames that fall out. For the Cuvid
        // path this may be empty for the first ~3 calls (CUVID is
        // asynchronous with a few packets of pipeline depth); for
        // Raw and CpuDecode every call produces exactly one frame.
        let started = Instant::now();
        let pts_us = match crate::media::compute_pts_us(
            frame.header.timestamp_us,
            self.recording_start_us,
            &mut self.last_pts_us,
            &mut self.nonmonotonic_warning_logged,
        ) {
            Some(v) => v,
            None => return Ok(()), // dropped by monotonicity check
        };
        let decoded = self.push_input_and_collect(frame, pts_us)?;

        // Phase 2: for each decoded frame, build the output stage
        // (filter + NVENC) lazily if not yet built (Cuvid path),
        // then push the frame through the filter graph.
        for mut decoded in decoded {
            if self.pipeline.as_ref().and_then(|p| p.output.as_ref()).is_none() {
                let output = self.build_cuvid_output_for_frame(&decoded)?;
                self.pipeline.as_mut().expect("pipeline ready").output = Some(output);
            }
            let output = self
                .pipeline
                .as_mut()
                .and_then(|p| p.output.as_mut())
                .expect("output built");
            output.filter.send_frame(&mut decoded)?;
        }

        self.ensure_config_sent(sink)?;

        // Phase 3: drain filter → encoder → sink.
        self.drain_filter_and_encode(frame, sink)?;
        self.metrics.encode_time =
            self.metrics.encode_time.saturating_add(started.elapsed());
        Ok(())
    }

    fn finish(
        mut self: Box<Self>,
        sink: &mut dyn EncodedPacketSink,
    ) -> Result<()> {
        if let Some(output) = self
            .pipeline
            .as_mut()
            .and_then(|p| p.output.as_mut())
        {
            // Flush filter + encoder. (If the output stage was never
            // built — Cuvid path that hit fewer than ~3 frames before
            // shutdown — there's nothing to drain.)
            output.encoder.send_eof()?;
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
                &mut output.encoder,
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
    /// Decode on the GPU via one of the `*_cuvid` decoders. Used for
    /// H.264 / HEVC / AV1 — well-formed elementary streams that the
    /// cuvid family handles directly.
    Cuvid(ffmpeg::codec::Id),
    /// Decode on the GPU via `mjpeg_cuvid` with the `mjpeg2jpeg`
    /// bitstream filter on the input side. Used for MJPG because
    /// V4L2 / UVC payloads omit standard JFIF DHT segments; the BSF
    /// prepends them so NVDEC will accept the stream. Falls back to
    /// `CpuDecode` if the cuvid path still rejects the bitstream.
    CuvidWithMjpegBsf(ffmpeg::codec::Id),
    /// Decode on the CPU via libav's software decoder, then hwupload
    /// the resulting AVFrame to CUDA inside the filter graph.
    /// Reserved for the inner fallback inside `CuvidWithMjpegBsf` —
    /// `input_strategy()` never returns this variant directly today,
    /// but keeping it surfaces the intent of "CPU decode is an
    /// available routing target" rather than burying it in pipeline
    /// glue.
    #[allow(dead_code)]
    CpuDecode(ffmpeg::codec::Id),
    /// Raw camera bytes, wrapped directly into an AVFrame and
    /// hwuploaded to CUDA by the filter graph. No decoder.
    Raw(Pixel),
}

fn input_strategy(pix: PixelFormat) -> InputStrategy {
    match pix {
        PixelFormat::Mjpeg => InputStrategy::CuvidWithMjpegBsf(ffmpeg::codec::Id::MJPEG),
        PixelFormat::H264AnnexB => InputStrategy::Cuvid(ffmpeg::codec::Id::H264),
        PixelFormat::Rgb24 => InputStrategy::Raw(Pixel::RGB24),
        PixelFormat::Bgr24 => InputStrategy::Raw(Pixel::BGR24),
        PixelFormat::Yuyv => InputStrategy::Raw(Pixel::YUYV422),
        PixelFormat::Gray8 => InputStrategy::Raw(Pixel::GRAY8),
        // Depth16 isn't supported by the color backend; the depth
        // registry handles it. ColorEncoderBackend::supports() rejects
        // it before we get here.
        PixelFormat::Depth16 => InputStrategy::Raw(Pixel::GRAY8),
    }
}

fn cuvid_decoder_name(source: ffmpeg::codec::Id) -> Result<&'static str> {
    Ok(match source {
        ffmpeg::codec::Id::MJPEG => "mjpeg_cuvid",
        ffmpeg::codec::Id::H264 => "h264_cuvid",
        ffmpeg::codec::Id::HEVC => "hevc_cuvid",
        ffmpeg::codec::Id::AV1 => "av1_cuvid",
        other => {
            return Err(EncoderError::message(format!(
                "no Cuvid decoder mapped for libav codec id {:?}",
                other
            )))
        }
    })
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

// Silence import warnings until later phases / tests reference these.
#[allow(dead_code)]
fn _silence() {
    let _ = CString::new("").unwrap();
    let _ = ptr::null::<u8>();
}
