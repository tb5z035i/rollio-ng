//! MCAP file writer module.
//!
//! Wraps the `mcap` crate's `Writer` with schema/channel registration
//! and a convenient message-writing interface for the episode assembler.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use mcap::records::MessageHeader;
use mcap::write::Metadata;
use mcap::Writer;

/// Schema encoding identifier for FlatBuffers in MCAP.
const SCHEMA_ENCODING: &str = "flatbuffer";

/// Message encoding identifier for FlatBuffers in MCAP.
const MESSAGE_ENCODING: &str = "flatbuffer";

// Schemas are compiled into the binary so deployments don't need to ship
// a separate bfbs directory or set `ROLLIO_BFBS_DIR`. The files live next
// to this source under `episode-mcap/schemas/`.
const BFBS_COMPRESSED_VIDEO: &[u8] = include_bytes!("../schemas/CompressedVideo.bfbs");
const BFBS_RAW_IMAGE: &[u8] = include_bytes!("../schemas/RawImage.bfbs");
const BFBS_JOINT_STATES: &[u8] = include_bytes!("../schemas/JointStates.bfbs");
const BFBS_IMU: &[u8] = include_bytes!("../schemas/Imu.bfbs");
const BFBS_TACTILE_DATA: &[u8] = include_bytes!("../schemas/TactileData.bfbs");
const BFBS_CAMERA_CALIBRATION: &[u8] = include_bytes!("../schemas/CameraCalibration.bfbs");
const BFBS_FRAME_TRANSFORM: &[u8] = include_bytes!("../schemas/FrameTransform.bfbs");

/// Known MCAP channel schema types used by the assembler.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SchemaType {
    CompressedVideo,
    RawImage,
    JointStates,
    Imu,
    TactileData,
    CameraCalibration,
    FrameTransform,
}

impl SchemaType {
    /// The canonical schema name as it appears in the MCAP file.
    pub fn schema_name(self) -> &'static str {
        match self {
            Self::CompressedVideo => "foxglove.CompressedVideo",
            Self::RawImage => "foxglove.RawImage",
            Self::JointStates => "foxglove.JointStates",
            Self::Imu => "discover.Imu",
            Self::TactileData => "discover.TactileData",
            Self::CameraCalibration => "foxglove.CameraCalibration",
            Self::FrameTransform => "foxglove.FrameTransform",
        }
    }

    /// Compile-time-embedded FlatBuffer reflection schema bytes.
    pub fn embedded_bfbs(self) -> &'static [u8] {
        match self {
            Self::CompressedVideo => BFBS_COMPRESSED_VIDEO,
            Self::RawImage => BFBS_RAW_IMAGE,
            Self::JointStates => BFBS_JOINT_STATES,
            Self::Imu => BFBS_IMU,
            Self::TactileData => BFBS_TACTILE_DATA,
            Self::CameraCalibration => BFBS_CAMERA_CALIBRATION,
            Self::FrameTransform => BFBS_FRAME_TRANSFORM,
        }
    }
}

/// Registered channel in the MCAP file.
#[derive(Debug, Clone)]
pub struct McapChannel {
    pub channel_id: u16,
    pub topic: String,
    pub schema_type: SchemaType,
    pub sequence: u32,
}

/// High-level MCAP episode writer.
///
/// Manages schema registration, channel creation, and message writing
/// for a single MCAP episode file.
pub struct McapEpisodeWriter {
    writer: Writer<BufWriter<File>>,
    channels: Vec<McapChannel>,
    output_path: PathBuf,
    /// Maps schema_type -> mcap schema_id (to avoid re-registering).
    schema_ids: BTreeMap<SchemaType, u16>,
}

impl McapEpisodeWriter {
    /// Create a new MCAP episode writer at the given path.
    pub fn new(output_path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let file = File::create(output_path)?;
        let buf_writer = BufWriter::new(file);
        let writer = Writer::new(buf_writer)?;

        Ok(Self {
            writer,
            channels: Vec::new(),
            output_path: output_path.to_path_buf(),
            schema_ids: BTreeMap::new(),
        })
    }

    /// Register a schema if not already registered, returning its MCAP schema ID.
    pub fn ensure_schema(
        &mut self,
        schema_type: SchemaType,
    ) -> Result<u16, Box<dyn std::error::Error>> {
        if let Some(&id) = self.schema_ids.get(&schema_type) {
            return Ok(id);
        }
        let schema_id = self.writer.add_schema(
            schema_type.schema_name(),
            SCHEMA_ENCODING,
            schema_type.embedded_bfbs(),
        )?;
        self.schema_ids.insert(schema_type, schema_id);
        Ok(schema_id)
    }

    /// Add a channel to the MCAP file.
    ///
    /// Returns the index into `self.channels` for later reference.
    pub fn add_channel(
        &mut self,
        topic: &str,
        schema_type: SchemaType,
    ) -> Result<usize, Box<dyn std::error::Error>> {
        let schema_id = self.ensure_schema(schema_type)?;
        let channel_id =
            self.writer
                .add_channel(schema_id, topic, MESSAGE_ENCODING, &BTreeMap::new())?;
        let idx = self.channels.len();
        self.channels.push(McapChannel {
            channel_id,
            topic: topic.to_string(),
            schema_type,
            sequence: 0,
        });
        Ok(idx)
    }

    /// Write a message to the specified channel.
    ///
    /// `timestamp_ns` is the log time in nanoseconds since epoch.
    /// `data` is the serialized FlatBuffer payload.
    pub fn write_message(
        &mut self,
        channel_idx: usize,
        timestamp_ns: u64,
        data: &[u8],
    ) -> Result<(), Box<dyn std::error::Error>> {
        let channel = &mut self.channels[channel_idx];
        channel.sequence += 1;
        let header = MessageHeader {
            channel_id: channel.channel_id,
            sequence: channel.sequence,
            log_time: timestamp_ns,
            publish_time: timestamp_ns,
        };
        self.writer.write_to_known_channel(&header, data)?;
        Ok(())
    }

    /// Write an MCAP metadata record (key-value pairs for episode info).
    pub fn write_metadata(
        &mut self,
        name: &str,
        entries: BTreeMap<String, String>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let metadata = Metadata {
            name: name.to_string(),
            metadata: entries,
        };
        self.writer.write_metadata(&metadata)?;
        Ok(())
    }

    /// Finalize and close the MCAP file.
    pub fn finish(mut self) -> Result<PathBuf, Box<dyn std::error::Error>> {
        self.writer.finish()?;
        Ok(self.output_path)
    }

    /// Get the output path.
    pub fn output_path(&self) -> &Path {
        &self.output_path
    }

    /// Get registered channels.
    pub fn channels(&self) -> &[McapChannel] {
        &self.channels
    }
}

/// Convert a microsecond timestamp to nanoseconds (for MCAP log_time).
pub fn us_to_ns(timestamp_us: u64) -> u64 {
    timestamp_us.saturating_mul(1_000)
}
