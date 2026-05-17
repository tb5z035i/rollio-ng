/*
 * horizon_x5_ffi.c — Thin C shim wrapping the Horizon X5 libmultimedia
 * codec API for the Rust encoder backend.
 *
 * Why a C shim instead of raw bindgen?
 * - The API is small (8 functions) and stable.
 * - A shim lets us handle buffer lifecycle and error mapping in C,
 *   exposing a flat, Rust-friendly interface without unsafe pointer
 *   gymnastics on the Rust side.
 * - Compiled by `cc` crate in build.rs (feature-gated on `horizon-x5`).
 */

#include "hb_media_codec.h"
#include <string.h>
#include <stdlib.h>

/* ─── Opaque session handle ─────────────────────────────────────── */

typedef struct {
    mc_context_t ctx;
    mc_video_codec_params_t params;
    int started;
} x5_encoder_t;

/* ─── Public FFI surface (called from Rust via extern "C") ──────── */

/**
 * Create and configure an encoder session.
 * Returns NULL on failure; caller must call x5_encoder_destroy().
 */
x5_encoder_t *x5_encoder_create(
    int32_t codec_id,       /* media_codec_id_t */
    uint32_t width,
    uint32_t height,
    uint32_t frame_rate,
    uint32_t bit_rate,
    uint32_t gop_size,
    int32_t quality         /* MJPEG quality 1-100; 0 = default */
) {
    x5_encoder_t *enc = (x5_encoder_t *)calloc(1, sizeof(x5_encoder_t));
    if (!enc) return NULL;

    int32_t ret = hb_mm_mc_initialize(&enc->ctx, (media_codec_id_t)codec_id, 1);
    if (ret != HB_MEDIA_OK) {
        free(enc);
        return NULL;
    }

    enc->params.codec_id   = (media_codec_id_t)codec_id;
    enc->params.pix_fmt    = MC_PIXEL_FORMAT_NV12;
    enc->params.width      = width;
    enc->params.height     = height;
    enc->params.frame_rate = frame_rate;
    enc->params.bit_rate   = bit_rate;
    enc->params.gop_size   = gop_size;
    enc->params.quality    = quality;

    ret = hb_mm_mc_configure(enc->ctx, &enc->params);
    if (ret != HB_MEDIA_OK) {
        hb_mm_mc_release(enc->ctx);
        free(enc);
        return NULL;
    }

    ret = hb_mm_mc_start(enc->ctx);
    if (ret != HB_MEDIA_OK) {
        hb_mm_mc_release(enc->ctx);
        free(enc);
        return NULL;
    }

    enc->started = 1;
    return enc;
}

/**
 * Encode one NV12 frame. Writes encoded bytes into `out_buf` (up to
 * `out_cap` bytes). Returns the number of bytes written, or negative
 * on error. Sets *out_is_key = 1 if the output is a keyframe.
 */
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
    if (!enc || !enc->started) return HB_MEDIA_ERR_INVALID;

    /* Dequeue an input buffer from the VPU pool */
    mc_av_frame_buffer_t frame;
    memset(&frame, 0, sizeof(frame));

    int32_t ret = hb_mm_mc_dequeue_input_buffer(enc->ctx, &frame, 1000);
    if (ret != HB_MEDIA_OK) return ret;

    /* Copy NV12 data into the VPU buffer.
     * Y plane: height rows of y_stride bytes.
     * UV plane: height/2 rows of uv_stride bytes. */
    uint32_t y_size = y_stride * height;
    uint32_t uv_size = uv_stride * (height / 2);

    if (frame.vir_ptr[0]) memcpy(frame.vir_ptr[0], y_plane, y_size);
    if (frame.vir_ptr[1]) memcpy(frame.vir_ptr[1], uv_plane, uv_size);

    frame.stride[0] = y_stride;
    frame.stride[1] = uv_stride;
    frame.width     = width;
    frame.height    = height;
    frame.pix_fmt   = MC_PIXEL_FORMAT_NV12;
    frame.pts       = pts;

    ret = hb_mm_mc_queue_input_buffer(enc->ctx, &frame);
    if (ret != HB_MEDIA_OK) return ret;

    /* Dequeue the encoded output */
    mc_video_stream_buffer_t stream;
    memset(&stream, 0, sizeof(stream));

    ret = hb_mm_mc_dequeue_output_buffer(enc->ctx, &stream, 1000);
    if (ret != HB_MEDIA_OK) return ret;

    /* Copy encoded data to caller's buffer */
    uint32_t copy_size = stream.size;
    if (copy_size > out_cap) copy_size = out_cap;
    if (stream.vir_ptr && copy_size > 0) {
        memcpy(out_buf, stream.vir_ptr, copy_size);
    }

    if (out_is_key) *out_is_key = (stream.flags & 1) ? 1 : 0;

    /* Return the output buffer to the pool */
    hb_mm_mc_queue_output_buffer(enc->ctx, &stream);

    return (int32_t)copy_size;
}

/**
 * Destroy the encoder session and release all VPU resources.
 */
void x5_encoder_destroy(x5_encoder_t *enc) {
    if (!enc) return;
    if (enc->started) {
        hb_mm_mc_pause(enc->ctx);
    }
    hb_mm_mc_release(enc->ctx);
    free(enc);
}
