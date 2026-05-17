// Flat C ABI for the tactile-cora device's Cora SDK shim.
//
// Lifecycle: `cora_bridge_create` initialises the Fast-DDS participant. A single
// `cora_bridge_subscribe_point_cloud2` call registers a
// `framework::ChannelReader<PointCloud2,...>` whose callback fires on the SDK's
// `CallbackExecutor` worker threads (started by `cora_bridge_start`).
// `cora_bridge_destroy` calls `stop()` internally and shuts the participant down.
// Subscriptions are immutable for the lifetime of the context.
//
// All pointer parameters in callbacks are valid only for the duration of the
// callback — Rust trampolines must copy out anything they need to keep.

#ifndef CORA_BRIDGE_H_
#define CORA_BRIDGE_H_

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct cora_bridge_ctx cora_bridge_ctx_t;

typedef struct {
    int32_t  domain_id;
    const char* participant_name;
    uint8_t  use_shared_memory;
    uint8_t  use_udp;
    uint32_t callback_threads;
} cora_bridge_config_t;

#define CORA_BRIDGE_OK              0
#define CORA_BRIDGE_ERR_NULL        -1
#define CORA_BRIDGE_ERR_DDS_INIT    -2
#define CORA_BRIDGE_ERR_SUBSCRIBE   -3
#define CORA_BRIDGE_ERR_NOT_RUNNING -4
#define CORA_BRIDGE_ERR_ALREADY_RUNNING -5
#define CORA_BRIDGE_ERR_INTERNAL    -100

// Mirrors sensor_msgs::msg::PointField datatype codes: INT8=1, UINT8=2, INT16=3,
// UINT16=4, INT32=5, UINT32=6, FLOAT32=7, FLOAT64=8.
typedef struct {
    const char* name;
    uint32_t    offset;
    uint8_t     datatype;
    uint32_t    count;
} cora_point_field_t;

typedef void (*cora_pointcloud_cb_t)(
    uint32_t sub_id, uint64_t ts_us,
    uint32_t width, uint32_t height,
    uint32_t point_step, uint32_t row_step,
    const cora_point_field_t* fields, size_t n_fields,
    const uint8_t* data, size_t len,
    uint8_t is_bigendian, uint8_t is_dense,
    void* user);

cora_bridge_ctx_t* cora_bridge_create(const cora_bridge_config_t* config);
int  cora_bridge_start(cora_bridge_ctx_t*);
int  cora_bridge_stop(cora_bridge_ctx_t*);
void cora_bridge_destroy(cora_bridge_ctx_t*);

int32_t cora_bridge_subscribe_point_cloud2(
    cora_bridge_ctx_t*, const char* topic, int qos_reliable,
    cora_pointcloud_cb_t cb, void* user);

#ifdef __cplusplus
}  // extern "C"
#endif

#endif  // CORA_BRIDGE_H_
