/*
 * Horizon Robotics X5 Multimedia SDK — Codec API
 *
 * Vendored subset for the rollio encoder shim.
 * Source: Horizon Linux BSP libmultimedia headers.
 * SPDX-License-Identifier: Proprietary (Horizon Robotics)
 */
#ifndef HB_MEDIA_CODEC_H
#define HB_MEDIA_CODEC_H

#include "hb_media_basic_types.h"
#include "hb_media_error.h"

#ifdef __cplusplus
extern "C" {
#endif

/*
 * Lifecycle:
 *   initialize -> configure -> start -> (encode loop) -> pause -> release
 *
 * Encode loop (per frame):
 *   dequeue_input_buffer -> fill NV12 planes -> queue_input_buffer
 *   -> dequeue_output_buffer -> read vstream -> queue_output_buffer
 */

/* Create a codec context for encoding */
int32_t hb_mm_mc_initialize(mc_context_t *context, media_codec_id_t codec_id,
                            int32_t is_encoder);

/* Configure codec parameters (call before start) */
int32_t hb_mm_mc_configure(mc_context_t context,
                           const mc_video_codec_params_t *params);

/* Start the codec pipeline */
int32_t hb_mm_mc_start(mc_context_t context);

/* Pause (stop accepting new frames) */
int32_t hb_mm_mc_pause(mc_context_t context);

/* Release all resources */
int32_t hb_mm_mc_release(mc_context_t context);

/* Dequeue an empty input buffer to fill with frame data.
 * timeout_ms: -1 = block, 0 = poll, >0 = wait up to N ms */
int32_t hb_mm_mc_dequeue_input_buffer(mc_context_t context,
                                      mc_av_frame_buffer_t *frame,
                                      int32_t timeout_ms);

/* Submit a filled input buffer for encoding */
int32_t hb_mm_mc_queue_input_buffer(mc_context_t context,
                                    const mc_av_frame_buffer_t *frame);

/* Dequeue an encoded output buffer.
 * timeout_ms: -1 = block, 0 = poll, >0 = wait up to N ms */
int32_t hb_mm_mc_dequeue_output_buffer(mc_context_t context,
                                       mc_video_stream_buffer_t *stream,
                                       int32_t timeout_ms);

/* Return an output buffer to the codec pool */
int32_t hb_mm_mc_queue_output_buffer(mc_context_t context,
                                     const mc_video_stream_buffer_t *stream);

#ifdef __cplusplus
}
#endif

#endif /* HB_MEDIA_CODEC_H */
