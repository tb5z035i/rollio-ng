// SPDX-License-Identifier: Apache-2.0
//
// UMI bridge runtime — the per-topic FastDDS subscriber / iceoryx2
// publisher fan-out loop.

#pragma once

#include "config.hpp"

#include <atomic>

namespace umi_bridge {

/// Run the bridge until either `ControlEvent::Shutdown` arrives on
/// `control/events` or `stop_flag` is set externally. Returns 0 on a
/// clean shutdown, non-zero on error. Throws on unrecoverable setup
/// failures (e.g. FastDDS DomainParticipant could not be created).
int run_bridge(const UmiBridgeConfig& config, std::atomic<bool>& stop_flag);

}  // namespace umi_bridge
