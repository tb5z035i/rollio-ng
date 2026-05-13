//! Minimal libav bitstream-filter wrapper.
//!
//! ffmpeg-sys-next's bindgen config doesn't include `libavcodec/bsf.h`,
//! so we declare just the symbols we need. The API is on the C-ABI
//! stable surface and hasn't changed since libavcodec 58 (FFmpeg 4.x).
//!
//! Only `mjpeg2jpeg` is exercised today — it injects a standard JFIF
//! DHT (Huffman table) segment into V4L2 / UVC MJPG frames so that
//! `mjpeg_cuvid` can accept them. NVDEC's MJPEG decoder requires
//! JFIF-compliant streams; UVC strips DHT to save bandwidth, citing
//! "use the standard tables" in spec — but JFIF parsers (including
//! NVDEC) require the DHT segment to be present in the stream.
//!
//! The wrapper is general enough to plug other BSFs in later
//! (e.g. `h264_mp4toannexb` if a future camera publishes AVCC).
//!
//! Lifetime: the safe wrapper owns the AVBSFContext and frees it on
//! Drop. Filter and codec-parameters pointers internal to the context
//! belong to libav; we never read them after free.
//!
//! Reentrancy: a BSF context is not internally synchronised. Each
//! NVIDIA session owns its own context; no sharing across threads.
//!
//! Error model: the C API returns negative `int` on error. We surface
//! these as `EncoderError::message` with the libav rc embedded so
//! callers can diagnose against `av_err2str`.

use std::ffi::{c_char, c_int, c_void, CString};
use std::ptr;

use ffmpeg_next as ffmpeg;
use ffmpeg::ffi as f;
use ffmpeg::packet::Mut;

use crate::error::{EncoderError, Result};

/// Opaque BSF descriptor returned by `av_bsf_get_by_name`. libav owns
/// it; we only ever pass the pointer through.
#[repr(C)]
struct AVBitStreamFilter {
    _opaque: [u8; 0],
}

/// Partial mirror of `AVBSFContext` from `libavcodec/bsf.h` (FFmpeg
/// 6.x, what Ubuntu's `libavcodec60` ships and what our static build
/// pins). Leading-field order is `av_class`, `filter`, `priv_data/tb5z035i/workspace`,
/// `par_in`, `par_out`, then the two time_base fields. We only need
/// `par_in` (caller fills it pre-init), so the rest is opaque
/// padding. Verified against
/// `/usr/include/x86_64-linux-gnu/libavcodec/bsf.h`; an earlier
/// guess that omitted `priv_data/tb5z035i/workspace` read a null `par_in` at runtime.
#[repr(C)]
struct AVBSFContext {
    _av_class: *const c_void,
    _filter: *const AVBitStreamFilter,
    _priv_data: *mut c_void,
    par_in: *mut f::AVCodecParameters,
    _par_out: *mut f::AVCodecParameters,
    // Remaining fields (time_base_in, time_base_out, priv_data, …)
    // intentionally elided. We never touch them.
}

extern "C" {
    fn av_bsf_get_by_name(name: *const c_char) -> *const AVBitStreamFilter;
    fn av_bsf_alloc(filter: *const AVBitStreamFilter, ctx: *mut *mut AVBSFContext) -> c_int;
    fn av_bsf_init(ctx: *mut AVBSFContext) -> c_int;
    fn av_bsf_send_packet(ctx: *mut AVBSFContext, pkt: *mut f::AVPacket) -> c_int;
    fn av_bsf_receive_packet(ctx: *mut AVBSFContext, pkt: *mut f::AVPacket) -> c_int;
    fn av_bsf_free(ctx: *mut *mut AVBSFContext);
}

/// Safe wrapper around an `mjpeg2jpeg` BSF context. Transforms each
/// MJPG packet by prepending a standard JFIF DHT segment, then hands
/// the packet back to the caller for forwarding to `mjpeg_cuvid`.
pub(crate) struct Mjpeg2JpegBsf {
    ctx: *mut AVBSFContext,
}

// The underlying AVBSFContext is not Sync (no internal locking), but
// each NvidiaCudaSession owns one BSF used from a single thread.
// `Send` is the only one we need; mirror the ffmpeg-next convention.
unsafe impl Send for Mjpeg2JpegBsf {}

