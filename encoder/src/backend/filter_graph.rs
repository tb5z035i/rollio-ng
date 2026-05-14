//! Hardware-aware scaling filter graph for the NVIDIA (and, in
//! phase 4, VAAPI) color backends.
//!
//! Two graph shapes depending on where the input frame lives:
//!
//! ```text
//! Cuvid input  (frame already on GPU, hw_frames_ctx from decoder):
//!     buffer ──► scale_cuda ──► buffersink
//!
//! Raw  input  (frame on CPU):
//!     buffer ──► hwupload ──► scale_cuda ──► buffersink
//! ```
//!
//! The plumbing is purely libav FFI because the ffmpeg-next wrappers
//! don't expose what we need:
//!
//! - `AVFilterGraph::hw_device_ctx` was removed in modern libav, so
//!   the CUDA device has to attach per-filter-context. We do that on
//!   the `hwupload` filter for the raw-input case, and propagate the
//!   device into the rest of the graph through `hw_frames_ctx` on the
//!   buffer source (for the Cuvid case, where `mjpeg_cuvid`/`*_cuvid`
//!   already produced a CUDA-backed `AVHWFramesContext`).
//!
//! - `avfilter_graph_create_filter` (which `ffmpeg-next::filter::Graph::add`
//!   wraps) allocates AND initialises the filter context in one shot;
//!   we need to set `hw_device_ctx` *between* alloc and init for
//!   hwupload, so we use the lower-level
//!   `avfilter_graph_alloc_filter` + `avfilter_init_str` pair.
//!
//! - For buffersrc, `av_buffersrc_parameters_set` is the runtime
//!   handle for stuffing `hw_frames_ctx` (and dims, format, time
//!   base) into the source after init.
//!
//! - The encoder side fishes `hw_frames_ctx` out of the buffersink's
//!   output via `av_buffersink_get_hw_frames_ctx`, which is the
//!   public way to do that.

use std::ffi::CString;
use std::ptr;

use ffmpeg::ffi as f;
use ffmpeg::filter::Graph;
use ffmpeg::util::format::pixel::Pixel;
use ffmpeg_next as ffmpeg;

use crate::error::{EncoderError, Result};
use crate::media::AvBufferRef;

/// Whether the input frames arrive on CPU (raw camera bytes, needing
/// `hwupload`) or already on the GPU (output of a hardware decoder).
/// `Hw` is generic — the actual hw type is selected via
/// [`ScaleGraphConfig::hw_accel`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InputResidency {
    Cpu,
    Hw,
}

/// Which hardware-acceleration backend the graph runs on. Picks the
/// scale filter name (`scale_cuda` vs `scale_vaapi`) and the hardware
/// pixel format (`Pixel::CUDA` vs `Pixel::VAAPI`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HwAccel {
    Cuda,
    Vaapi,
}

impl HwAccel {
    fn scale_filter_name(self) -> &'static str {
        match self {
            HwAccel::Cuda => "scale_cuda",
            HwAccel::Vaapi => "scale_vaapi",
        }
    }

    fn hw_pixel(self) -> Pixel {
        match self {
            HwAccel::Cuda => Pixel::CUDA,
            HwAccel::Vaapi => Pixel::VAAPI,
        }
    }

    fn hw_pixel_name(self) -> &'static str {
        match self {
            HwAccel::Cuda => "cuda",
            HwAccel::Vaapi => "vaapi",
        }
    }
}

/// Inputs to [`ScaleGraph::build`].
pub(crate) struct ScaleGraphConfig<'a> {
    /// Which HW family (CUDA or VAAPI).
    pub hw_accel: HwAccel,
    /// Hardware device the graph runs on (CUDA or VAAPI AVBufferRef).
    /// The helper clones the AVBufferRef as needed; the caller
    /// continues to own the original.
    pub hw_device: &'a AvBufferRef,
    pub residency: InputResidency,
    pub src_width: u32,
    pub src_height: u32,
    /// libav pixel format of the source. For hardware input this
    /// should be `Pixel::CUDA` or `Pixel::VAAPI` per `hw_accel`; for
    /// raw input it's the actual CPU pixel format (RGB24, YUYV422,
    /// NV12, …).
    pub src_pixel: Pixel,
    /// Used only for the hardware-input path: the decoder's
    /// `hw_frames_ctx`. Required when `residency = Hw`; ignored
    /// otherwise. The helper clones it; caller keeps ownership of
    /// the original ref.
    pub src_hw_frames_ctx: Option<*mut f::AVBufferRef>,
    pub dst_width: u32,
    pub dst_height: u32,
    /// Encoder-input "software" format inside the hardware frame.
    /// `Pixel::NV12` for both NVENC and VA-API's standard paths.
    pub dst_sw_format: Pixel,
    pub time_base: ffmpeg::Rational,
}

