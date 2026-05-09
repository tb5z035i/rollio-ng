//! iceoryx2-backed implementations of [`crate::codec::EncodedPacketSink`].
//!
//! Three concrete sinks land here:
//!
//! - [`IpcRecordingSink`] publishes recording packets on the per-camera
//!   `…/recording-config` (history=1) and `…/recording-packets` topics
//!   with strict delivery (no overflow, publisher blocks).
//! - [`IpcPreviewPacketSink`] publishes encoded preview packets with
//!   loss-tolerant defaults (drop-latest is acceptable; sequence
//!   numbers + keyframes recover).
//! - [`IpcPreviewJpegSink`] adapts the [`crate::codec::EncodedPacketSink`]
//!   shape onto the existing `CameraFrameHeader` + JPEG bytes plumbing
//!   used by the visualizer's JPEG mode. It only honours `Packet`
//!   writes (jpeg has no codec extradata, no EOS to forward).

use crate::codec::EncodedPacketSink;
use crate::error::{map_iceoryx_error, EncoderError, Result};
use iceoryx2::port::publisher::Publisher;
use iceoryx2::prelude::*;
use rollio_bus::{PREVIEW_PACKET_BUFFER, RECORDING_PACKET_BUFFER, STREAM_CONFIG_HISTORY_SIZE};
use rollio_types::messages::{
    CameraFrameHeader, EncodedPacketHeader, EncodedPacketKind, PixelFormat,
};

const MAX_PUBLISHERS: usize = 16;
const MAX_SUBSCRIBERS: usize = 16;
const MAX_NODES: usize = 16;

type PacketPublisher = Publisher<ipc::Service, [u8], EncodedPacketHeader>;
type CameraPublisher = Publisher<ipc::Service, [u8], CameraFrameHeader>;

// ---------------------------------------------------------------------------
// IpcRecordingSink — strict delivery, two topics (config + packets)
// ---------------------------------------------------------------------------

pub struct IpcRecordingSink {
    config_publisher: PacketPublisher,
    packet_publisher: PacketPublisher,
}

impl IpcRecordingSink {
    pub fn open(
        node: &Node<ipc::Service>,
        config_topic: &str,
        packet_topic: &str,
        max_payload_bytes: usize,
    ) -> Result<Self> {
        let config_service_name: ServiceName =
            config_topic.try_into().map_err(map_iceoryx_error)?;
        let config_service = node
            .service_builder(&config_service_name)
            .publish_subscribe::<[u8]>()
            .user_header::<EncodedPacketHeader>()
            .history_size(STREAM_CONFIG_HISTORY_SIZE)
            .subscriber_max_buffer_size(STREAM_CONFIG_HISTORY_SIZE.max(2))
            .max_publishers(MAX_PUBLISHERS)
            .max_subscribers(MAX_SUBSCRIBERS)
            .max_nodes(MAX_NODES)
            .open_or_create()
            .map_err(map_iceoryx_error)?;
        let config_publisher = config_service
            .publisher_builder()
            .initial_max_slice_len(max_payload_bytes.max(8 * 1024))
            .allocation_strategy(AllocationStrategy::PowerOfTwo)
            .create()
            .map_err(map_iceoryx_error)?;

        let packet_service_name: ServiceName =
            packet_topic.try_into().map_err(map_iceoryx_error)?;
        let packet_service = node
            .service_builder(&packet_service_name)
            .publish_subscribe::<[u8]>()
            .user_header::<EncodedPacketHeader>()
            .enable_safe_overflow(false)
            .subscriber_max_buffer_size(RECORDING_PACKET_BUFFER)
            .max_publishers(MAX_PUBLISHERS)
            .max_subscribers(MAX_SUBSCRIBERS)
            .max_nodes(MAX_NODES)
            .open_or_create()
            .map_err(map_iceoryx_error)?;
        let packet_publisher = packet_service
            .publisher_builder()
            .initial_max_slice_len(max_payload_bytes.max(64 * 1024))
            .allocation_strategy(AllocationStrategy::PowerOfTwo)
            .unable_to_deliver_strategy(UnableToDeliverStrategy::Block)
            .create()
            .map_err(map_iceoryx_error)?;
        Ok(Self {
            config_publisher,
            packet_publisher,
        })
    }
}

impl EncodedPacketSink for IpcRecordingSink {
    fn write_config(&mut self, header: EncodedPacketHeader, extradata: &[u8]) -> Result<()> {
        publish_packet(&self.config_publisher, header, extradata)
    }

    fn write_packet(&mut self, header: EncodedPacketHeader, payload: &[u8]) -> Result<()> {
        publish_packet(&self.packet_publisher, header, payload)
    }

    fn write_eos(&mut self, header: EncodedPacketHeader) -> Result<()> {
        publish_packet(&self.packet_publisher, header, &[])
    }
}

// ---------------------------------------------------------------------------
// IpcPreviewPacketSink — best-effort delivery, two topics (config + packets)
// ---------------------------------------------------------------------------

pub struct IpcPreviewPacketSink {
    config_publisher: PacketPublisher,
    packet_publisher: PacketPublisher,
}

