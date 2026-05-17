/*
 * horizon_x5_ffi.c — C shim wrapping the Horizon X5 libmultimedia
 * codec API for the Rust encoder backend.
 *
 * The vendored headers under encoder-x5/vendor/include/ are copied
 * verbatim from the X5 BSP (/usr/include/hb_media_*.h) so the struct
 * layouts and function signatures match the runtime library exactly.
 *
 * Lifecycle per session:
 *   memset(ctx) + populate fields -> initialize -> configure ->
 *     start -> (encode loop) -> pause -> release
 *
 * Per-frame:
 *   dequeue_input_buffer -> memcpy NV12 -> queue_input_buffer ->
 *     dequeue_output_buffer -> memcpy bitstream -> queue_output_buffer
 *
 * Note: `hb_mm_mc_get_default_context` cannot be used on the X5
 * wave521cl — its defaults include GOP presets / B-frames the
 * hardware rejects at start time. The BSP samples
 * (multimedia_samples/sample_codec/) build the context from scratch
 * with explicit values; we mirror that pattern below.
 */

#include "hb_media_codec.h"
#include "hb_media_error.h"
#include <string.h>
#include <stdlib.h>

/* Rust-facing codec IDs (stable across BSP revisions). */
#define X5_CODEC_H264  0
#define X5_CODEC_MJPEG 1

typedef struct {
    media_codec_context_t ctx;
    int started;
    int is_h264;
} x5_encoder_t;

static media_codec_id_t map_codec_id(int32_t app_id) {
    switch (app_id) {
        case X5_CODEC_H264:  return MEDIA_CODEC_ID_H264;
        case X5_CODEC_MJPEG: return MEDIA_CODEC_ID_MJPEG;
        default:             return MEDIA_CODEC_ID_NONE;
    }
}