/// A constructed + validated filter graph plus pointers to its
/// source/sink filter contexts. Owns the graph; cleans up on drop.
pub(crate) struct ScaleGraph {
    // The graph owns every filter context the raw pointers below
    // point into. Field is intentionally unused beyond Drop; keeping
    // it here ensures the graph outlives `src_ctx` / `sink_ctx`.
    #[allow(dead_code)]
    graph: Graph,
    src_ctx: *mut f::AVFilterContext,
    sink_ctx: *mut f::AVFilterContext,
}

// The raw filter-context pointers live inside the AVFilterGraph owned
// by `graph`; they're valid as long as `Graph` is alive. The graph
// itself is `Send + Sync` per ffmpeg-next; we mirror that.
unsafe impl Send for ScaleGraph {}

impl ScaleGraph {
    pub(crate) fn build(config: ScaleGraphConfig<'_>) -> Result<Self> {
        let mut graph = Graph::new();
        let graph_ptr = unsafe { graph.as_mut_ptr() };

        // -- buffer source ----------------------------------------
        let src_name = CString::new("in").unwrap();
        let src_args = source_args(&config)?;
        let src_args_c = CString::new(src_args.as_str()).map_err(|e| {
            EncoderError::message(format!("buffer source args CString failed: {e}"))
        })?;
        let src_filter_name = CString::new("buffer").unwrap();
        let src_filter = unsafe { f::avfilter_get_by_name(src_filter_name.as_ptr()) };
        if src_filter.is_null() {
            return Err(EncoderError::message(
                "filter `buffer` not registered in libavfilter",
            ));
        }
        // `avfilter_graph_create_filter` allocates AND initialises the
        // context with the args we pass. For the source filter we
        // *want* args-based init (video_size, pix_fmt, time_base) and
        // we don't need any pre-init hw_device hook, so this single
        // call is fine here.
        let mut src_ctx: *mut f::AVFilterContext = ptr::null_mut();
        let rc = unsafe {
            f::avfilter_graph_create_filter(
                &mut src_ctx,
                src_filter,
                src_name.as_ptr(),
                src_args_c.as_ptr(),
                ptr::null_mut(),
                graph_ptr,
            )
        };
        if rc < 0 {
            return Err(EncoderError::message(format!(
                "avfilter_graph_create_filter(buffer, `{}`) failed: rc={}",
                src_args, rc
            )));
        }

        // For the hardware-decoder path, attach `hw_frames_ctx` to the
        // buffer source so the scale filter picks up the device from
        // the input link's hw_frames_ctx.
        if let InputResidency::Hw = config.residency {
            let frames_src = config.src_hw_frames_ctx.ok_or_else(|| {
                EncoderError::message("ScaleGraph: Hw residency requires `src_hw_frames_ctx`")
            })?;
            if frames_src.is_null() {
                return Err(EncoderError::message(
                    "ScaleGraph: src_hw_frames_ctx is null",
                ));
            }
            unsafe {
                let params = f::av_buffersrc_parameters_alloc();
                if params.is_null() {
                    return Err(EncoderError::message(
                        "av_buffersrc_parameters_alloc returned null",
                    ));
                }
                (*params).format = f::AVPixelFormat::from(config.hw_accel.hw_pixel()) as i32;
                (*params).width = config.src_width as i32;
                (*params).height = config.src_height as i32;
                (*params).time_base = f::AVRational {
                    num: config.time_base.numerator(),
                    den: config.time_base.denominator(),
                };
                // av_buffersrc_parameters_set takes ownership of any
                // `hw_frames_ctx` we attach via the params struct (it
                // moves the ref into the source's internal state).
                // Bump the ref ourselves so the caller's
                // `AVBufferRef` stays valid.
                (*params).hw_frames_ctx = f::av_buffer_ref(frames_src);
                let rc = f::av_buffersrc_parameters_set(src_ctx, params);
                f::av_free(params as *mut _);
                if rc < 0 {
                    return Err(EncoderError::message(format!(
                        "av_buffersrc_parameters_set failed: rc={rc}"
                    )));
                }
            }
        }

        // -- (raw path) hwupload ----------------------------------
        // For CPU input we need an `hwupload` filter between the
        // buffer and scale_cuda. `hwupload` requires `hw_device_ctx`
        // to be set on the filter context *before* init.
        let upload_ctx = if let InputResidency::Cpu = config.residency {
            let upload_name = CString::new("upload").unwrap();
            let upload_filter_name = CString::new("hwupload").unwrap();
            let upload_filter = unsafe { f::avfilter_get_by_name(upload_filter_name.as_ptr()) };
            if upload_filter.is_null() {
                return Err(EncoderError::message(
                    "filter `hwupload` not registered in libavfilter",
                ));
            }
            // `avfilter_graph_alloc_filter` returns an *uninitialised*
            // context so we can stamp `hw_device_ctx` before init.
            let ctx = unsafe {
                f::avfilter_graph_alloc_filter(graph_ptr, upload_filter, upload_name.as_ptr())
            };
            if ctx.is_null() {
                return Err(EncoderError::message(
                    "avfilter_graph_alloc_filter(hwupload) returned null",
                ));
            }
            unsafe {
                (*ctx).hw_device_ctx = f::av_buffer_ref(config.hw_device.as_ptr());
                let rc = f::avfilter_init_str(ctx, ptr::null());
                if rc < 0 {
                    return Err(EncoderError::message(format!(
                        "avfilter_init_str(hwupload) failed: rc={rc}"
                    )));
                }
            }
            Some(ctx)
        } else {
            None
        };

        // -- scale_cuda -------------------------------------------
        let scale_name = CString::new("scale").unwrap();
        let scale_filter_name = CString::new(config.hw_accel.scale_filter_name()).unwrap();
        let scale_filter = unsafe { f::avfilter_get_by_name(scale_filter_name.as_ptr()) };
        if scale_filter.is_null() {
            return Err(EncoderError::message(format!(
                "filter `{}` not registered (libavfilter built without {} support?)",
                config.hw_accel.scale_filter_name(),
                config.hw_accel.hw_pixel_name()
            )));
        }
        let scale_args = format!(
            "w={dw}:h={dh}:format={fmt}",
            dw = config.dst_width,
            dh = config.dst_height,
            fmt = pix_fmt_name(config.dst_sw_format)?,
        );
        let scale_args_c = CString::new(scale_args.as_str()).map_err(|e| {
            EncoderError::message(format!(
                "{} args CString failed: {e}",
                config.hw_accel.scale_filter_name()
            ))
        })?;
        let mut scale_ctx: *mut f::AVFilterContext = ptr::null_mut();
        let rc = unsafe {
            f::avfilter_graph_create_filter(
                &mut scale_ctx,
                scale_filter,
                scale_name.as_ptr(),
                scale_args_c.as_ptr(),
                ptr::null_mut(),
                graph_ptr,
            )
        };
        if rc < 0 {
            return Err(EncoderError::message(format!(
                "avfilter_graph_create_filter({}, `{}`) failed: rc={}",
                config.hw_accel.scale_filter_name(),
                scale_args,
                rc
            )));
        }

        // -- buffersink -------------------------------------------
        let sink_name = CString::new("out").unwrap();
        let sink_filter_name = CString::new("buffersink").unwrap();
        let sink_filter = unsafe { f::avfilter_get_by_name(sink_filter_name.as_ptr()) };
        if sink_filter.is_null() {
            return Err(EncoderError::message(
                "filter `buffersink` not registered in libavfilter",
            ));
        }
        let mut sink_ctx: *mut f::AVFilterContext = ptr::null_mut();
        let rc = unsafe {
            f::avfilter_graph_create_filter(
                &mut sink_ctx,
                sink_filter,
                sink_name.as_ptr(),
                ptr::null(),
                ptr::null_mut(),
                graph_ptr,
            )
        };
        if rc < 0 {
            return Err(EncoderError::message(format!(
                "avfilter_graph_create_filter(buffersink) failed: rc={rc}"
            )));
        }

        // -- Link --------------------------------------------------
        match config.residency {
            InputResidency::Hw => {
                link(src_ctx, 0, scale_ctx, 0)?;
                link(scale_ctx, 0, sink_ctx, 0)?;
            }
            InputResidency::Cpu => {
                let upload = upload_ctx.expect("upload_ctx allocated for Cpu residency");
                link(src_ctx, 0, upload, 0)?;
                link(upload, 0, scale_ctx, 0)?;
                link(scale_ctx, 0, sink_ctx, 0)?;
            }
        }

        // -- Config -----------------------------------------------
        let rc = unsafe { f::avfilter_graph_config(graph_ptr, ptr::null_mut()) };
        if rc < 0 {
            return Err(EncoderError::message(format!(
                "avfilter_graph_config (validate) failed: rc={rc}"
            )));
        }

        Ok(Self {
            graph,
            src_ctx,
            sink_ctx,
        })
    }

