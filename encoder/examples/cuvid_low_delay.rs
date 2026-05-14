//! Probe whether AV_CODEC_FLAG_LOW_DELAY eliminates mjpeg_cuvid's
//! 3-packet warmup. Reads a captured V4L2 MJPG stream, sends frames
//! one at a time, and reports for each input packet whether a
//! decoded frame falls out immediately (LOW_DELAY working) or only
//! after several packets queued up (default).

use std::env;
use std::fs;

use ffmpeg_next as ffmpeg;
use ffmpeg::ffi as f;

fn main() {
    ffmpeg::init().expect("ffmpeg init");

    let path = env::args()
        .nth(1)
        .unwrap_or_else(|| "/tmp/sample-10.bin".to_string());
    let bytes = fs::read(&path).expect("read sample MJPG");
    let mut frames: Vec<Vec<u8>> = Vec::new();
    let mut last = 0usize;
    let mut i = 1usize;
    while i < bytes.len() {
        if bytes[i - 1] == 0xff && bytes[i] == 0xd8 && (i - 1) != last {
            frames.push(bytes[last..(i - 1)].to_vec());
            last = i - 1;
        }
        i += 1;
    }
    frames.push(bytes[last..].to_vec());
    eprintln!("split into {} frames", frames.len());

    for &low_delay in &[false, true] {
        let label = if low_delay { "LOW_DELAY" } else { "default" };
        eprintln!("\n==== {label} ====");

        let mut hw_device: *mut f::AVBufferRef = std::ptr::null_mut();
        let rc = unsafe {
            f::av_hwdevice_ctx_create(
                &mut hw_device,
                f::AVHWDeviceType::AV_HWDEVICE_TYPE_CUDA,
                std::ptr::null(),
                std::ptr::null_mut(),
                0,
            )
        };
        assert!(rc >= 0 && !hw_device.is_null());

        let decoder_filter =
            ffmpeg::decoder::find_by_name("mjpeg_cuvid").expect("mjpeg_cuvid not found");
        let mut ctx = ffmpeg::codec::context::Context::new_with_codec(decoder_filter);
        unsafe {
            (*ctx.as_mut_ptr()).hw_device_ctx = f::av_buffer_ref(hw_device);
            (*ctx.as_mut_ptr()).width = 1920;
            (*ctx.as_mut_ptr()).height = 1080;
            if low_delay {
                (*ctx.as_mut_ptr()).flags |= f::AV_CODEC_FLAG_LOW_DELAY as i32;
            }
        }
        let mut decoder = ctx.decoder().video().expect("decoder().video() failed");

        let mut first_output_at: Option<usize> = None;
        for (fi, fb) in frames.iter().enumerate() {
            let packet = ffmpeg::Packet::copy(fb);
            decoder.send_packet(&packet).expect("send_packet");
            let mut got = 0;
            loop {
                let mut decoded = ffmpeg::frame::Video::empty();
                if decoder.receive_frame(&mut decoded).is_err() {
                    break;
                }
                got += 1;
                if first_output_at.is_none() {
                    first_output_at = Some(fi);
                }
            }
            eprintln!("  packet {fi} sent ({}b) -> {} frame(s) out", fb.len(), got);
        }
        eprintln!(
            "  first output emerged at packet index {:?}",
            first_output_at
        );

        unsafe { f::av_buffer_unref(&mut hw_device) };
    }
}
