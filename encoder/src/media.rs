//! Shared FFmpeg / libav helpers used by codec sessions and probing.

use crate::error::{EncoderError, Result};
use ffmpeg_next as ffmpeg;
use rollio_types::config::{
    ChromaSubsampling, EncoderBackend, EncoderCapability, EncoderCapabilityDirection,
    EncoderCapabilityReport, EncoderCodec, EncoderColorSpace, EncoderImplementationFamily,
};
use rollio_types::messages::{CameraFrameHeader, PixelFormat};
use rvl::{
    CodecKind as RvlCodecKind, DepthDecoder, EncodedFrame as RvlEncodedFrame,
    FrameKind as RvlFrameKind,
};
use std::ffi::CString;
use std::fs::{self, File};
use std::io::{BufReader, Read};
use std::path::Path;
use std::ptr;
use std::sync::OnceLock;
use std::time::Duration;

const RVL_MAGIC: &[u8; 4] = b"RVL1";

#[derive(Debug, Clone, Default)]
pub struct EncodeMetrics {
    pub frames: usize,
    pub raw_bytes: usize,
    pub encoded_bytes: usize,
    pub dropped_frames: usize,
    pub encode_time: Duration,
}

impl EncodeMetrics {
    pub fn record_frame(&mut self, raw_bytes: usize, encoded_bytes: usize, elapsed: Duration) {
        self.frames += 1;
        self.raw_bytes += raw_bytes;
        self.encoded_bytes += encoded_bytes;
        self.encode_time += elapsed;
    }
}

#[derive(Debug, Clone, Default)]
pub struct DecodedArtifact {
    pub width: u32,
    pub height: u32,
    pub frame_count: usize,
    pub first_rgb_frame: Option<Vec<u8>>,
    pub last_rgb_frame: Option<Vec<u8>>,
    pub first_depth_frame: Option<Vec<u16>>,
    pub last_depth_frame: Option<Vec<u16>>,
}

static FFMPEG_INITIALIZED: OnceLock<Result<()>> = OnceLock::new();

pub fn ensure_ffmpeg_initialized() -> Result<()> {
    match FFMPEG_INITIALIZED.get_or_init(|| {
        let result = ffmpeg::init().map_err(Into::into);
        if std::env::var("ROLLIO_FFMPEG_DEBUG").is_ok() {
            unsafe { ffmpeg::ffi::av_log_set_level(ffmpeg::ffi::AV_LOG_DEBUG) };
        }
        result
    }) {
        Ok(()) => Ok(()),
        Err(error) => Err(EncoderError::message(error.to_string())),
    }
}

pub(crate) struct AvBufferRef {
    ptr: *mut ffmpeg::ffi::AVBufferRef,
}

impl AvBufferRef {
    pub(crate) fn new(ptr: *mut ffmpeg::ffi::AVBufferRef, context: &str) -> Result<Self> {
        if ptr.is_null() {
            return Err(EncoderError::message(format!(
                "{context}: received null AVBufferRef"
            )));
        }
        Ok(Self { ptr })
    }

    pub(crate) fn clone_raw(&self) -> Result<*mut ffmpeg::ffi::AVBufferRef> {
        let cloned = unsafe { ffmpeg::ffi::av_buffer_ref(self.ptr) };
        if cloned.is_null() {
            return Err(EncoderError::message("av_buffer_ref returned null"));
        }
        Ok(cloned)
    }

    pub(crate) fn as_ptr(&self) -> *mut ffmpeg::ffi::AVBufferRef {
        self.ptr
    }
}

impl Drop for AvBufferRef {
    fn drop(&mut self) {
        unsafe {
            ffmpeg::ffi::av_buffer_unref(&mut self.ptr);
        }
    }
}

pub fn probe_capabilities() -> Result<EncoderCapabilityReport> {
    ensure_ffmpeg_initialized()?;
    let mut codecs = Vec::new();
    let video_pixel_formats = &[
        PixelFormat::Rgb24,
        PixelFormat::Bgr24,
        PixelFormat::Gray8,
        PixelFormat::Yuyv,
        PixelFormat::Mjpeg,
    ];
    for codec in [EncoderCodec::H264, EncoderCodec::H265, EncoderCodec::Av1] {
        codecs.extend(probe_video_capabilities(
            codec,
            &[
                EncoderBackend::Cpu,
                EncoderBackend::Nvidia,
                EncoderBackend::Vaapi,
            ],
            video_pixel_formats,
        ));
    }
    codecs.extend(probe_video_capabilities(
        EncoderCodec::Mjpg,
        &[EncoderBackend::Cpu],
        video_pixel_formats,
    ));
    codecs.push(EncoderCapability {
        codec: EncoderCodec::Rvl,
        implementation: EncoderImplementationFamily::Rvl,
        direction: EncoderCapabilityDirection::Encode,
        backend: EncoderBackend::Cpu,
        pixel_formats: vec![PixelFormat::Depth16],
        available: true,
        codec_name: Some("rvl".to_string()),
        note: Some("pure Rust in-repo depth encoder".to_string()),
    });
    codecs.push(EncoderCapability {
        codec: EncoderCodec::Rvl,
        implementation: EncoderImplementationFamily::Rvl,
        direction: EncoderCapabilityDirection::Decode,
        backend: EncoderBackend::Cpu,
        pixel_formats: vec![PixelFormat::Depth16],
        available: true,
        codec_name: Some("rvl".to_string()),
        note: Some("pure Rust in-repo depth decoder".to_string()),
    });
    Ok(EncoderCapabilityReport { codecs })
}