impl IpcPreviewPacketSink {
    pub fn open(
        node: &Node<ipc::Service>,
        config_topic: &str,
        packet_topic: &str,
        max_payload_bytes: usize,
    ) -> Result<Self> {
        let config_service_name: ServiceName =
            config_topic.try_into().map_err(map_iceoryx_error)?;
        let config_service = node
            .service_builder(&config_service_name)
            .publish_subscribe::<[u8]>()
            .user_header::<EncodedPacketHeader>()
            .history_size(STREAM_CONFIG_HISTORY_SIZE)
            .subscriber_max_buffer_size(STREAM_CONFIG_HISTORY_SIZE.max(2))
            .max_publishers(MAX_PUBLISHERS)
            .max_subscribers(MAX_SUBSCRIBERS)
            .max_nodes(MAX_NODES)
            .open_or_create()
            .map_err(map_iceoryx_error)?;
        let config_publisher = config_service
            .publisher_builder()
            .initial_max_slice_len(max_payload_bytes.max(8 * 1024))
            .allocation_strategy(AllocationStrategy::PowerOfTwo)
            .create()
            .map_err(map_iceoryx_error)?;

        let packet_service_name: ServiceName =
            packet_topic.try_into().map_err(map_iceoryx_error)?;
        let packet_service = node
            .service_builder(&packet_service_name)
            .publish_subscribe::<[u8]>()
            .user_header::<EncodedPacketHeader>()
            .subscriber_max_buffer_size(PREVIEW_PACKET_BUFFER)
            .max_publishers(MAX_PUBLISHERS)
            .max_subscribers(MAX_SUBSCRIBERS)
            .max_nodes(MAX_NODES)
            .open_or_create()
            .map_err(map_iceoryx_error)?;
        let packet_publisher = packet_service
            .publisher_builder()
            .initial_max_slice_len(max_payload_bytes.max(64 * 1024))
            .allocation_strategy(AllocationStrategy::PowerOfTwo)
            .create()
            .map_err(map_iceoryx_error)?;
        Ok(Self {
            config_publisher,
            packet_publisher,
        })
    }
}

impl EncodedPacketSink for IpcPreviewPacketSink {
    fn write_config(&mut self, header: EncodedPacketHeader, extradata: &[u8]) -> Result<()> {
        publish_packet(&self.config_publisher, header, extradata)
    }

    fn write_packet(&mut self, header: EncodedPacketHeader, payload: &[u8]) -> Result<()> {
        publish_packet(&self.packet_publisher, header, payload)
    }

    fn write_eos(&mut self, header: EncodedPacketHeader) -> Result<()> {
        publish_packet(&self.packet_publisher, header, &[])
    }
}

// ---------------------------------------------------------------------------
// IpcPreviewJpegSink — adapts EncodedPacketSink onto a CameraFrameHeader topic
// ---------------------------------------------------------------------------

pub struct IpcPreviewJpegSink {
    publisher: CameraPublisher,
}

impl IpcPreviewJpegSink {
    pub fn open(node: &Node<ipc::Service>, topic: &str, max_payload_bytes: usize) -> Result<Self> {
        let service_name: ServiceName = topic.try_into().map_err(map_iceoryx_error)?;
        let service = node
            .service_builder(&service_name)
            .publish_subscribe::<[u8]>()
            .user_header::<CameraFrameHeader>()
            .subscriber_max_buffer_size(PREVIEW_PACKET_BUFFER)
            .max_publishers(MAX_PUBLISHERS)
            .max_subscribers(MAX_SUBSCRIBERS)
            .max_nodes(MAX_NODES)
            .open_or_create()
            .map_err(map_iceoryx_error)?;
        let publisher = service
            .publisher_builder()
            .initial_max_slice_len(max_payload_bytes.max(64 * 1024))
            .allocation_strategy(AllocationStrategy::PowerOfTwo)
            .create()
            .map_err(map_iceoryx_error)?;
        Ok(Self { publisher })
    }
}

impl EncodedPacketSink for IpcPreviewJpegSink {
    /// The JPEG sink has no codec extradata to publish — JPEG is
    /// self-contained per frame.
    fn write_config(&mut self, _header: EncodedPacketHeader, _extradata: &[u8]) -> Result<()> {
        Ok(())
    }

    /// Adapt the encoded packet header to a `CameraFrameHeader` and
    /// publish the JPEG bytes verbatim. The visualizer's JPEG
    /// pipeline already expects this header shape, so no other code
    /// has to change for jpeg-mode preview to work end-to-end.
    fn write_packet(&mut self, header: EncodedPacketHeader, payload: &[u8]) -> Result<()> {
        // `Config` packets from the JPEG path arrive only because of
        // the trait shape; the actual codec session that drives this
        // sink (PreviewBuilder + JpegCompressor in
        // crate::runtime::preview) only emits `Packet` kinds.
        if !matches!(header.kind, EncodedPacketKind::Packet) {
            return Ok(());
        }
        let cam_header = CameraFrameHeader {
            timestamp_us: header.source_timestamp_us,
            width: header.width,
            height: header.height,
            pixel_format: PixelFormat::Mjpeg,
            frame_index: header.source_frame_index,
        };
        let mut sample = self
            .publisher
            .loan_slice_uninit(payload.len())
            .map_err(map_iceoryx_error)?;
        *sample.user_header_mut() = cam_header;
        let sample = sample.write_from_slice(payload);
        sample.send().map_err(map_iceoryx_error)?;
        Ok(())
    }

    fn write_eos(&mut self, _header: EncodedPacketHeader) -> Result<()> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// shared publish helper
// ---------------------------------------------------------------------------

fn publish_packet(
    publisher: &PacketPublisher,
    header: EncodedPacketHeader,
    payload: &[u8],
) -> Result<()> {
    let mut sample = publisher
        .loan_slice_uninit(payload.len())
        .map_err(map_iceoryx_error)?;
    *sample.user_header_mut() = header;
    let sample = sample.write_from_slice(payload);
    sample.send().map_err(map_iceoryx_error)?;
    Ok(())
}

// EncoderError import for the unused-warning cleanup.
#[allow(dead_code)]
const _: fn() = || {
    let _: fn(EncoderError) = |_| {};
};
