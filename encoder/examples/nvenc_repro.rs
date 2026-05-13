//! Repro: drive the real `open_session` path with the exact params the
//! preview-role encoder uses, with verbose libav logging enabled so we
//! see which NVENC parameter NVENC rejects.

use ffmpeg_next as ffmpeg;
use rollio_encoder::codec::{open_session, CodecSessionParams, OwnedFrame};
use rollio_types::config::{ChromaSubsampling, EncoderBackend, EncoderCodec, EncoderColorSpace};
use rollio_types::messages::{CameraFrameHeader, PixelFormat};

fn main() {
    ffmpeg::init().expect("ffmpeg init");
    unsafe { ffmpeg::ffi::av_log_set_level(ffmpeg::ffi::AV_LOG_DEBUG) };

    // First frame mirrors the camera: 1920x1080 MJPEG bytes. The codec
    // session is opened at 320x240 (preview output dims).
    let first_frame = OwnedFrame {
        header: CameraFrameHeader {
            timestamp_us: 1_000_000,
            width: 1920,
            height: 1080,
            pixel_format: PixelFormat::Mjpeg,
            frame_index: 0,
        },
        payload: vec![0u8; 16], // open() doesn't inspect payload
    };

    let params = CodecSessionParams {
        codec: EncoderCodec::H264,
        backend: EncoderBackend::Auto,
        fps: 60,
        crf: Some(32),
        preset: None,
        tune: None,
        bit_depth: 8,
        chroma_subsampling: ChromaSubsampling::S420,
        color_space: EncoderColorSpace::Auto,
        process_id: "preview-encoder.repro",
        episode_index: 0,
        recording_start_us: first_frame.header.timestamp_us,
        output_width: 320,
        output_height: 240,
        allow_rescale: true,
    };

    eprintln!("==== attempting open_session at 320x240 H264 auto ====");
    match open_session(params, &first_frame) {
        Ok(_) => eprintln!("==== open SUCCEEDED ===="),
        Err(e) => eprintln!("==== open FAILED: {e} ===="),
    }
}
