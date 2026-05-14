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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packets::{EncodedPacketRecord, EncodedStreamConfig};
    use rollio_types::messages::{
        EncodedCodecId, EncodedPacketHeader, EncodedPacketKind, PixelFormat,
    };
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    const WIDTH: u32 = 64;
    const HEIGHT: u32 = 48;
    const FRAME_COUNT: usize = 4;
    const FRAME_DURATION_US: i64 = 33_333;

    #[test]
    fn h264_annex_b_packets_mux_to_decodable_mp4() -> Result<(), Box<dyn Error>> {
        let Some(stream) = build_annex_b_h264_stream()? else {
            eprintln!("skipping: libx264 encoder unavailable");
            return Ok(());
        };

        let dir = temp_dir("rollio-h264-annexb-mux-test")?;
        let target = dir.join("episode_000000.mp4");
        write_stream(&target, &stream)?;

        assert!(
            fs::metadata(&target)?.len() > 0,
            "muxer should create a non-empty MP4"
        );

        let mut input = ffmpeg::format::input(&target)?;
        let video_stream = input
            .streams()
            .best(ffmpeg::media::Type::Video)
            .ok_or("muxed MP4 is missing a video stream")?;
        assert_eq!(video_stream.parameters().id(), ffmpeg::codec::Id::H264);
        let stream_index = video_stream.index();

        let context = ffmpeg::codec::context::Context::from_parameters(video_stream.parameters())?;
        let mut decoder = context.decoder().video()?;
        assert_eq!(decoder.width(), WIDTH);
        assert_eq!(decoder.height(), HEIGHT);

        let mut decoded = 0usize;
        for (stream, packet) in input.packets() {
            if stream.index() != stream_index {
                continue;
            }
            decoder.send_packet(&packet)?;
            decoded += drain_decoder(&mut decoder)?;
        }
        decoder.send_eof()?;
        decoded += drain_decoder(&mut decoder)?;

        assert_eq!(
            decoded, FRAME_COUNT,
            "MP4 produced from Annex B AUs should decode every frame"
        );

        let _ = fs::remove_dir_all(dir);
        Ok(())
    }

    fn build_annex_b_h264_stream() -> Result<Option<RecordingStreamBuffer>, Box<dyn Error>> {
        ffmpeg::init().ok();
        let Some(codec) = ffmpeg::encoder::find_by_name("libx264") else {
            return Ok(None);
        };

        let mut encoder = ffmpeg::codec::context::Context::new_with_codec(codec)
            .encoder()
            .video()?;
        encoder.set_width(WIDTH);
        encoder.set_height(HEIGHT);
        encoder.set_aspect_ratio(ffmpeg::Rational(1, 1));
        encoder.set_format(ffmpeg::util::format::pixel::Pixel::YUV420P);
        encoder.set_frame_rate(Some(ffmpeg::Rational(30, 1)));
        encoder.set_time_base(ffmpeg::Rational(1, 1_000_000));
        encoder.set_max_b_frames(0);

        let mut options = ffmpeg::Dictionary::new();
        options.set("preset", "ultrafast");
        options.set("tune", "zerolatency");
        options.set("crf", "35");
        let mut encoder = encoder.open_as_with(codec, options)?;

        let mut encoded = Vec::new();
        for index in 0..FRAME_COUNT {
            let frame = synthetic_yuv420_frame(index);
            encoder.send_frame(&frame)?;
            drain_encoder(&mut encoder, &mut encoded)?;
        }
        encoder.send_eof()?;
        drain_encoder(&mut encoder, &mut encoded)?;

        let extradata = encoded
            .iter()
            .find_map(|payload| extract_sps_pps(payload))
            .ok_or("synthetic H.264 stream did not contain Annex B SPS/PPS")?;
        assert!(
            starts_with_annex_b_start_code(&extradata),
            "Config extradata must be Annex B SPS/PPS"
        );
        assert!(
            encoded
                .iter()
                .all(|payload| starts_with_annex_b_start_code(payload)),
            "every synthetic packet must be an Annex B access unit"
        );

        let mut packets = Vec::with_capacity(encoded.len());
        for (index, payload) in encoded.into_iter().enumerate() {
            let pts = index as i64 * FRAME_DURATION_US;
            let mut header = EncodedPacketHeader {
                kind: EncodedPacketKind::Packet,
                codec: EncodedCodecId::H264,
                flags: 0,
                width: WIDTH,
                height: HEIGHT,
                pixel_format: PixelFormat::H264AnnexB,
                _reserved0: 0,
                time_base_num: 1,
                time_base_den: 1_000_000,
                pts_us: pts,
                dts_us: pts,
                duration_us: FRAME_DURATION_US,
                sequence_number: index as u64 + 1,
                source_timestamp_us: pts as u64,
                source_frame_index: index as u64,
                episode_index: 0,
                payload_len: payload.len() as u32,
            };
            header.set_keyframe(contains_h264_nal_type(&payload, 5));
            packets.push(EncodedPacketRecord { header, payload });
        }

        let mut stream = RecordingStreamBuffer::default();
        stream.config = Some(EncodedStreamConfig {
            codec: rollio_types::config::EncoderCodec::H264,
            width: WIDTH,
            height: HEIGHT,
            pixel_format: PixelFormat::H264AnnexB,
            time_base_num: 1,
            time_base_den: 1_000_000,
            extradata,
        });
        stream.packets = packets;
        stream.eos_received = true;
        Ok(Some(stream))
    }

    fn drain_encoder(
        encoder: &mut ffmpeg::encoder::Video,
        encoded: &mut Vec<Vec<u8>>,
    ) -> Result<(), Box<dyn Error>> {
        let mut packet = ffmpeg::Packet::empty();
        while encoder.receive_packet(&mut packet).is_ok() {
            let payload = packet
                .data()
                .ok_or("encoder returned a packet without payload")?
                .to_vec();
            encoded.push(payload);
        }
        Ok(())
    }

    fn drain_decoder(decoder: &mut ffmpeg::decoder::Video) -> Result<usize, Box<dyn Error>> {
        let mut decoded = 0usize;
        loop {
            let mut frame = ffmpeg::frame::Video::empty();
            match decoder.receive_frame(&mut frame) {
                Ok(()) => decoded += 1,
                Err(_) => break,
            }
        }
        Ok(decoded)
    }

    fn synthetic_yuv420_frame(index: usize) -> ffmpeg::frame::Video {
        let mut frame =
            ffmpeg::frame::Video::new(ffmpeg::util::format::pixel::Pixel::YUV420P, WIDTH, HEIGHT);
        frame.set_pts(Some(index as i64 * FRAME_DURATION_US));

        let y_stride = frame.stride(0);
        let u_stride = frame.stride(1);
        let v_stride = frame.stride(2);

        {
            let y = frame.data_mut(0);
            for row in 0..HEIGHT as usize {
                for col in 0..WIDTH as usize {
                    y[row * y_stride + col] = ((row + col + index * 7) % 256) as u8;
                }
            }
        }
        {
            let u = frame.data_mut(1);
            for row in 0..(HEIGHT / 2) as usize {
                for col in 0..(WIDTH / 2) as usize {
                    u[row * u_stride + col] = 96_u8.saturating_add((index as u8) * 8);
                }
            }
        }
        {
            let v = frame.data_mut(2);
            for row in 0..(HEIGHT / 2) as usize {
                for col in 0..(WIDTH / 2) as usize {
                    v[row * v_stride + col] = 160_u8.saturating_sub((index as u8) * 8);
                }
            }
        }

        frame
    }

    fn extract_sps_pps(data: &[u8]) -> Option<Vec<u8>> {
        let nalus = split_annex_b_nalus(data);
        let mut sps: Option<&[u8]> = None;
        let mut pps: Option<&[u8]> = None;
        for nalu in nalus {
            if nalu.is_empty() {
                continue;
            }
            match nalu[0] & 0x1F {
                7 => sps.get_or_insert(nalu),
                8 => pps.get_or_insert(nalu),
                _ => continue,
            };
        }
        let sps = sps?;
        let pps = pps?;
        let mut out = Vec::with_capacity(4 + sps.len() + 4 + pps.len());
        out.extend_from_slice(&[0, 0, 0, 1]);
        out.extend_from_slice(sps);
        out.extend_from_slice(&[0, 0, 0, 1]);
        out.extend_from_slice(pps);
        Some(out)
    }

    fn contains_h264_nal_type(data: &[u8], needle: u8) -> bool {
        split_annex_b_nalus(data)
            .iter()
            .any(|nalu| !nalu.is_empty() && (nalu[0] & 0x1F) == needle)
    }

    fn split_annex_b_nalus(bytes: &[u8]) -> Vec<&[u8]> {
        let mut starts: Vec<(usize, usize)> = Vec::new();
        let mut i = 0;
        while i + 2 < bytes.len() {
            if bytes[i] == 0 && bytes[i + 1] == 0 {
                if bytes[i + 2] == 1 {
                    starts.push((i, 3));
                    i += 3;
                    continue;
                }
                if i + 3 < bytes.len() && bytes[i + 2] == 0 && bytes[i + 3] == 1 {
                    starts.push((i, 4));
                    i += 4;
                    continue;
                }
            }
            i += 1;
        }

        starts
            .iter()
            .enumerate()
            .filter_map(|(idx, &(offset, prefix))| {
                let body_start = offset + prefix;
                let body_end = starts
                    .get(idx + 1)
                    .map(|(next, _)| *next)
                    .unwrap_or(bytes.len());
                (body_start < body_end).then_some(&bytes[body_start..body_end])
            })
            .collect()
    }

    fn starts_with_annex_b_start_code(bytes: &[u8]) -> bool {
        bytes.starts_with(&[0, 0, 1]) || bytes.starts_with(&[0, 0, 0, 1])
    }

    fn temp_dir(prefix: &str) -> Result<std::path::PathBuf, Box<dyn Error>> {
        let suffix = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let dir = std::env::temp_dir().join(format!("{prefix}-{}-{suffix}", std::process::id()));
        fs::create_dir_all(&dir)?;
        Ok(dir)
    }
}