fn probe_video_capabilities(
    codec: EncoderCodec,
    backends: &[EncoderBackend],
    pixel_formats: &[PixelFormat],
) -> Vec<EncoderCapability> {
    let mut capabilities = Vec::new();
    for &backend in backends {
        let encode_name = select_encoder_name(codec, backend).map(ToOwned::to_owned);
        let decode_name = select_decoder_name(codec, backend).map(ToOwned::to_owned);

        capabilities.push(EncoderCapability {
            codec,
            implementation: EncoderImplementationFamily::Libav,
            direction: EncoderCapabilityDirection::Encode,
            backend,
            pixel_formats: pixel_formats.to_vec(),
            available: encode_name.is_some(),
            codec_name: encode_name.clone(),
            note: availability_note(backend, encode_name.is_some()),
        });
        capabilities.push(EncoderCapability {
            codec,
            implementation: EncoderImplementationFamily::Libav,
            direction: EncoderCapabilityDirection::Decode,
            backend,
            pixel_formats: pixel_formats.to_vec(),
            available: decode_name.is_some(),
            codec_name: decode_name.clone(),
            note: availability_note(backend, decode_name.is_some()),
        });
    }
    capabilities
}

fn availability_note(backend: EncoderBackend, available: bool) -> Option<String> {
    if !available {
        return None;
    }
    Some(match backend {
        EncoderBackend::Auto => "auto resolves to the best available backend".into(),
        EncoderBackend::Cpu => "software codec path".into(),
        EncoderBackend::Nvidia => "requires CUDA/NVENC capable host libraries".into(),
        EncoderBackend::Vaapi => "requires VAAPI-capable host libraries".into(),
    })
}

pub(crate) fn resolve_backend(codec: EncoderCodec, requested: EncoderBackend) -> EncoderBackend {
    if codec == EncoderCodec::Rvl {
        return EncoderBackend::Cpu;
    }
    if requested != EncoderBackend::Auto {
        return requested;
    }
    for candidate in [
        EncoderBackend::Nvidia,
        EncoderBackend::Vaapi,
        EncoderBackend::Cpu,
    ] {
        if select_encoder_name(codec, candidate).is_some() {
            return candidate;
        }
    }
    EncoderBackend::Cpu
}

pub(crate) fn select_encoder_name(
    codec: EncoderCodec,
    backend: EncoderBackend,
) -> Option<&'static str> {
    if !backend_is_usable(backend) {
        return None;
    }
    let candidates = match (codec, backend) {
        (EncoderCodec::H264, EncoderBackend::Cpu) => &["libx264", "h264"][..],
        (EncoderCodec::H264, EncoderBackend::Nvidia) => &["h264_nvenc"][..],
        (EncoderCodec::H264, EncoderBackend::Vaapi) => &["h264_vaapi"][..],
        (EncoderCodec::H265, EncoderBackend::Cpu) => &["libx265", "hevc"][..],
        (EncoderCodec::H265, EncoderBackend::Nvidia) => &["hevc_nvenc"][..],
        (EncoderCodec::H265, EncoderBackend::Vaapi) => &["hevc_vaapi"][..],
        (EncoderCodec::Av1, EncoderBackend::Cpu) => {
            &["libsvtav1", "librav1e", "libaom-av1", "av1"][..]
        }
        (EncoderCodec::Av1, EncoderBackend::Nvidia) => &["av1_nvenc"][..],
        (EncoderCodec::Av1, EncoderBackend::Vaapi) => &["av1_vaapi"][..],
        (EncoderCodec::Mjpg, EncoderBackend::Cpu) => &["mjpeg"][..],
        (EncoderCodec::Rvl, EncoderBackend::Cpu) => &["rvl"][..],
        _ => &[][..],
    };
    candidates
        .iter()
        .copied()
        .find(|candidate| codec_encoder_exists(candidate))
}

fn select_decoder_name(codec: EncoderCodec, backend: EncoderBackend) -> Option<&'static str> {
    if !backend_is_usable(backend) {
        return None;
    }
    let candidates = match (codec, backend) {
        (EncoderCodec::H264, EncoderBackend::Cpu) => &["h264"][..],
        (EncoderCodec::H264, EncoderBackend::Nvidia) => &["h264_cuvid"][..],
        (EncoderCodec::H264, EncoderBackend::Vaapi) => &["h264"][..],
        (EncoderCodec::H265, EncoderBackend::Cpu) => &["hevc"][..],
        (EncoderCodec::H265, EncoderBackend::Nvidia) => &["hevc_cuvid"][..],
        (EncoderCodec::H265, EncoderBackend::Vaapi) => &["hevc"][..],
        (EncoderCodec::Av1, EncoderBackend::Cpu) => &["av1"][..],
        (EncoderCodec::Av1, EncoderBackend::Nvidia) => &["av1_cuvid"][..],
        (EncoderCodec::Av1, EncoderBackend::Vaapi) => &["av1"][..],
        (EncoderCodec::Mjpg, EncoderBackend::Cpu) => &["mjpeg"][..],
        (EncoderCodec::Rvl, EncoderBackend::Cpu) => &["rvl"][..],
        _ => &[][..],
    };
    candidates.iter().copied().find(|name| {
        if *name == "rvl" {
            true
        } else {
            codec_decoder_exists(name)
        }
    })
}

