/*
 * Link-time stubs for libmultimedia.
 *
 * These empty function bodies satisfy the linker during cross-compilation.
 * At runtime on the Horizon X5 board, the real libmultimedia.so.1 from the
 * BSP is loaded via LD_LIBRARY_PATH / ld.so.conf — these stubs are never
 * called.
 *
 * If a stub IS accidentally called (e.g. running the binary off-target),
 * it returns HB_MEDIA_ERR_NOT_READY (-5), making the failure obvious.
 */
#include "include/hb_media_basic_types.h"
#include "include/hb_media_error.h"

int32_t hb_mm_mc_initialize(mc_context_t *ctx, media_codec_id_t id, int32_t enc) {
    (void)ctx; (void)id; (void)enc; return HB_MEDIA_ERR_NOT_READY;
}
int32_t hb_mm_mc_configure(mc_context_t ctx, const mc_video_codec_params_t *p) {
    (void)ctx; (void)p; return HB_MEDIA_ERR_NOT_READY;
}
int32_t hb_mm_mc_start(mc_context_t ctx) {
    (void)ctx; return HB_MEDIA_ERR_NOT_READY;
}
int32_t hb_mm_mc_pause(mc_context_t ctx) {
    (void)ctx; return HB_MEDIA_ERR_NOT_READY;
}
int32_t hb_mm_mc_release(mc_context_t ctx) {
    (void)ctx; return HB_MEDIA_ERR_NOT_READY;
}
int32_t hb_mm_mc_dequeue_input_buffer(mc_context_t ctx, mc_av_frame_buffer_t *f, int32_t t) {
    (void)ctx; (void)f; (void)t; return HB_MEDIA_ERR_NOT_READY;
}
int32_t hb_mm_mc_queue_input_buffer(mc_context_t ctx, const mc_av_frame_buffer_t *f) {
    (void)ctx; (void)f; return HB_MEDIA_ERR_NOT_READY;
}
int32_t hb_mm_mc_dequeue_output_buffer(mc_context_t ctx, mc_video_stream_buffer_t *s, int32_t t) {
    (void)ctx; (void)s; (void)t; return HB_MEDIA_ERR_NOT_READY;
}
int32_t hb_mm_mc_queue_output_buffer(mc_context_t ctx, const mc_video_stream_buffer_t *s) {
    (void)ctx; (void)s; return HB_MEDIA_ERR_NOT_READY;
}