x5_encoder_t *x5_encoder_create(
    int32_t codec_id,       /* X5_CODEC_* */
    uint32_t width,
    uint32_t height,
    uint32_t frame_rate,
    uint32_t bit_rate,
    uint32_t gop_size,      /* unused — X5 only supports gop_preset 1 or 9 */
    int32_t quality         /* unused for now */
) {
    (void)gop_size; (void)quality;

    media_codec_id_t mc_id = map_codec_id(codec_id);
    if (mc_id == MEDIA_CODEC_ID_NONE) return NULL;

    x5_encoder_t *enc = (x5_encoder_t *)calloc(1, sizeof(x5_encoder_t));
    if (!enc) return NULL;
    enc->is_h264 = (mc_id == MEDIA_CODEC_ID_H264);

    memset(&enc->ctx, 0, sizeof(enc->ctx));
    enc->ctx.encoder = 1;
    enc->ctx.codec_id = mc_id;

    mc_video_codec_enc_params_t *p = &enc->ctx.video_enc_params;
    p->width  = (hb_s32)width;
    p->height = (hb_s32)height;
    p->pix_fmt = MC_PIXEL_FORMAT_NV12;
    /* bitstream output buffer must be 1KB-aligned (or 4KB for MJPEG).
     * Sizing at width*height*3/2 matches the BSP sample. */
    uint32_t align = enc->is_h264 ? 0x3ff : 0xfff;
    p->bitstream_buf_size = (width * height * 3 / 2 + align) & ~align;
    p->frame_buf_count = 3;
    p->bitstream_buf_count = 3;
    p->external_frame_buf = 0;
    /* X5 wave521cl: only gop_preset 1 (all-I) and 9 (IPPP) are
     * valid. Use IPPP for streaming — all-I bloats the bitstream and
     * confuses keyframe-driven SPS/PPS prepend logic downstream. */
    p->gop_params.gop_preset_idx = 9;
    p->gop_params.decoding_refresh_type = 2;
    p->rot_degree = MC_CCW_0;
    p->mir_direction = MC_DIRECTION_NONE;
    p->frame_cropping_flag = 0;
    p->enable_user_pts = 1;

    if (enc->is_h264) {
        p->rc_params.mode = MC_AV_RC_MODE_H264CBR;
        /* Defaults cribbed from the BSP sample's get_rc_params(). */
        mc_h264_cbr_params_t *cbr = &p->rc_params.h264_cbr_params;
        cbr->intra_period = 30;
        cbr->intra_qp = 30;
        cbr->bit_rate = bit_rate > 0 ? bit_rate / 1000 : 5000; /* kbps */
        cbr->frame_rate = frame_rate > 0 ? frame_rate : 30;
        cbr->initial_rc_qp = 20;
        cbr->vbv_buffer_size = 3000;
        cbr->mb_level_rc_enalbe = 1;
        cbr->min_qp_I = 8;  cbr->max_qp_I = 50;
        cbr->min_qp_P = 8;  cbr->max_qp_P = 50;
        cbr->min_qp_B = 8;  cbr->max_qp_B = 50;
        cbr->hvs_qp_enable = 1;
        cbr->hvs_qp_scale = 2;
        cbr->max_delta_qp = 10;
        cbr->qp_map_enable = 0;
    } else {
        p->rc_params.mode = MC_AV_RC_MODE_MJPEGFIXQP;
        p->rc_params.mjpeg_fixqp_params.frame_rate = frame_rate > 0 ? frame_rate : 30;
        p->rc_params.mjpeg_fixqp_params.quality_factor = quality > 0 ? quality : 50;
        p->mjpeg_enc_config.restart_interval = width / 16;
    }

    if (hb_mm_mc_initialize(&enc->ctx) != 0) {
        free(enc);
        return NULL;
    }
    if (hb_mm_mc_configure(&enc->ctx) != 0) {
        hb_mm_mc_release(&enc->ctx);
        free(enc);
        return NULL;
    }

    mc_av_codec_startup_params_t startup;
    memset(&startup, 0, sizeof(startup));
    startup.video_enc_startup_params.receive_frame_number = 0; /* infinite */

    if (hb_mm_mc_start(&enc->ctx, &startup) != 0) {
        hb_mm_mc_release(&enc->ctx);
        free(enc);
        return NULL;
    }

    /* Force the first frame to be an IDR so the downstream WebCodecs
     * decoder has a clean entry point. In all-I (gop_preset_idx=1)
     * mode the X5 doesn't otherwise emit IDR slices, only non-IDR
     * I-slices — those are intra-coded but can't reinitialize a
     * stalled or freshly-attached decoder. Errors here are non-fatal:
     * the encoder may still emit an IDR at start of its own accord. */
    if (enc->is_h264) {
        hb_mm_mc_request_idr_frame(&enc->ctx);
    }

    enc->started = 1;
    return enc;
}