fn backend_is_usable(backend: EncoderBackend) -> bool {
    match backend {
        EncoderBackend::Auto | EncoderBackend::Cpu => true,
        EncoderBackend::Nvidia => {
            Path::new("/dev/nvidiactl").exists()
                || Path::new("/proc/driver/nvidia/version").exists()
        }
        EncoderBackend::Vaapi => {
            fs::read_dir("/dev/dri")
                .map(|dir| {
                    dir.filter_map(|e| e.ok())
                        .any(|e| e.file_name().to_string_lossy().starts_with("renderD"))
                })
                .unwrap_or(false)
                || Path::new("/dev/dri/card0").exists()
        }
    }
}

fn codec_encoder_exists(name: &str) -> bool {
    codec_by_name(name, true)
}

fn codec_decoder_exists(name: &str) -> bool {
    codec_by_name(name, false)
}

fn codec_by_name(name: &str, encoder: bool) -> bool {
    let name = CString::new(name).expect("codec name should not contain NUL");
    unsafe {
        if encoder {
            !ffmpeg::ffi::avcodec_find_encoder_by_name(name.as_ptr()).is_null()
        } else {
            !ffmpeg::ffi::avcodec_find_decoder_by_name(name.as_ptr()).is_null()
        }
    }
}

pub(crate) fn scaled_pixel_format(
    _codec: EncoderCodec,
    backend: EncoderBackend,
    subsampling: ChromaSubsampling,
    bit_depth: u8,
) -> Result<ffmpeg::util::format::pixel::Pixel> {
    use ffmpeg::util::format::pixel::Pixel;
    let pixel = match (backend, subsampling, bit_depth) {
        (EncoderBackend::Cpu | EncoderBackend::Auto, ChromaSubsampling::S420, 8) => Pixel::YUV420P,
        (EncoderBackend::Cpu | EncoderBackend::Auto, ChromaSubsampling::S422, 8) => Pixel::YUV422P,
        (EncoderBackend::Cpu | EncoderBackend::Auto, ChromaSubsampling::S420, 10) => {
            Pixel::YUV420P10LE
        }
        (EncoderBackend::Cpu | EncoderBackend::Auto, ChromaSubsampling::S422, 10) => {
            Pixel::YUV422P10LE
        }
        (EncoderBackend::Nvidia | EncoderBackend::Vaapi, ChromaSubsampling::S420, 8) => Pixel::NV12,
        (EncoderBackend::Nvidia | EncoderBackend::Vaapi, ChromaSubsampling::S422, 8) => Pixel::NV16,
        (EncoderBackend::Nvidia | EncoderBackend::Vaapi, ChromaSubsampling::S420, 10) => {
            Pixel::P010LE
        }
        (EncoderBackend::Nvidia | EncoderBackend::Vaapi, ChromaSubsampling::S422, 10) => {
            Pixel::P210LE
        }
        (_, _, depth) => {
            return Err(EncoderError::message(format!(
                "unsupported bit_depth {depth} (must be 8 or 10)"
            )));
        }
    };
    Ok(pixel)
}

pub(crate) fn encoder_pixel_format(
    codec: EncoderCodec,
    backend: EncoderBackend,
    subsampling: ChromaSubsampling,
    bit_depth: u8,
) -> Result<ffmpeg::util::format::pixel::Pixel> {
    if backend == EncoderBackend::Vaapi {
        return Ok(ffmpeg::util::format::pixel::Pixel::VAAPI);
    }
    scaled_pixel_format(codec, backend, subsampling, bit_depth)
}

pub(crate) fn resolve_chroma_subsampling(
    codec_name: &str,
    backend: EncoderBackend,
    requested: ChromaSubsampling,
    process_id: &str,
) -> ChromaSubsampling {
    if requested == ChromaSubsampling::S420 {
        return ChromaSubsampling::S420;
    }
    if backend == EncoderBackend::Vaapi {
        eprintln!(
            "rollio-encoder: process={process_id} downgrading chroma_subsampling to 4:2:0 (vaapi)"
        );
        return ChromaSubsampling::S420;
    }
    let Some(codec) = ffmpeg::encoder::find_by_name(codec_name) else {
        return ChromaSubsampling::S420;
    };
    let Ok(codec_video) = codec.video() else {
        return ChromaSubsampling::S420;
    };
    let Some(formats) = codec_video.formats() else {
        return ChromaSubsampling::S420;
    };
    let wanted = match backend {
        EncoderBackend::Cpu | EncoderBackend::Auto => ffmpeg::util::format::pixel::Pixel::YUV422P,
        EncoderBackend::Nvidia => ffmpeg::util::format::pixel::Pixel::NV16,
        EncoderBackend::Vaapi => unreachable!("vaapi handled above"),
    };
    if formats.into_iter().any(|fmt| fmt == wanted) {
        ChromaSubsampling::S422
    } else {
        eprintln!(
            "rollio-encoder: process={process_id} downgrading chroma_subsampling to 4:2:0 (codec={codec_name} no 4:2:2)"
        );
        ChromaSubsampling::S420
    }
}

