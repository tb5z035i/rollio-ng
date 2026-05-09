//! RVL container muxer.
//!
//! The encoder emits the RVL container preamble (`magic + width +
//! height + fps`) once as `Config.extradata`, then ships per-frame
//! `[ts_us, frame_index, payload_len, payload]` blocks as packet
//! payloads. We reproduce the legacy `.rvl` byte layout by writing
//! the preamble verbatim followed by every packet payload in order.

use crate::packets::RecordingStreamBuffer;
use std::error::Error;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

pub fn write_stream(target: &Path, stream: &RecordingStreamBuffer) -> Result<(), Box<dyn Error>> {
    let config = stream
        .config
        .as_ref()
        .ok_or("RVL stream missing Config packet")?;
    let file = File::create(target)?;
    let mut writer = BufWriter::new(file);
    writer.write_all(&config.extradata)?;
    for packet in &stream.packets {
        writer.write_all(&packet.payload)?;
    }
    writer.flush()?;
    Ok(())
}