int32_t x5_encoder_encode(
    x5_encoder_t *enc,
    const uint8_t *y_plane,
    const uint8_t *uv_plane,
    uint32_t y_stride,
    uint32_t uv_stride,
    uint32_t width,
    uint32_t height,
    uint64_t pts,
    uint8_t *out_buf,
    uint32_t out_cap,
    int32_t *out_is_key
) {
    if (!enc || !enc->started) return HB_MEDIA_ERR_INVALID_PARAMS;

    media_codec_buffer_t in_buf;
    memset(&in_buf, 0, sizeof(in_buf));

    int32_t ret = hb_mm_mc_dequeue_input_buffer(&enc->ctx, &in_buf, 2000);
    if (ret != 0) return ret;

    /* Mirror the BSP sample (multimedia_samples/sample_codec.c): after
     * dequeue, set type + minimal frame metadata, then copy NV12 data
     * contiguously into vir_ptr[0] (Y) and vir_ptr[1] (UV). Stride /
     * vstride / frame_end are managed by the codec and must NOT be
     * overwritten on regular frames — setting frame_end=1 makes the
     * VPU treat the frame as EOS and terminate the venc_feeder. */
    in_buf.type = MC_VIDEO_FRAME_BUFFER;
    in_buf.vframe_buf.width   = (hb_s32)width;
    in_buf.vframe_buf.height  = (hb_s32)height;
    in_buf.vframe_buf.pix_fmt = MC_PIXEL_FORMAT_NV12;
    in_buf.vframe_buf.size    = width * height * 3 / 2;
    in_buf.vframe_buf.pts     = pts;

    uint32_t y_size = y_stride * height;
    uint32_t uv_size = uv_stride * (height / 2);
    if (in_buf.vframe_buf.vir_ptr[0] && y_plane)
        memcpy(in_buf.vframe_buf.vir_ptr[0], y_plane, y_size);
    if (in_buf.vframe_buf.vir_ptr[1] && uv_plane)
        memcpy(in_buf.vframe_buf.vir_ptr[1], uv_plane, uv_size);

    ret = hb_mm_mc_queue_input_buffer(&enc->ctx, &in_buf, 2000);
    if (ret != 0) return ret;

    /* Pull the encoded packet. */
    media_codec_buffer_t out;
    media_codec_output_buffer_info_t info;
    memset(&out, 0, sizeof(out));
    memset(&info, 0, sizeof(info));
    out.type = MC_VIDEO_STREAM_BUFFER;

    ret = hb_mm_mc_dequeue_output_buffer(&enc->ctx, &out, &info, 2000);
    if (ret != 0) return ret;

    uint32_t copy_size = out.vstream_buf.size;
    if (copy_size > out_cap) copy_size = out_cap;
    if (out.vstream_buf.vir_ptr && copy_size > 0)
        memcpy(out_buf, out.vstream_buf.vir_ptr, copy_size);

    if (out_is_key) {
        if (enc->is_h264) {
            /* The BSP's `info.video_stream_info.nalu_type` reports the
             * frame-type (I/P/B/IDR), not the actual NAL unit type
             * present in the bitstream. In gop_preset_idx=1 (all-I)
             * mode the X5 emits non-IDR I-slices (NAL type 1) for
             * every frame after the first, but `nalu_type` still
             * reports MC_H264_NALU_TYPE_I (0). Marking those as
             * keyframes confuses WebCodecs (which requires "key"
             * chunks to contain an actual IDR access unit) and any
             * downstream consumer that does the same.
             *
             * Walk the Annex B bitstream and look for an IDR slice
             * (NAL type 5). */
            *out_is_key = 0;
            uint32_t i = 0;
            while (i + 4 <= copy_size) {
                uint32_t sc;
                if (out_buf[i] == 0 && out_buf[i+1] == 0 && out_buf[i+2] == 0 && out_buf[i+3] == 1) {
                    sc = 4;
                } else if (out_buf[i] == 0 && out_buf[i+1] == 0 && out_buf[i+2] == 1) {
                    sc = 3;
                } else {
                    i++;
                    continue;
                }
                if (i + sc >= copy_size) break;
                uint8_t nal_type = out_buf[i + sc] & 0x1F;
                if (nal_type == 5) {            /* IDR slice */
                    *out_is_key = 1;
                    break;
                } else if (nal_type == 1) {     /* non-IDR slice */
                    break;
                }
                /* SPS(7) / PPS(8) / SEI(6) / AUD(9) — keep scanning. */
                i += sc;
            }
        } else {
            /* MJPEG: every frame is independent. */
            *out_is_key = 1;
        }
    }

    hb_mm_mc_queue_output_buffer(&enc->ctx, &out, 2000);
    return (int32_t)copy_size;
}

void x5_encoder_destroy(x5_encoder_t *enc) {
    if (!enc) return;
    if (enc->started) {
        /* Sample uses pause + release (not stop) for teardown. */
        hb_mm_mc_pause(&enc->ctx);
        enc->started = 0;
    }
    hb_mm_mc_release(&enc->ctx);
    free(enc);
}