pub(crate) fn resolve_bit_depth(
    codec_name: &str,
    backend: EncoderBackend,
    subsampling: ChromaSubsampling,
    requested: u8,
    process_id: &str,
) -> u8 {
    if requested == 8 {
        return 8;
    }
    if requested != 10 {
        eprintln!("rollio-encoder: process={process_id} unexpected bit_depth={requested}, using 8");
        return 8;
    }
    if backend == EncoderBackend::Vaapi {
        eprintln!("rollio-encoder: process={process_id} downgrading bit_depth to 8 (vaapi)");
        return 8;
    }
    if codec_name == "h264_nvenc" {
        eprintln!(
            "rollio-encoder: process={process_id} downgrading bit_depth to 8 (h264_nvenc has no 10-bit)"
        );
        return 8;
    }
    let Some(codec) = ffmpeg::encoder::find_by_name(codec_name) else {
        return 8;
    };
    let Ok(codec_video) = codec.video() else {
        return 8;
    };
    let Some(formats) = codec_video.formats() else {
        return 8;
    };
    let wanted = match (backend, subsampling) {
        (EncoderBackend::Cpu | EncoderBackend::Auto, ChromaSubsampling::S420) => {
            ffmpeg::util::format::pixel::Pixel::YUV420P10LE
        }
        (EncoderBackend::Cpu | EncoderBackend::Auto, ChromaSubsampling::S422) => {
            ffmpeg::util::format::pixel::Pixel::YUV422P10LE
        }
        (EncoderBackend::Nvidia, ChromaSubsampling::S420) => {
            ffmpeg::util::format::pixel::Pixel::P010LE
        }
        (EncoderBackend::Nvidia, ChromaSubsampling::S422) => {
            ffmpeg::util::format::pixel::Pixel::P210LE
        }
        (EncoderBackend::Vaapi, _) => unreachable!("vaapi handled above"),
    };
    if formats.into_iter().any(|fmt| fmt == wanted) {
        10
    } else {
        eprintln!(
            "rollio-encoder: process={process_id} downgrading bit_depth to 8 (codec={codec_name} no 10-bit)"
        );
        8
    }
}

pub(crate) fn color_space_metadata(
    color_space: EncoderColorSpace,
) -> Option<(
    ffmpeg::ffi::AVColorPrimaries,
    ffmpeg::ffi::AVColorTransferCharacteristic,
    ffmpeg::ffi::AVColorSpace,
)> {
    use ffmpeg::ffi::AVColorPrimaries::*;
    use ffmpeg::ffi::AVColorSpace::*;
    use ffmpeg::ffi::AVColorTransferCharacteristic::*;
    match color_space {
        EncoderColorSpace::Auto => None,
        EncoderColorSpace::Bt709Limited => {
            Some((AVCOL_PRI_BT709, AVCOL_TRC_BT709, AVCOL_SPC_BT709))
        }
        EncoderColorSpace::Bt601Limited => Some((
            AVCOL_PRI_SMPTE170M,
            AVCOL_TRC_SMPTE170M,
            AVCOL_SPC_SMPTE170M,
        )),
    }
}

pub(crate) fn build_codec_options(
    codec_name: &str,
    backend: EncoderBackend,
    crf: Option<u8>,
    preset: Option<&str>,
    tune: Option<&str>,
) -> ffmpeg::Dictionary<'static> {
    let mut opts = ffmpeg::Dictionary::new();
    if let Some(preset) = preset {
        opts.set("preset", preset);
    }
    if let Some(tune) = tune {
        opts.set("tune", tune);
    }
    if let Some(crf) = crf {
        let crf_str = crf.to_string();
        match (codec_name, backend) {
            (
                "h264_nvenc" | "hevc_nvenc" | "av1_nvenc",
                EncoderBackend::Nvidia | EncoderBackend::Auto,
            ) => {
                opts.set("rc", "vbr");
                opts.set("cq", &crf_str);
            }
            (_, EncoderBackend::Vaapi) => {
                opts.set("rc_mode", "CQP");
                opts.set("qp", &crf_str);
            }
            _ => {
                opts.set("crf", &crf_str);
            }
        }
    }
    opts
}

pub(crate) fn create_hw_device(backend: EncoderBackend) -> Result<AvBufferRef> {
    let device_type = backend_hw_device_type(backend)
        .ok_or_else(|| EncoderError::message("requested backend does not use a hardware device"))?;
    let mut device_ref = ptr::null_mut();
    let _vaapi_cstring: Option<CString> = if backend == EncoderBackend::Vaapi {
        vaapi_device_cstring()?
    } else {
        None
    };
    let device_name = if backend == EncoderBackend::Vaapi {
        _vaapi_cstring
            .as_ref()
            .map(|c| c.as_ptr())
            .unwrap_or(ptr::null())
    } else {
        ptr::null()
    };
    let error = unsafe {
        ffmpeg::ffi::av_hwdevice_ctx_create(
            &mut device_ref,
            device_type,
            device_name,
            ptr::null_mut(),
            0,
        )
    };
    if error < 0 {
        return Err(ffmpeg::Error::from(error).into());
    }
    AvBufferRef::new(device_ref, "create hardware device")
}