    /// Push one frame into the graph. The frame's PTS / format / dims
    /// must match what the source filter was configured with.
    pub(crate) fn send_frame(&mut self, frame: &mut ffmpeg::frame::Video) -> Result<()> {
        let rc = unsafe { f::av_buffersrc_add_frame_flags(self.src_ctx, frame.as_mut_ptr(), 0) };
        if rc < 0 {
            return Err(EncoderError::message(format!(
                "av_buffersrc_add_frame_flags failed: rc={rc}"
            )));
        }
        Ok(())
    }

    /// Pull one frame from the graph's sink. Returns `Ok(None)` when
    /// the sink is empty (no error, just no frame ready).
    pub(crate) fn receive_frame(&mut self, frame: &mut ffmpeg::frame::Video) -> Result<Option<()>> {
        let rc = unsafe { f::av_buffersink_get_frame(self.sink_ctx, frame.as_mut_ptr()) };
        if rc == 0 {
            Ok(Some(()))
        } else if rc == f::AVERROR(f::EAGAIN) || rc == f::AVERROR_EOF {
            Ok(None)
        } else {
            Err(EncoderError::message(format!(
                "av_buffersink_get_frame failed: rc={rc}"
            )))
        }
    }

    /// Clone the buffersink's output `hw_frames_ctx` for assignment
    /// to an encoder's `hw_frames_ctx`. Returns a raw `AVBufferRef`
    /// pointer (one new ref). The caller is responsible for handing
    /// it to the encoder context, which will manage its lifetime via
    /// `avcodec_free_context`.
    pub(crate) fn clone_output_hw_frames_ctx(&self) -> Result<*mut f::AVBufferRef> {
        let frames = unsafe { f::av_buffersink_get_hw_frames_ctx(self.sink_ctx) };
        if frames.is_null() {
            return Err(EncoderError::message(
                "buffersink has no hw_frames_ctx; was scale_cuda actually configured?",
            ));
        }
        let cloned = unsafe { f::av_buffer_ref(frames) };
        if cloned.is_null() {
            return Err(EncoderError::message(
                "av_buffer_ref on buffersink hw_frames_ctx returned null",
            ));
        }
        Ok(cloned)
    }
}

