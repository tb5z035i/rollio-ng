// Flat C ABI for the imu-cora device's Cora SDK shim.
//
// Lifecycle: `cora_bridge_create` initialises the Fast-DDS participant. A single
// `cora_bridge_subscribe_imu` call registers a `framework::ChannelReader<Imu,...>`
// whose callback fires on the SDK's `CallbackExecutor` worker threads (started by
// `cora_bridge_start`). `cora_bridge_destroy` calls `stop()` internally and shuts
// the participant down. Subscriptions are immutable for the lifetime of the
// context.
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
    const char* participant_name;  // nul-terminated; copied internally
    uint8_t  use_shared_memory;    // 0/1
    uint8_t  use_udp;              // 0/1
    uint32_t callback_threads;
} cora_bridge_config_t;

#define CORA_BRIDGE_OK              0
#define CORA_BRIDGE_ERR_NULL        -1
#define CORA_BRIDGE_ERR_DDS_INIT    -2
#define CORA_BRIDGE_ERR_SUBSCRIBE   -3
#define CORA_BRIDGE_ERR_NOT_RUNNING -4
#define CORA_BRIDGE_ERR_ALREADY_RUNNING -5
#define CORA_BRIDGE_ERR_INTERNAL    -100

typedef void (*cora_imu_cb_t)(
    uint32_t sub_id, uint64_t ts_us,
    double ax, double ay, double az,
    double gx, double gy, double gz,
    double qx, double qy, double qz, double qw,
    void* user);

cora_bridge_ctx_t* cora_bridge_create(const cora_bridge_config_t* config);
int  cora_bridge_start(cora_bridge_ctx_t*);
int  cora_bridge_stop(cora_bridge_ctx_t*);
void cora_bridge_destroy(cora_bridge_ctx_t*);

int32_t cora_bridge_subscribe_imu(
    cora_bridge_ctx_t*, const char* topic, int qos_reliable,
    cora_imu_cb_t cb, void* user);

// Standalone DDS discovery: spins up an ephemeral DomainParticipant on
// `domain_id`, installs a DomainParticipantListener, waits `wait_ms` ms
// collecting unique (topic, type) pairs published by remote writers, then
// invokes `cb(topic, type, user)` once per discovered pair before tearing
// the participant down. Does NOT touch the singleton DDSParticipant used
// by the run path. Returns the count of pairs emitted, or a negative
// CORA_BRIDGE_ERR_* code on failure.
typedef void (*cora_topic_cb_t)(const char* topic, const char* type, void* user);

int32_t cora_bridge_discover_topics(
    int32_t domain_id, const char* participant_name,
    uint32_t wait_ms, cora_topic_cb_t cb, void* user);

#ifdef __cplusplus
}  // extern "C"
#endif

#endif  // CORA_BRIDGE_H_