pub(crate) fn create_hw_frames_context(
    device: &AvBufferRef,
    hw_format: ffmpeg::util::format::pixel::Pixel,
    sw_format: ffmpeg::util::format::pixel::Pixel,
    width: u32,
    height: u32,
    initial_pool_size: i32,
) -> Result<AvBufferRef> {
    let frames_ref = unsafe { ffmpeg::ffi::av_hwframe_ctx_alloc(device.as_ptr()) };
    let frames_ref = AvBufferRef::new(frames_ref, "allocate hardware frames context")?;
    unsafe {
        let context = (*frames_ref.as_ptr()).data as *mut ffmpeg::ffi::AVHWFramesContext;
        if context.is_null() {
            return Err(EncoderError::message(
                "hardware frames context pointer was null",
            ));
        }
        (*context).format = hw_format.into();
        (*context).sw_format = sw_format.into();
        (*context).width = width as i32;
        (*context).height = height as i32;
        (*context).initial_pool_size = initial_pool_size;
        let result = ffmpeg::ffi::av_hwframe_ctx_init(frames_ref.as_ptr());
        if result < 0 {
            return Err(ffmpeg::Error::from(result).into());
        }
    }
    Ok(frames_ref)
}

pub(crate) fn upload_hw_frame(
    hw_frames: &AvBufferRef,
    sw_frame: &ffmpeg::frame::Video,
    hw_format: ffmpeg::util::format::pixel::Pixel,
) -> Result<ffmpeg::frame::Video> {
    let mut hw_frame = ffmpeg::frame::Video::empty();
    hw_frame.set_format(hw_format);
    hw_frame.set_width(sw_frame.width());
    hw_frame.set_height(sw_frame.height());
    hw_frame.set_pts(sw_frame.pts());
    unsafe {
        (*hw_frame.as_mut_ptr()).hw_frames_ctx = hw_frames.clone_raw()?;
        let result =
            ffmpeg::ffi::av_hwframe_get_buffer(hw_frames.as_ptr(), hw_frame.as_mut_ptr(), 0);
        if result < 0 {
            return Err(ffmpeg::Error::from(result).into());
        }
        let result =
            ffmpeg::ffi::av_hwframe_transfer_data(hw_frame.as_mut_ptr(), sw_frame.as_ptr(), 0);
        if result < 0 {
            return Err(ffmpeg::Error::from(result).into());
        }
    }
    Ok(hw_frame)
}

pub(crate) fn backend_hw_device_type(
    backend: EncoderBackend,
) -> Option<ffmpeg::ffi::AVHWDeviceType> {
    Some(match backend {
        EncoderBackend::Nvidia => ffmpeg::ffi::AVHWDeviceType::AV_HWDEVICE_TYPE_CUDA,
        EncoderBackend::Vaapi => ffmpeg::ffi::AVHWDeviceType::AV_HWDEVICE_TYPE_VAAPI,
        EncoderBackend::Cpu | EncoderBackend::Auto => return None,
    })
}

const DRM_VENDOR_NVIDIA: &str = "0x10de";

fn vaapi_device_cstring() -> Result<Option<CString>> {
    if let Ok(over) = std::env::var("ROLLIO_VAAPI_DRI") {
        let p = over.trim();
        if !p.is_empty() {
            if !Path::new(p).exists() {
                return Err(EncoderError::message(format!(
                    "ROLLIO_VAAPI_DRI={p:?} does not exist"
                )));
            }
            return Ok(Some(CString::new(p).map_err(|e| {
                EncoderError::message(format!("invalid ROLLIO_VAAPI_DRI: {e}"))
            })?));
        }
    }
    if let Some(path) = first_non_nvidia_render_node_path() {
        return Ok(Some(CString::new(path).map_err(|e| {
            EncoderError::message(format!("invalid VAAPI DRI path: {e}"))
        })?));
    }
    Ok(None)
}

fn list_drm_render_d_nodes() -> Vec<String> {
    let Ok(dir) = fs::read_dir("/dev/dri") else {
        return Vec::new();
    };
    let mut names: Vec<String> = dir
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("renderD") && n.len() > 7)
        .collect();
    names.sort_by_key(|n| n[7..].parse::<u32>().unwrap_or(u32::MAX));
    names
}

fn drm_device_vendor_id(drm_name: &str) -> Option<String> {
    let path = format!("/sys/class/drm/{drm_name}/device/vendor");
    fs::read_to_string(&path).ok().map(|s| s.trim().to_string())
}

fn is_nvidia_drm_node(drm_name: &str) -> bool {
    drm_device_vendor_id(drm_name).is_some_and(|v| v.eq_ignore_ascii_case(DRM_VENDOR_NVIDIA))
}

fn first_non_nvidia_render_node_path() -> Option<String> {
    for name in list_drm_render_d_nodes() {
        if is_nvidia_drm_node(&name) {
            continue;
        }
        let path = format!("/dev/dri/{name}");
        if Path::new(&path).exists() {
            return Some(path);
        }
    }
    None
}

