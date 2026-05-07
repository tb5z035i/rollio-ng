// SPDX-License-Identifier: Apache-2.0
//
// UMI bridge runtime config. Consumes the standard rollio
// `BinaryDeviceConfig` shape and pulls out the bridge-specific knobs that
// the rollio-types schema flattens into channel.extra / device.extra.

#pragma once

#include "rollio/device_config.hpp"
#include "rollio/types.h"

#include <cstdint>
#include <optional>
#include <string>
#include <vector>

namespace umi_bridge {

struct CameraBridge {
    std::string channel_type;          ///< rollio iceoryx2 channel_type
    std::string dds_topic;             ///< cora-side FastDDS topic (e.g. rt/...)
    uint32_t width{0};                 ///< source resolution; informational
    uint32_t height{0};                ///< source resolution; informational
};

struct ImuBridge {
    std::string channel_type;
    std::string dds_topic;
};

struct DdsTransportConfig {
    uint32_t domain_id{0};
    bool use_shm{true};
    bool use_udp{false};
};

struct UmiBridgeConfig {
    std::string bus_root;
    DdsTransportConfig dds;
    std::vector<CameraBridge> cameras;
    std::vector<ImuBridge> imus;
};

/// Build the bridge runtime config from a parsed `BinaryDeviceConfig`.
/// Throws on missing/invalid fields (e.g. a camera channel without a
/// `dds_topic` extra). The walking pattern matches realsense's
/// `resolve_realsense_camera_channels`.
UmiBridgeConfig resolve_bridge_config(const ::rollio::BinaryDeviceConfig& device);

}  // namespace umi_bridge
