//! Preview-role encoder runtime.
//!
//! Always-on. Subscribes to the per-camera frame topic + the shared
//! `ControlEvent` service (only honours `Shutdown`) + the per-channel
//! `…/preview-control` topic (`PreviewControl::SetSize`).
//!
//! Output mode (jpeg vs encoded) is decided once at startup from
//! [`rollio_types::config::PreviewEncoderConfig`]:
//!
//! * **JPEG mode**: every received frame goes through
//!   [`crate::preview::PreviewBuilder`] (decode/copy → swscale to RGB24
//!   at the configured preview dims), then through
//!   [`crate::preview::JpegCompressor`] (turbojpeg), then is published
//!   on the per-channel `…/preview-jpeg` topic via
//!   [`crate::sink::IpcPreviewJpegSink`]. The visualizer's existing
//!   `CameraFrameHeader`-based plumbing handles the rest.
//!
//! * **Encoded mode**: each frame is fed into a long-lived
//!   [`crate::codec::EncoderSession`] (H.264 by default for color, RVL
//!   for depth) and published on `…/preview-config` +
//!   `…/preview-packets` via [`crate::sink::IpcPreviewPacketSink`]. On
//!   `PreviewControl::SetSize`, the session is closed (with EOS) and
//!   reopened at the new dims so the very next packet is a fresh
//!   keyframe.

use crate::codec::{open_session, CodecSessionParams, EncoderSession, OwnedFrame};
use crate::error::{map_iceoryx_error, EncoderError, Result};
use crate::preview::{JpegCompressor, PreviewBuilder};
use crate::sink::{IpcPreviewJpegSink, IpcPreviewPacketSink};
use iceoryx2::prelude::*;
use rollio_bus::CAMERA_FRAMES_MAX_SUBSCRIBERS;
use rollio_bus::CONTROL_EVENTS_SERVICE;
use rollio_types::config::{EncoderRuntimeConfigV2, PreviewEncoderConfig, PreviewOutputMode};
use rollio_types::messages::{CameraFrameHeader, ControlEvent, PixelFormat, PreviewControl};
use std::time::Duration;

pub fn run(config: EncoderRuntimeConfigV2) -> Result<()> {
    let preview = config
        .preview
        .clone()
        .ok_or_else(|| EncoderError::message("preview-role config missing [preview] block"))?;

    let node = NodeBuilder::new()
        .signal_handling_mode(SignalHandlingMode::Disabled)
        .create::<ipc::Service>()
        .map_err(map_iceoryx_error)?;

    let frame_service_name: ServiceName = config
        .frame_topic
        .as_str()
        .try_into()
        .map_err(map_iceoryx_error)?;
    let frame_service = node
        .service_builder(&frame_service_name)
        .publish_subscribe::<[u8]>()
        .user_header::<CameraFrameHeader>()
        .max_subscribers(CAMERA_FRAMES_MAX_SUBSCRIBERS)
        .open_or_create()
        .map_err(map_iceoryx_error)?;
    let frame_subscriber = frame_service
        .subscriber_builder()
        .create()
        .map_err(map_iceoryx_error)?;

    let control_service_name: ServiceName = CONTROL_EVENTS_SERVICE
        .try_into()
        .map_err(map_iceoryx_error)?;
    let control_service = node
        .service_builder(&control_service_name)
        .publish_subscribe::<ControlEvent>()
        .open_or_create()
        .map_err(map_iceoryx_error)?;
    let control_subscriber = control_service
        .subscriber_builder()
        .create()
        .map_err(map_iceoryx_error)?;

    let preview_control_service_name: ServiceName = preview
        .control_topic
        .as_str()
        .try_into()
        .map_err(map_iceoryx_error)?;
    let preview_control_service = node
        .service_builder(&preview_control_service_name)
        .publish_subscribe::<PreviewControl>()
        .open_or_create()
        .map_err(map_iceoryx_error)?;
    let preview_control_subscriber = preview_control_service
        .subscriber_builder()
        .create()
        .map_err(map_iceoryx_error)?;

    let mut state = match preview.output_mode {
        PreviewOutputMode::Jpeg => PreviewState::open_jpeg(&node, &config, &preview)?,
        PreviewOutputMode::Encoded => PreviewState::open_encoded(&node, &config, &preview)?,
    };

    let mut shutdown = false;
    while !shutdown {
        // Drain control-plane events.
        while let Some(sample) = control_subscriber.receive().map_err(map_iceoryx_error)? {
            if matches!(sample.payload(), ControlEvent::Shutdown) {
                shutdown = true;
            }
        }

        // Drain preview-control events; on a size change, reopen the
        // current preview producer at the new dims.
        let mut new_dims: Option<(u32, u32)> = None;
        while let Some(sample) = preview_control_subscriber
            .receive()
            .map_err(map_iceoryx_error)?
        {
            let PreviewControl::SetSize { width, height } = *sample.payload();
            if width > 0 && height > 0 {
                new_dims = Some((width, height));
            }
        }
        if let Some((w, h)) = new_dims {
            state.resize(&config, &preview, w, h)?;
        }

        // Process whatever frames are queued. Preview is best-effort,
        // so we always take the latest frame per polling pass.
        let mut latest: Option<OwnedFrame> = None;
        while let Some(sample) = frame_subscriber.receive().map_err(map_iceoryx_error)? {
            let owned = OwnedFrame {
                header: *sample.user_header(),
                payload: sample.payload().to_vec(),
            };
            latest = Some(owned);
        }
        if let Some(frame) = latest {
            if let Err(error) = state.handle_frame(&config, &preview, &frame) {
                eprintln!(
                    "rollio-encoder: preview frame failed for process={} channel={}: {error}",
                    config.process_id, config.channel_id
                );
            }
        }

        std::thread::sleep(Duration::from_millis(2));
    }

    state.shutdown();
    Ok(())
}