pub(crate) fn pixel_format_for_libav(
    pixel_format: PixelFormat,
) -> Result<ffmpeg::util::format::pixel::Pixel> {
    match pixel_format {
        PixelFormat::Rgb24 => Ok(ffmpeg::util::format::pixel::Pixel::RGB24),
        PixelFormat::Bgr24 => Ok(ffmpeg::util::format::pixel::Pixel::BGR24),
        PixelFormat::Gray8 => Ok(ffmpeg::util::format::pixel::Pixel::GRAY8),
        PixelFormat::Yuyv => Ok(ffmpeg::util::format::pixel::Pixel::YUYV422),
        PixelFormat::Mjpeg => Err(EncoderError::message(
            "MJPEG frames are decoded via libav's MJPEG decoder, not via direct AVFrame copy",
        )),
        PixelFormat::Depth16 => Err(EncoderError::message(
            "depth16 frames are only supported via the RVL backend",
        )),
    }
}

pub(crate) fn validate_source_pixel_format(pixel_format: PixelFormat) -> Result<()> {
    match pixel_format {
        PixelFormat::Rgb24
        | PixelFormat::Bgr24
        | PixelFormat::Gray8
        | PixelFormat::Yuyv
        | PixelFormat::Mjpeg => Ok(()),
        PixelFormat::Depth16 => Err(EncoderError::message(
            "depth16 frames are only supported via the RVL backend",
        )),
    }
}

pub(crate) fn set_swscale_color_range_to_mpeg(
    scaler: &mut ffmpeg::software::scaling::context::Context,
    source_pixel: ffmpeg::util::format::pixel::Pixel,
    scale_pixel: ffmpeg::util::format::pixel::Pixel,
) -> Result<()> {
    use ffmpeg::ffi as f;
    let table = unsafe { f::sws_getCoefficients(f::SWS_CS_ITU601) };
    let src_full_range = matches!(
        source_pixel,
        ffmpeg::util::format::pixel::Pixel::YUVJ420P
            | ffmpeg::util::format::pixel::Pixel::YUVJ422P
            | ffmpeg::util::format::pixel::Pixel::YUVJ444P
    ) as i32;
    let dst_full_range = matches!(
        scale_pixel,
        ffmpeg::util::format::pixel::Pixel::YUVJ420P
            | ffmpeg::util::format::pixel::Pixel::YUVJ422P
            | ffmpeg::util::format::pixel::Pixel::YUVJ444P
    ) as i32;
    let result = unsafe {
        f::sws_setColorspaceDetails(
            scaler.as_mut_ptr(),
            table,
            src_full_range,
            table,
            dst_full_range,
            0,
            65_536,
            65_536,
        )
    };
    if result < 0 {
        return Err(EncoderError::message(format!(
            "sws_setColorspaceDetails failed (rc={result})"
        )));
    }
    Ok(())
}

/// Validate that an incoming frame is compatible with a codec session
/// configured for `(width, height)`.
///
/// `allow_rescale` controls whether dim drift is acceptable:
///
/// * `false` — recording / RVL sessions reject dim changes outright.
///   The recording-side muxer cannot deal with a mid-stream resize, and
///   surfacing a hard error has historically caught camera-driver bugs
///   that would otherwise produce a silently-corrupt episode.
/// * `true` — preview-encoded sessions accept arbitrary source dims and
///   leave the swscale rescale to the codec session. Pixel-format
///   changes are still rejected because mid-stream pixel-format swaps
///   indicate a driver bug regardless of role.
pub(crate) fn ensure_frame_compatibility(
    header: &CameraFrameHeader,
    width: u32,
    height: u32,
    allow_rescale: bool,
) -> Result<()> {
    if !allow_rescale && (header.width != width || header.height != height) {
        return Err(EncoderError::message(format!(
            "frame dimensions changed during recording: expected {}x{}, got {}x{}",
            width, height, header.width, header.height
        )));
    }
    Ok(())
}

pub(crate) fn copy_frame_payload(
    frame: &mut ffmpeg::frame::Video,
    header: &CameraFrameHeader,
    payload: &[u8],
) -> Result<()> {
    let bytes_per_pixel = match header.pixel_format {
        PixelFormat::Rgb24 | PixelFormat::Bgr24 => 3,
        PixelFormat::Yuyv => 2,
        PixelFormat::Gray8 => 1,
        other => {
            return Err(EncoderError::message(format!(
                "unsupported libav source format for direct AVFrame copy: {:?}",
                other
            )))
        }
    };
    let row_bytes = header.width as usize * bytes_per_pixel;
    let stride = frame.stride(0);
    let expected_bytes = row_bytes * header.height as usize;
    if payload.len() < expected_bytes {
        return Err(EncoderError::message(format!(
            "{:?} payload too short: expected at least {} bytes for {}x{}, got {}",
            header.pixel_format,
            expected_bytes,
            header.width,
            header.height,
            payload.len()
        )));
    }
    for row in 0..header.height as usize {
        let src_offset = row * row_bytes;
        let dst_offset = row * stride;
        frame.data_mut(0)[dst_offset..dst_offset + row_bytes]
            .copy_from_slice(&payload[src_offset..src_offset + row_bytes]);
    }
    Ok(())
}

pub fn decode_artifact(path: &Path, codec: EncoderCodec) -> Result<DecodedArtifact> {
    decode_artifact_with_backend(path, codec, EncoderBackend::Cpu)
}

