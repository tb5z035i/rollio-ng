/*
 * Link-time stubs for libmultimedia.
 *
 * These empty function bodies satisfy the linker during cross-compilation.
 * At runtime on the Horizon X5 board, the real libmultimedia.so.1 from the
 * BSP is loaded via LD_LIBRARY_PATH / ld.so.conf — these stubs are never
 * called.
 *
 * Signatures mirror /usr/include/hb_media_codec.h on the X5 BSP. If a
 * stub IS accidentally called (e.g. running the binary off-target),
 * it returns HB_MEDIA_ERR_UNKNOWN, making the failure obvious.
 */
#include "include/hb_media_codec.h"
#include "include/hb_media_error.h"

hb_s32 hb_mm_mc_get_default_context(media_codec_id_t codec_id,
                                    hb_bool encoder,
                                    media_codec_context_t *context) {
    (void)codec_id; (void)encoder; (void)context;
    return HB_MEDIA_ERR_UNKNOWN;
}
hb_s32 hb_mm_mc_initialize(media_codec_context_t *context) {
    (void)context; return HB_MEDIA_ERR_UNKNOWN;
}
hb_s32 hb_mm_mc_configure(media_codec_context_t *context) {
    (void)context; return HB_MEDIA_ERR_UNKNOWN;
}
hb_s32 hb_mm_mc_start(media_codec_context_t *context,
                      const mc_av_codec_startup_params_t *info) {
    (void)context; (void)info; return HB_MEDIA_ERR_UNKNOWN;
}
hb_s32 hb_mm_mc_stop(media_codec_context_t *context) {
    (void)context; return HB_MEDIA_ERR_UNKNOWN;
}
hb_s32 hb_mm_mc_pause(media_codec_context_t *context) {
    (void)context; return HB_MEDIA_ERR_UNKNOWN;
}
hb_s32 hb_mm_mc_release(media_codec_context_t *context) {
    (void)context; return HB_MEDIA_ERR_UNKNOWN;
}
hb_s32 hb_mm_mc_queue_input_buffer(media_codec_context_t *context,
                                   media_codec_buffer_t *buffer,
                                   hb_s32 timeout) {
    (void)context; (void)buffer; (void)timeout; return HB_MEDIA_ERR_UNKNOWN;
}
hb_s32 hb_mm_mc_dequeue_input_buffer(media_codec_context_t *context,
                                     media_codec_buffer_t *buffer,
                                     hb_s32 timeout) {
    (void)context; (void)buffer; (void)timeout; return HB_MEDIA_ERR_UNKNOWN;
}
hb_s32 hb_mm_mc_queue_output_buffer(media_codec_context_t *context,
                                    media_codec_buffer_t *buffer,
                                    hb_s32 timeout) {
    (void)context; (void)buffer; (void)timeout; return HB_MEDIA_ERR_UNKNOWN;
}
hb_s32 hb_mm_mc_dequeue_output_buffer(media_codec_context_t *context,
                                      media_codec_buffer_t *buffer,
                                      media_codec_output_buffer_info_t *info,
                                      hb_s32 timeout) {
    (void)context; (void)buffer; (void)info; (void)timeout;
    return HB_MEDIA_ERR_UNKNOWN;
}
hb_s32 hb_mm_mc_request_idr_frame(media_codec_context_t *context) {
    (void)context; return HB_MEDIA_ERR_UNKNOWN;
}
hb_s32 hb_mm_strerror(hb_s32 err_num, hb_string err_buf, size_t errbuf_size) {
    (void)err_num; (void)err_buf; (void)errbuf_size; return 0;
}