/// One of the two preview output modes; bundles the active producer
/// pieces (jpeg vs encoded) so the polling loop above is uniform.
enum PreviewState {
    Jpeg {
        builder: PreviewBuilder,
        compressor: JpegCompressor,
        sink: IpcPreviewJpegSink,
    },
    Encoded {
        sink: IpcPreviewPacketSink,
        session: Option<EncoderSession>,
        /// Owned mutable copy of the preview block so `set_preview_size`
        /// can update `width`/`height` and the next `handle_frame`
        /// reopens the session at the new dims.
        preview: PreviewEncoderConfig,
    },
}

impl PreviewState {
    fn open_jpeg(
        node: &Node<ipc::Service>,
        _config: &EncoderRuntimeConfigV2,
        preview: &PreviewEncoderConfig,
    ) -> Result<Self> {
        let topic = preview
            .jpeg_topic
            .as_deref()
            .ok_or_else(|| EncoderError::message("preview jpeg mode requires jpeg_topic"))?;
        let sink = IpcPreviewJpegSink::open(node, topic, 8 * 1024 * 1024)?;
        let builder = PreviewBuilder::new(preview.width, preview.height, preview.fps);
        let compressor = JpegCompressor::new(preview.jpeg_quality)?;
        Ok(Self::Jpeg {
            builder,
            compressor,
            sink,
        })
    }

    fn open_encoded(
        node: &Node<ipc::Service>,
        _config: &EncoderRuntimeConfigV2,
        preview: &PreviewEncoderConfig,
    ) -> Result<Self> {
        let config_topic = preview
            .config_topic
            .as_deref()
            .ok_or_else(|| EncoderError::message("preview encoded mode requires config_topic"))?;
        let packet_topic = preview
            .packet_topic
            .as_deref()
            .ok_or_else(|| EncoderError::message("preview encoded mode requires packet_topic"))?;
        let sink = IpcPreviewPacketSink::open(node, config_topic, packet_topic, 8 * 1024 * 1024)?;
        Ok(Self::Encoded {
            sink,
            session: None,
            preview: preview.clone(),
        })
    }

    fn handle_frame(
        &mut self,
        config: &EncoderRuntimeConfigV2,
        _preview_block: &PreviewEncoderConfig,
        frame: &OwnedFrame,
    ) -> Result<()> {
        match self {
            Self::Jpeg {
                builder,
                compressor,
                sink,
            } => {
                let Some(built) = builder.build(frame)? else {
                    return Ok(());
                };
                let compressed = compressor.compress(
                    &built.rgb,
                    built.width,
                    built.height,
                    built.width,
                    built.height,
                )?;
                use crate::codec::EncodedPacketSink;
                use rollio_types::messages::{
                    EncodedCodecId, EncodedPacketHeader, EncodedPacketKind,
                };
                let header = EncodedPacketHeader {
                    kind: EncodedPacketKind::Packet,
                    codec: EncodedCodecId::Mjpg,
                    flags: rollio_types::messages::ENCODED_PACKET_FLAG_KEYFRAME,
                    width: compressed.width,
                    height: compressed.height,
                    pixel_format: PixelFormat::Mjpeg,
                    _reserved0: 0,
                    time_base_num: 1,
                    time_base_den: 1_000_000,
                    pts_us: 0,
                    dts_us: 0,
                    duration_us: 0,
                    sequence_number: 0,
                    source_timestamp_us: built.timestamp_us,
                    source_frame_index: built.frame_index,
                    episode_index: 0,
                    payload_len: compressed.jpeg_data.len() as u32,
                };
                sink.write_packet(header, compressed.jpeg_data)?;
                Ok(())
            }
            Self::Encoded {
                sink,
                session,
                preview,
            } => {
                if session.is_none() {
                    let codec = if frame.header.pixel_format == PixelFormat::Depth16 {
                        preview.depth_codec
                    } else {
                        preview.color_codec
                    };
                    let params = CodecSessionParams {
                        codec,
                        backend: preview.backend,
                        fps: preview.fps,
                        crf: preview.crf,
                        preset: None,
                        tune: None,
                        bit_depth: 8,
                        chroma_subsampling: rollio_types::config::ChromaSubsampling::S420,
                        color_space: rollio_types::config::EncoderColorSpace::Auto,
                        process_id: &config.process_id,
                        episode_index: 0,
                        recording_start_us: frame.header.timestamp_us,
                        output_width: preview.width,
                        output_height: preview.height,
                    };
                    *session = Some(open_session(params, frame)?);
                }
                if let Some(s) = session.as_mut() {
                    s.encode(frame, sink)?;
                }
                Ok(())
            }
        }
    }

    fn resize(
        &mut self,
        _config: &EncoderRuntimeConfigV2,
        _preview_block: &PreviewEncoderConfig,
        width: u32,
        height: u32,
    ) -> Result<()> {
        match self {
            Self::Jpeg { builder, .. } => {
                builder.set_output_dims(width, height);
                Ok(())
            }
            Self::Encoded {
                sink,
                session,
                preview,
            } => {
                preview.width = width;
                preview.height = height;
                if let Some(s) = session.take() {
                    let _ = s.finish(sink);
                }
                Ok(())
            }
        }
    }

    fn shutdown(self) {
        match self {
            Self::Jpeg { .. } => {}
            Self::Encoded {
                mut sink, session, ..
            } => {
                if let Some(s) = session {
                    let _ = s.finish(&mut sink);
                }
            }
        }
    }
}