pub fn decode_artifact_with_backend(
    path: &Path,
    codec: EncoderCodec,
    backend: EncoderBackend,
) -> Result<DecodedArtifact> {
    match codec {
        EncoderCodec::Rvl => decode_rvl_artifact(path),
        EncoderCodec::H264 | EncoderCodec::H265 | EncoderCodec::Av1 | EncoderCodec::Mjpg => {
            decode_video_artifact(path, codec, backend)
        }
    }
}

fn decode_video_artifact(
    path: &Path,
    codec: EncoderCodec,
    backend: EncoderBackend,
) -> Result<DecodedArtifact> {
    ensure_ffmpeg_initialized()?;
    let mut input = ffmpeg::format::input(path)?;
    let stream = input
        .streams()
        .best(ffmpeg::media::Type::Video)
        .ok_or_else(|| EncoderError::message("video stream not found"))?;
    let stream_index = stream.index();
    let mut hw_device = None;
    let mut decoder = if backend == EncoderBackend::Cpu || backend == EncoderBackend::Auto {
        let context = ffmpeg::codec::context::Context::from_parameters(stream.parameters())?;
        context.decoder().video()?
    } else {
        let decoder_name = select_decoder_name(codec, backend).ok_or_else(|| {
            EncoderError::message(format!(
                "decoder backend {:?} for {} is not available",
                backend,
                codec.as_str()
            ))
        })?;
        let decoder_codec = ffmpeg::decoder::find_by_name(decoder_name)
            .ok_or_else(|| EncoderError::message(format!("decoder {decoder_name} not found")))?;
        let mut context = ffmpeg::codec::context::Context::new_with_codec(decoder_codec);
        context.set_parameters(stream.parameters())?;
        if let Some(_device_type) = backend_hw_device_type(backend) {
            let device = create_hw_device(backend)?;
            unsafe {
                (*context.as_mut_ptr()).hw_device_ctx = device.clone_raw()?;
            }
            hw_device = Some(device);
        }
        context.decoder().open_as(decoder_codec)?.video()?
    };
    let mut scaler = None;
    let mut summary = DecodedArtifact {
        width: decoder.width(),
        height: decoder.height(),
        ..DecodedArtifact::default()
    };
    for (packet_stream, packet) in input.packets() {
        if packet_stream.index() != stream_index {
            continue;
        }
        decoder.send_packet(&packet)?;
        drain_decoder(&mut decoder, &mut scaler, &mut summary)?;
    }
    decoder.send_eof()?;
    drain_decoder(&mut decoder, &mut scaler, &mut summary)?;
    drop(hw_device);
    Ok(summary)
}

fn drain_decoder(
    decoder: &mut ffmpeg::decoder::Video,
    scaler: &mut Option<ffmpeg::software::scaling::context::Context>,
    summary: &mut DecodedArtifact,
) -> Result<()> {
    let mut decoded = ffmpeg::frame::Video::empty();
    while decoder.receive_frame(&mut decoded).is_ok() {
        if is_hardware_pixel(decoded.format()) {
            let mut sw_frame = ffmpeg::frame::Video::new(
                decoder_sw_pixel(decoder),
                decoded.width(),
                decoded.height(),
            );
            unsafe {
                let result = ffmpeg::ffi::av_hwframe_transfer_data(
                    sw_frame.as_mut_ptr(),
                    decoded.as_ptr(),
                    0,
                );
                if result < 0 {
                    return Err(ffmpeg::Error::from(result).into());
                }
            }
            process_decoded_frame(&sw_frame, scaler, summary)?;
        } else {
            process_decoded_frame(&decoded, scaler, summary)?;
        }
    }
    Ok(())
}

fn process_decoded_frame(
    frame: &ffmpeg::frame::Video,
    scaler: &mut Option<ffmpeg::software::scaling::context::Context>,
    summary: &mut DecodedArtifact,
) -> Result<()> {
    if scaler.is_none() {
        *scaler = Some(ffmpeg::software::scaling::context::Context::get(
            frame.format(),
            frame.width(),
            frame.height(),
            ffmpeg::util::format::pixel::Pixel::RGB24,
            frame.width(),
            frame.height(),
            ffmpeg::software::scaling::flag::Flags::BILINEAR,
        )?);
    }
    let mut rgb = ffmpeg::frame::Video::empty();
    scaler
        .as_mut()
        .expect("scaler should be initialized")
        .run(frame, &mut rgb)?;
    let bytes = compact_rgb_frame(&rgb);
    if summary.first_rgb_frame.is_none() {
        summary.first_rgb_frame = Some(bytes.clone());
    }
    summary.last_rgb_frame = Some(bytes);
    summary.frame_count += 1;
    Ok(())
}

fn is_hardware_pixel(pixel: ffmpeg::util::format::pixel::Pixel) -> bool {
    matches!(
        pixel,
        ffmpeg::util::format::pixel::Pixel::CUDA | ffmpeg::util::format::pixel::Pixel::VAAPI
    )
}

fn decoder_sw_pixel(decoder: &ffmpeg::decoder::Video) -> ffmpeg::util::format::pixel::Pixel {
    unsafe { ffmpeg::util::format::pixel::Pixel::from((*decoder.as_ptr()).sw_pix_fmt) }
}

