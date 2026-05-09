//! Mux H.264 / H.265 / AV1 / MJPG packets into MP4 / MKV via libavformat.
//!
//! The encoder ships Annex B access units in the packet payload (for
//! H.264/H.265), AV1 OBUs, or self-contained JPEGs (MJPG). This
//! module wraps libavformat's muxer and feeds packets verbatim,
//! relying on `mp4` / `matroska` to produce the final container.
//!
//! H.264/H.265 codec extradata travels in the `Config` packet as
//! Annex B SPS+PPS. libavformat's MP4 muxer accepts Annex B in its
//! global codec extradata thanks to the `h264_mp4toannexb` /
//! `hevc_mp4toannexb` bsfs being applied automatically when the
//! input is identified as Annex B; we simply hand the bytes to the
//! codec parameters and let libavformat handle the rest.

use crate::packets::RecordingStreamBuffer;
use ffmpeg_next as ffmpeg;
use rollio_types::config::EncoderCodec;
use std::error::Error;
use std::path::Path;

pub fn write_stream(target: &Path, stream: &RecordingStreamBuffer) -> Result<(), Box<dyn Error>> {
    let config = stream
        .config
        .as_ref()
        .ok_or("video stream missing Config packet")?;
    ffmpeg::init().ok();

    let codec_id = match config.codec {
        EncoderCodec::H264 => ffmpeg::codec::Id::H264,
        EncoderCodec::H265 => ffmpeg::codec::Id::HEVC,
        EncoderCodec::Av1 => ffmpeg::codec::Id::AV1,
        EncoderCodec::Mjpg => ffmpeg::codec::Id::MJPEG,
        EncoderCodec::Rvl => {
            return Err("ffmpeg_video::write_stream cannot mux RVL streams".into());
        }
    };

    let mut output = ffmpeg::format::output(&target)?;
    let codec = ffmpeg::encoder::find(codec_id)
        .ok_or_else(|| format!("encoder for {codec_id:?} not found"))?;
    let stream_index;
    {
        let mut output_stream = output.add_stream(codec)?;
        stream_index = output_stream.index();
        // Tell the muxer to use the encoder's microsecond time base;
        // most container muxers will rewrite this internally during
        // `write_header`, but it serves as a sane starting point.
        let tb = ffmpeg::Rational(
            config.time_base_num.max(1) as i32,
            config.time_base_den.max(1) as i32,
        );
        output_stream.set_time_base(tb);
        unsafe {
            let params = output_stream.parameters().as_mut_ptr();
            (*params).codec_type = ffmpeg::media::Type::Video.into();
            (*params).codec_id = codec_id.into();
            (*params).width = config.width as i32;
            (*params).height = config.height as i32;
            (*params).format = ffmpeg::util::format::pixel::Pixel::YUV420P as i32;
            (*params).codec_tag = 0;
            // Hand over the codec extradata bytes (Annex B SPS/PPS for
            // H.264/H.265, AV1 sequence header, empty for MJPG). The
            // container muxer takes ownership.
            if !config.extradata.is_empty() {
                let len = config.extradata.len();
                let buf = ffmpeg::ffi::av_mallocz(
                    len + ffmpeg::ffi::AV_INPUT_BUFFER_PADDING_SIZE as usize,
                );
                if buf.is_null() {
                    return Err("av_mallocz returned null while copying codec extradata".into());
                }
                std::ptr::copy_nonoverlapping(config.extradata.as_ptr(), buf as *mut u8, len);
                (*params).extradata = buf as *mut u8;
                (*params).extradata_size = len as i32;
            }
        }
    }
    output.write_header()?;
    let stream_time_base = output
        .stream(stream_index)
        .ok_or("missing video stream after write_header")?
        .time_base();

    let encoder_time_base = ffmpeg::Rational(
        config.time_base_num.max(1) as i32,
        config.time_base_den.max(1) as i32,
    );

    for record in &stream.packets {
        let mut packet = ffmpeg::Packet::copy(&record.payload);
        packet.set_stream(stream_index);
        packet.set_pts(Some(record.header.pts_us));
        packet.set_dts(Some(record.header.dts_us));
        if record.header.duration_us > 0 {
            packet.set_duration(record.header.duration_us);
        }
        if record.header.is_keyframe() {
            packet.set_flags(ffmpeg::packet::Flags::KEY);
        }
        packet.rescale_ts(encoder_time_base, stream_time_base);
        packet.write_interleaved(&mut output)?;
    }
    output.write_trailer()?;
    Ok(())
}
