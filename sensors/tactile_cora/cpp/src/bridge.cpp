// Lifecycle of the C ABI bridge. One context per device process; owns the singleton
// DDS participant and serialises subscription registration.

#include "cora_bridge.h"

#include <atomic>
#include <chrono>
#include <memory>
#include <mutex>
#include <string>
#include <vector>

#include <cora/dds/callback_executor.h>
#include <cora/dds/dds_participant.h>

#include "subscriber.h"

struct cora_bridge_ctx {
    cora_bridge_config_t config;
    std::string participant_name;
    std::atomic<bool> running{false};

    std::mutex subscriptions_mutex;
    std::vector<std::unique_ptr<CoraSubscription>> subscriptions;
    std::atomic<uint32_t> next_sub_id{0};
};

namespace {
std::mutex& process_init_mutex() {
    static std::mutex m;
    return m;
}

std::atomic<int> active_contexts{0};
}  // namespace

cora_bridge_ctx_t* cora_bridge_create(const cora_bridge_config_t* config) {
    if (!config || !config->participant_name) {
        return nullptr;
    }
    std::lock_guard<std::mutex> lock(process_init_mutex());

    auto ctx = std::make_unique<cora_bridge_ctx_t>();
    ctx->config = *config;
    ctx->participant_name = config->participant_name;
    ctx->config.participant_name = ctx->participant_name.c_str();

    auto& participant = framework::dds::DDSParticipant::instance();
    if (!participant.isInitialized()) {
        framework::dds::DDSConfig dcfg;
        dcfg.domain_id = config->domain_id;
        dcfg.participant_name = ctx->participant_name;
        dcfg.use_shared_memory = config->use_shared_memory != 0;
        dcfg.use_udp = config->use_udp != 0;
        if (!participant.initialize(dcfg)) {
            return nullptr;
        }
    }

    active_contexts.fetch_add(1, std::memory_order_acq_rel);
    return ctx.release();
}

int cora_bridge_start(cora_bridge_ctx_t* ctx) {
    if (!ctx) return CORA_BRIDGE_ERR_NULL;
    bool expected = false;
    if (!ctx->running.compare_exchange_strong(expected, true)) {
        return CORA_BRIDGE_ERR_ALREADY_RUNNING;
    }
    auto& executor = framework::CallbackExecutor::instance();
    if (!executor.isRunning()) {
        uint32_t threads = ctx->config.callback_threads > 0 ? ctx->config.callback_threads : 2;
        executor.start(threads);
    }
    return CORA_BRIDGE_OK;
}

int cora_bridge_stop(cora_bridge_ctx_t* ctx) {
    if (!ctx) return CORA_BRIDGE_ERR_NULL;
    bool expected = true;
    if (!ctx->running.compare_exchange_strong(expected, false)) {
        return CORA_BRIDGE_ERR_NOT_RUNNING;
    }
    {
        std::lock_guard<std::mutex> lock(ctx->subscriptions_mutex);
        for (auto& sub : ctx->subscriptions) {
            if (sub) sub->clear();
        }
    }
    return CORA_BRIDGE_OK;
}

void cora_bridge_destroy(cora_bridge_ctx_t* ctx) {
    if (!ctx) return;
    if (ctx->running.load()) {
        cora_bridge_stop(ctx);
    }
    {
        std::lock_guard<std::mutex> lock(ctx->subscriptions_mutex);
        ctx->subscriptions.clear();
    }

    std::lock_guard<std::mutex> lock(process_init_mutex());
    int remaining = active_contexts.fetch_sub(1, std::memory_order_acq_rel) - 1;
    if (remaining == 0) {
        auto& executor = framework::CallbackExecutor::instance();
        if (executor.isRunning()) {
            executor.stop(std::chrono::milliseconds(3000));
        }
        auto& participant = framework::dds::DDSParticipant::instance();
        if (participant.isInitialized()) {
            participant.shutdown();
        }
    }
    delete ctx;
}

int32_t cora_bridge_subscribe_point_cloud2(
    cora_bridge_ctx_t* ctx, const char* topic, int qos_reliable,
    cora_pointcloud_cb_t cb, void* user) {
    if (!ctx || !topic || !cb) return CORA_BRIDGE_ERR_NULL;
    auto sub = make_point_cloud2_subscription(topic, qos_reliable != 0, cb, user);
    if (!sub) return CORA_BRIDGE_ERR_SUBSCRIBE;
    std::lock_guard<std::mutex> lock(ctx->subscriptions_mutex);
    uint32_t id = ctx->next_sub_id.fetch_add(1, std::memory_order_acq_rel);
    sub->set_id(id);
    ctx->subscriptions.push_back(std::move(sub));
    return static_cast<int32_t>(id);
}
