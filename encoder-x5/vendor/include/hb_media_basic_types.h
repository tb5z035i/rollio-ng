/*
 * Horizon Robotics X5 Multimedia SDK — Basic types
 *
 * Vendored subset for the rollio encoder shim.
 * Source: Horizon Linux BSP libmultimedia headers.
 * SPDX-License-Identifier: Proprietary (Horizon Robotics)
 */
#ifndef HB_MEDIA_BASIC_TYPES_H
#define HB_MEDIA_BASIC_TYPES_H

#include <stdint.h>
#include <stddef.h>

/* Codec identifiers */
typedef enum {
    MEDIA_CODEC_ID_NONE = 0,
    MEDIA_CODEC_ID_H264 = 1,
    MEDIA_CODEC_ID_H265 = 2,
    MEDIA_CODEC_ID_MJPEG = 3,
} media_codec_id_t;

/* Pixel format for input frames */
typedef enum {
    MC_PIXEL_FORMAT_NONE = 0,
    MC_PIXEL_FORMAT_NV12 = 1,
    MC_PIXEL_FORMAT_NV21 = 2,
    MC_PIXEL_FORMAT_YUV420P = 3,
    MC_PIXEL_FORMAT_YUYV = 4,
} mc_pixel_format_t;

/* Video stream buffer — output from encoder */
typedef struct {
    uint8_t *vir_ptr;
    uint64_t phy_ptr;
    uint32_t size;
    uint64_t pts;
    uint32_t stream_index;
    int32_t  flags;  /* key frame if & 1 */
} mc_video_stream_buffer_t;

/* AV frame buffer — input to encoder */
typedef struct {
    uint8_t *vir_ptr[3];   /* plane pointers: [0]=Y, [1]=UV (NV12) */
    uint64_t phy_ptr[3];
    uint32_t stride[3];
    uint32_t width;
    uint32_t height;
    mc_pixel_format_t pix_fmt;
    uint64_t pts;
} mc_av_frame_buffer_t;

/* Codec parameters for configure */
typedef struct {
    media_codec_id_t codec_id;
    mc_pixel_format_t pix_fmt;
    uint32_t width;
    uint32_t height;
    uint32_t frame_rate;
    uint32_t bit_rate;       /* bps, 0 = VBR default */
    uint32_t gop_size;       /* I-frame interval */
    int32_t  quality;        /* MJPEG quality 1-100, ignored for H264 */
} mc_video_codec_params_t;

/* Opaque codec context handle */
typedef void *mc_context_t;

#endif /* HB_MEDIA_BASIC_TYPES_H */