fn compact_rgb_frame(frame: &ffmpeg::frame::Video) -> Vec<u8> {
    let row_bytes = frame.width() as usize * 3;
    let stride = frame.stride(0);
    let mut output = vec![0u8; row_bytes * frame.height() as usize];
    for row in 0..frame.height() as usize {
        let src_offset = row * stride;
        let dst_offset = row * row_bytes;
        output[dst_offset..dst_offset + row_bytes]
            .copy_from_slice(&frame.data(0)[src_offset..src_offset + row_bytes]);
    }
    output
}

#[allow(dead_code)]
pub fn maybe_test_encode_delay() {
    let delay_ms = std::env::var("ROLLIO_ENCODER_TEST_ENCODE_DELAY_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0);
    if delay_ms > 0 {
        std::thread::sleep(Duration::from_millis(delay_ms));
    }
}

fn decode_rvl_artifact(path: &Path) -> Result<DecodedArtifact> {
    let mut reader = BufReader::new(File::open(path)?);
    let mut magic = [0u8; 4];
    reader.read_exact(&mut magic)?;
    if &magic != RVL_MAGIC {
        return Err(EncoderError::message(format!(
            "invalid RVL stream magic in {}",
            path.display()
        )));
    }
    let width = read_u32(&mut reader)?;
    let height = read_u32(&mut reader)?;
    let _fps = read_u32(&mut reader)?;
    let frame_len = (width as usize) * (height as usize);
    let mut decoder = DepthDecoder::rvl(frame_len);
    let mut summary = DecodedArtifact {
        width,
        height,
        ..DecodedArtifact::default()
    };
    loop {
        let Some(_timestamp_us) = read_optional_u64(&mut reader)? else {
            break;
        };
        let _frame_index = read_u64(&mut reader)?;
        let payload_len = read_u32(&mut reader)? as usize;
        let mut payload = vec![0u8; payload_len];
        reader.read_exact(&mut payload)?;
        let frame = RvlEncodedFrame::new(RvlCodecKind::Rvl, RvlFrameKind::Key, frame_len, payload);
        let decoded = decoder.decode(&frame)?;
        if summary.first_depth_frame.is_none() {
            summary.first_depth_frame = Some(decoded.clone());
        }
        summary.last_depth_frame = Some(decoded);
        summary.frame_count += 1;
    }
    Ok(summary)
}

fn read_u32<R: Read>(reader: &mut R) -> Result<u32> {
    let mut bytes = [0u8; 4];
    reader.read_exact(&mut bytes)?;
    Ok(u32::from_le_bytes(bytes))
}

fn read_u64<R: Read>(reader: &mut R) -> Result<u64> {
    let mut bytes = [0u8; 8];
    reader.read_exact(&mut bytes)?;
    Ok(u64::from_le_bytes(bytes))
}

fn read_optional_u64<R: Read>(reader: &mut R) -> Result<Option<u64>> {
    let mut bytes = [0u8; 8];
    match reader.read_exact(&mut bytes) {
        Ok(()) => Ok(Some(u64::from_le_bytes(bytes))),
        Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => Ok(None),
        Err(error) => Err(error.into()),
    }
}

pub(crate) fn compute_pts_us(
    frame_timestamp_us: u64,
    recording_start_us: u64,
    last_pts_us: &mut Option<i64>,
    nonmonotonic_warned: &mut bool,
) -> Option<i64> {
    let raw_pts =
        i64::try_from(frame_timestamp_us).ok()? - i64::try_from(recording_start_us).ok()?;
    if raw_pts < 0 {
        return None;
    }
    let pts = match *last_pts_us {
        Some(last) if raw_pts <= last => {
            if !*nonmonotonic_warned {
                *nonmonotonic_warned = true;
                eprintln!(
                    "rollio-encoder: warning: non-monotonic frame timestamp \
                     (raw={raw_pts} us, last={last} us); bumping by 1 us. \
                     Subsequent occurrences silenced."
                );
            }
            last + 1
        }
        _ => raw_pts,
    };
    *last_pts_us = Some(pts);
    Some(pts)
}

#[cfg(test)]
mod pts_tests {
    use super::*;

    #[test]
    fn pre_recording_frame_returns_none() {
        let mut last_pts_us = None;
        let mut warned = false;
        assert_eq!(
            compute_pts_us(999_900, 1_000_000, &mut last_pts_us, &mut warned),
            None
        );
        assert_eq!(last_pts_us, None);
        assert!(!warned);
    }

    #[test]
    fn typical_increasing_timestamps_yield_relative_us_pts() {
        let mut last_pts_us = None;
        let mut warned = false;
        assert_eq!(
            compute_pts_us(1_001_000, 1_000_000, &mut last_pts_us, &mut warned),
            Some(1_000)
        );
        assert_eq!(
            compute_pts_us(1_034_000, 1_000_000, &mut last_pts_us, &mut warned),
            Some(34_000)
        );
        assert!(!warned);
    }

    #[test]
    fn duplicate_timestamp_bumped_by_one_us() {
        let mut last_pts_us = None;
        let mut warned = false;
        assert_eq!(
            compute_pts_us(1_010_000, 1_000_000, &mut last_pts_us, &mut warned),
            Some(10_000)
        );
        assert_eq!(
            compute_pts_us(1_010_000, 1_000_000, &mut last_pts_us, &mut warned),
            Some(10_001)
        );
        assert!(warned);
    }
}
