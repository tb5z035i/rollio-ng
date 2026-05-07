// SPDX-License-Identifier: Apache-2.0

#include "config.hpp"

#include <stdexcept>
#include <string>

namespace umi_bridge {

namespace {

bool to_bool(const std::string& raw, bool fallback) {
    if (raw == "true") {
        return true;
    }
    if (raw == "false") {
        return false;
    }
    return fallback;
}

uint32_t to_u32(const std::string& raw, uint32_t fallback) {
    if (raw.empty()) {
        return fallback;
    }
    try {
        return static_cast<uint32_t>(std::stoul(raw));
    } catch (...) {
        return fallback;
    }
}

const std::string& require_extra(const std::unordered_map<std::string, std::string>& extra,
                                 const std::string& key, const std::string& channel_type) {
    auto it = extra.find(key);
    if (it == extra.end() || it->second.empty()) {
        throw std::runtime_error("UMI bridge channel \"" + channel_type +
                                 "\" requires extra key \"" + key + "\"");
    }
    return it->second;
}

}  // namespace

UmiBridgeConfig resolve_bridge_config(const ::rollio::BinaryDeviceConfig& device) {
    if (device.driver != "umi") {
        throw std::runtime_error("UMI bridge requires driver = \"umi\", got \"" + device.driver +
                                 "\"");
    }
    if (device.bus_root.empty()) {
        throw std::runtime_error("UMI bridge requires non-empty bus_root");
    }

    UmiBridgeConfig out;
    out.bus_root = device.bus_root;

    // DDS settings flattened from `[dds]` table on the Rust side end up as
    // dotted-path keys in device.extra ("dds.domain_id", "dds.use_shm",
    // "dds.use_udp"). Anything missing falls back to the SHM-only default.
    auto find_extra = [&](const std::string& key) -> const std::string* {
        auto it = device.extra.find(key);
        return it == device.extra.end() ? nullptr : &it->second;
    };

    if (const auto* v = find_extra("dds.domain_id")) {
        out.dds.domain_id = to_u32(*v, 0);
    }
    if (const auto* v = find_extra("dds.use_shm")) {
        out.dds.use_shm = to_bool(*v, true);
    }
    if (const auto* v = find_extra("dds.use_udp")) {
        out.dds.use_udp = to_bool(*v, false);
    }

    for (const auto& ch : device.channels) {
        if (!ch.enabled) {
            continue;
        }
        switch (ch.kind) {
            case ::rollio::DeviceKind::Camera: {
                if (!ch.profile.has_value()) {
                    throw std::runtime_error("UMI bridge camera channel \"" + ch.channel_type +
                                             "\" requires a profile");
                }
                if (ch.profile->pixel_format != ::rollio::PixelFormat::H264) {
                    throw std::runtime_error(
                        "UMI bridge camera channel \"" + ch.channel_type +
                        "\" requires pixel_format = \"h264\"; got " +
                        std::string(::rollio::pixel_format_to_string(ch.profile->pixel_format)));
                }
                CameraBridge cam;
                cam.channel_type = ch.channel_type;
                cam.dds_topic = require_extra(ch.extra, "dds_topic", ch.channel_type);
                cam.width = ch.profile->width;
                cam.height = ch.profile->height;
                out.cameras.push_back(std::move(cam));
                break;
            }
            case ::rollio::DeviceKind::Imu: {
                ImuBridge imu;
                imu.channel_type = ch.channel_type;
                imu.dds_topic = require_extra(ch.extra, "dds_topic", ch.channel_type);
                out.imus.push_back(std::move(imu));
                break;
            }
            case ::rollio::DeviceKind::Robot:
                throw std::runtime_error("UMI bridge does not support kind=\"robot\" channels (\"" +
                                         ch.channel_type + "\"); use kind=\"imu\" instead");
        }
    }

    if (out.cameras.empty() && out.imus.empty()) {
        throw std::runtime_error(
            "UMI bridge requires at least one enabled [[channels]] entry (camera or imu)");
    }
    return out;
}

}  // namespace umi_bridge