impl Drop for ScaleGraph {
    fn drop(&mut self) {
        // The Graph wrapper's Drop frees the AVFilterGraph (which
        // owns all the filter contexts we created). Nothing to do
        // here beyond letting `graph` go out of scope.
    }
}

fn link(
    src: *mut f::AVFilterContext,
    src_pad: u32,
    dst: *mut f::AVFilterContext,
    dst_pad: u32,
) -> Result<()> {
    let rc = unsafe { f::avfilter_link(src, src_pad, dst, dst_pad) };
    if rc < 0 {
        return Err(EncoderError::message(format!(
            "avfilter_link failed: rc={rc}"
        )));
    }
    Ok(())
}

/// Build the `args` string for the buffer source filter. For the
/// hardware-input path the actual `hw_frames_ctx` arrives later via
/// `av_buffersrc_parameters_set`; the args still need a sensible
/// `pix_fmt` so init succeeds (we pass `cuda` or `vaapi`).
fn source_args(config: &ScaleGraphConfig<'_>) -> Result<String> {
    let pix_fmt = match config.residency {
        InputResidency::Hw => config.hw_accel.hw_pixel_name(),
        InputResidency::Cpu => pix_fmt_name(config.src_pixel)?,
    };
    Ok(format!(
        "video_size={}x{}:pix_fmt={}:time_base={}/{}:pixel_aspect=1/1",
        config.src_width,
        config.src_height,
        pix_fmt,
        config.time_base.numerator(),
        config.time_base.denominator(),
    ))
}

fn pix_fmt_name(fmt: Pixel) -> Result<&'static str> {
    // The lavfi filter spec uses libav's canonical short names from
    // `av_get_pix_fmt_name`. Map the variants the color backends
    // actually plumb through scale_cuda; anything else is a bug to
    // surface loudly.
    Ok(match fmt {
        Pixel::NV12 => "nv12",
        Pixel::NV16 => "nv16",
        Pixel::YUV420P => "yuv420p",
        Pixel::YUV422P => "yuv422p",
        Pixel::YUVJ420P => "yuvj420p",
        // V4L2 webcams' MJPG payloads commonly decode to YUVJ422P
        // (4:2:2 chroma + JPEG full range). scale_cuda then handles
        // the J → MPEG range shift while it converts to NV12.
        Pixel::YUVJ422P => "yuvj422p",
        Pixel::YUVJ444P => "yuvj444p",
        Pixel::RGB24 => "rgb24",
        Pixel::BGR24 => "bgr24",
        Pixel::YUYV422 => "yuyv422",
        Pixel::GRAY8 => "gray",
        Pixel::CUDA => "cuda",
        Pixel::VAAPI => "vaapi",
        other => {
            return Err(EncoderError::message(format!(
                "filter_graph: unsupported pixel format {other:?} (extend pix_fmt_name to plumb \
                 the new format through scale_cuda)",
            )))
        }
    })
}