impl Mjpeg2JpegBsf {
    /// Allocate, configure for MJPEG input, and init the BSF. The
    /// width / height we set on `par_in` are advisory — `mjpeg2jpeg`
    /// itself doesn't read them, but `av_bsf_init` validates that
    /// `codec_id` is what the named BSF supports.
    pub(crate) fn new() -> Result<Self> {
        let name = CString::new("mjpeg2jpeg").unwrap();
        let filter = unsafe { av_bsf_get_by_name(name.as_ptr()) };
        if filter.is_null() {
            return Err(EncoderError::message(
                "BSF `mjpeg2jpeg` not registered in libavcodec (NVDEC MJPG path requires it)",
            ));
        }
        let mut ctx: *mut AVBSFContext = ptr::null_mut();
        let rc = unsafe { av_bsf_alloc(filter, &mut ctx as *mut _) };
        if rc < 0 || ctx.is_null() {
            return Err(EncoderError::message(format!(
                "av_bsf_alloc(mjpeg2jpeg) failed: rc={rc}"
            )));
        }
        unsafe {
            let par_in = (*ctx).par_in;
            if par_in.is_null() {
                av_bsf_free(&mut ctx as *mut _);
                return Err(EncoderError::message(
                    "av_bsf_alloc returned a context with null par_in",
                ));
            }
            (*par_in).codec_type = f::AVMediaType::AVMEDIA_TYPE_VIDEO;
            (*par_in).codec_id = f::AVCodecID::AV_CODEC_ID_MJPEG;
        }
        let rc = unsafe { av_bsf_init(ctx) };
        if rc < 0 {
            unsafe { av_bsf_free(&mut ctx as *mut _) };
            return Err(EncoderError::message(format!(
                "av_bsf_init(mjpeg2jpeg) failed: rc={rc}"
            )));
        }
        Ok(Self { ctx })
    }

    /// Run one MJPG packet through the BSF in place. On success the
    /// caller can hand the (now-JFIF-compliant) packet to
    /// `mjpeg_cuvid`. The BSF is documented to produce exactly one
    /// output packet per input packet for `mjpeg2jpeg`, so the
    /// receive loop runs once; we still drain to be defensive against
    /// future filter variants.
    pub(crate) fn filter(&mut self, packet: &mut ffmpeg::Packet) -> Result<()> {
        // Hand ownership of the source packet over to the BSF; the
        // libav contract is that `av_bsf_send_packet` consumes the
        // input packet (after which it's empty), and
        // `av_bsf_receive_packet` fills it with the transformed
        // payload. We use the same Packet struct for both ends.
        let rc = unsafe { av_bsf_send_packet(self.ctx, packet.as_mut_ptr()) };
        if rc < 0 {
            return Err(EncoderError::message(format!(
                "av_bsf_send_packet(mjpeg2jpeg) failed: rc={rc}"
            )));
        }
        let rc = unsafe { av_bsf_receive_packet(self.ctx, packet.as_mut_ptr()) };
        if rc < 0 {
            return Err(EncoderError::message(format!(
                "av_bsf_receive_packet(mjpeg2jpeg) failed: rc={rc}"
            )));
        }
        // Sanity-check: mjpeg2jpeg is documented as 1-in-1-out, so a
        // second `receive_packet` must report EAGAIN. Anything else
        // (another packet, an error) is unexpected and worth
        // surfacing loudly rather than silently dropping payload.
        let mut extra = ffmpeg::Packet::empty();
        let rc = unsafe { av_bsf_receive_packet(self.ctx, extra.as_mut_ptr()) };
        if rc == f::AVERROR(f::EAGAIN) || rc == f::AVERROR_EOF {
            return Ok(());
        }
        if rc < 0 {
            return Err(EncoderError::message(format!(
                "av_bsf_receive_packet drain failed: rc={rc}"
            )));
        }
        Err(EncoderError::message(
            "mjpeg2jpeg BSF unexpectedly emitted >1 packet for one input",
        ))
    }
}

impl Drop for Mjpeg2JpegBsf {
    fn drop(&mut self) {
        if !self.ctx.is_null() {
            unsafe { av_bsf_free(&mut self.ctx as *mut _) };
        }
    }
}
