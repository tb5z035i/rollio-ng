// SPDX-License-Identifier: Apache-2.0
//
// rollio-device-umi entry point. Implements the standard rollio device
// CLI contract (probe / validate / capabilities / query / run) over a
// FastDDS->iceoryx2 bridge configured via the rollio BinaryDeviceConfig
// TOML shape.

#include "bridge.hpp"
#include "config.hpp"

#include "rollio/device_config.hpp"

#include <atomic>
#include <csignal>
#include <iostream>
#include <optional>
#include <stdexcept>
#include <string>
#include <string_view>
#include <vector>

namespace {

constexpr const char* kSingletonId = "umi";

void print_usage() {
    std::cerr << "Usage: rollio-device-umi <probe|validate|capabilities|query|run> [args...]\n"
              << "  probe [--json]\n"
              << "  validate [--json] [--channel-type <type>]... <id>\n"
              << "  capabilities <id>\n"
              << "  query [--json] [--config-inline <toml>] <id>\n"
              << "  run (--config <path> | --config-inline <toml>) [--dry-run]\n";
}

std::optional<std::string> optional_arg(int argc, char* argv[], const std::string& name) {
    for (int i = 0; i + 1 < argc; ++i) {
        if (name == argv[i]) {
            return std::string(argv[i + 1]);
        }
    }
    return std::nullopt;
}

bool has_flag(int argc, char* argv[], const std::string& name) {
    for (int i = 0; i < argc; ++i) {
        if (name == argv[i]) {
            return true;
        }
    }
    return false;
}

std::string json_escape(std::string_view value) {
    std::string out;
    out.reserve(value.size());
    for (char ch : value) {
        switch (ch) {
            case '\\':
                out += "\\\\";
                break;
            case '"':
                out += "\\\"";
                break;
            case '\n':
                out += "\\n";
                break;
            case '\r':
                out += "\\r";
                break;
            case '\t':
                out += "\\t";
                break;
            default:
                out.push_back(ch);
        }
    }
    return out;
}

int handle_probe(int argc, char* argv[]) {
    const bool json = has_flag(argc, argv, "--json");
    if (json) {
        std::cout << "[\"" << kSingletonId << "\"]\n";
    } else {
        std::cout << kSingletonId << "\n";
    }
    return 0;
}

int handle_capabilities(int /*argc*/, char* /*argv*/[]) {
    std::cout << "rollio-device-umi: bridge for cora's FastDDS topics.\n"
              << "Configure via [[channels]] in the device TOML; bridge-specific\n"
              << "knobs (dds_topic, [dds]) live in flattened extras. See\n"
              << "devices/umi/README.md for the full schema.\n";
    return 0;
}

int handle_validate(int argc, char* argv[]) {
    bool json = false;
    std::string id;
    std::vector<std::string> channel_types;
    for (int i = 0; i < argc; ++i) {
        std::string_view arg(argv[i]);
        if (arg == "--json") {
            json = true;
        } else if (arg == "--channel-type") {
            if (i + 1 >= argc) {
                throw std::runtime_error("--channel-type requires a value");
            }
            channel_types.emplace_back(argv[i + 1]);
            ++i;
        } else if (!arg.empty() && arg.front() == '-') {
            throw std::runtime_error(std::string("unknown flag: ") + std::string(arg));
        } else {
            if (!id.empty()) {
                throw std::runtime_error("validate expects a single device id");
            }
            id = std::string(arg);
        }
    }
    if (id.empty()) {
        throw std::runtime_error("validate requires an id (typically \"umi\")");
    }
    const bool valid = id == kSingletonId;
    if (json) {
        std::cout << "{\"valid\":" << (valid ? "true" : "false") << ",\"id\":\""
                  << json_escape(id) << "\",\"driver\":\"umi\",\"channel_types\":[";
        for (size_t i = 0; i < channel_types.size(); ++i) {
            if (i) {
                std::cout << ",";
            }
            std::cout << "\"" << json_escape(channel_types[i]) << "\"";
        }
        std::cout << "]}\n";
    } else {
        std::cout << id << (valid ? " is valid\n" : " is invalid\n");
    }
    return 0;
}

int handle_query(int argc, char* argv[]) {
    bool json = false;
    std::string id;
    std::optional<std::string> config_inline;
    for (int i = 0; i < argc; ++i) {
        std::string_view arg(argv[i]);
        if (arg == "--json") {
            json = true;
        } else if (arg == "--config-inline") {
            if (i + 1 >= argc) {
                throw std::runtime_error("--config-inline requires a value");
            }
            config_inline = std::string(argv[i + 1]);
            ++i;
        } else if (!arg.empty() && arg.front() == '-') {
            throw std::runtime_error(std::string("unknown flag: ") + std::string(arg));
        } else {
            if (!id.empty()) {
                throw std::runtime_error("query expects a single device id");
            }
            id = std::string(arg);
        }
    }
    if (id.empty()) {
        throw std::runtime_error("query requires an id (typically \"umi\")");
    }

    // Build the channels list from the optional config. Without a
    // config, channels are empty so the controller naturally skips us
    // during setup-wizard rendering.
    std::vector<::rollio::DeviceChannelConfigV2> channels;
    if (config_inline) {
        try {
            const auto device = ::rollio::parse_binary_device_config(*config_inline);
            channels = device.channels;
        } catch (const std::exception& e) {
            std::cerr << "rollio-device-umi: query --config-inline parse failed: " << e.what()
                      << "\n";
            return 2;
        }
    }

    if (json) {
        std::cout << "{\"driver\":\"umi\",\"devices\":[{\"id\":\"" << json_escape(id)
                  << "\",\"device_class\":\"umi\",\"device_label\":\"UMI bridge\","
                  << "\"default_device_name\":\"umi\","
                  << "\"optional_info\":{},\"channels\":[";
        for (size_t i = 0; i < channels.size(); ++i) {
            if (i) {
                std::cout << ",";
            }
            const auto& ch = channels[i];
            const char* kind_str = ::rollio::device_kind_to_string(ch.kind);
            const std::string ros_kind =
                ch.kind == ::rollio::DeviceKind::Camera ? "camera"
                : ch.kind == ::rollio::DeviceKind::Imu  ? "imu"
                                                        : "robot";
            std::cout << "{\"channel_type\":\"" << json_escape(ch.channel_type)
                      << "\",\"kind\":\"" << ros_kind << "\",\"available\":true";
            // Cameras include a profile array with a single H264 profile
            // matching the operator's input. IMU has no profile.
            if (ch.kind == ::rollio::DeviceKind::Camera && ch.profile.has_value()) {
                std::cout << ",\"profiles\":[{"
                          << "\"width\":" << ch.profile->width
                          << ",\"height\":" << ch.profile->height
                          << ",\"fps\":" << ch.profile->fps
                          << ",\"pixel_format\":\""
                          << ::rollio::pixel_format_to_string(ch.profile->pixel_format) << "\"}]";
            }
            (void)kind_str;
            std::cout << "}";
        }
        std::cout << "]}]}\n";
    } else {
        std::cout << id << " (umi bridge): " << channels.size() << " configured channel(s)\n";
        for (const auto& ch : channels) {
            std::cout << "  - " << ch.channel_type << " ["
                      << ::rollio::device_kind_to_string(ch.kind) << "]\n";
        }
    }
    return 0;
}

std::atomic<bool>* g_stop_flag = nullptr;
void handle_signal(int /*sig*/) {
    if (g_stop_flag) {
        g_stop_flag->store(true);
    }
}

int handle_run(int argc, char* argv[]) {
    const auto config_path = optional_arg(argc, argv, "--config");
    const auto config_inline = optional_arg(argc, argv, "--config-inline");
    if (config_path.has_value() == config_inline.has_value()) {
        throw std::runtime_error("run requires exactly one of --config or --config-inline");
    }
    const bool dry_run = has_flag(argc, argv, "--dry-run");

    const auto device = config_inline.has_value()
                            ? ::rollio::parse_binary_device_config(*config_inline)
                            : ::rollio::load_binary_device_config_from_file(*config_path);
    const auto bridge_config = ::umi_bridge::resolve_bridge_config(device);

    if (dry_run) {
        std::cerr << "rollio-device-umi: dry-run ok bus_root=" << bridge_config.bus_root
                  << " cameras=" << bridge_config.cameras.size()
                  << " imus=" << bridge_config.imus.size() << "\n";
        for (const auto& cam : bridge_config.cameras) {
            std::cerr << "  camera channel_type=" << cam.channel_type
                      << " dds_topic=" << cam.dds_topic << " size=" << cam.width << "x"
                      << cam.height << "\n";
        }
        for (const auto& imu : bridge_config.imus) {
            std::cerr << "  imu channel_type=" << imu.channel_type
                      << " dds_topic=" << imu.dds_topic << "\n";
        }
        return 0;
    }

    std::atomic<bool> stop{false};
    g_stop_flag = &stop;
    std::signal(SIGINT, handle_signal);
    std::signal(SIGTERM, handle_signal);

    return ::umi_bridge::run_bridge(bridge_config, stop);
}

}  // namespace

int main(int argc, char* argv[]) {
    if (argc < 2) {
        print_usage();
        return 2;
    }
    const std::string subcommand = argv[1];
    try {
        if (subcommand == "probe") {
            return handle_probe(argc - 2, argv + 2);
        }
        if (subcommand == "validate") {
            return handle_validate(argc - 2, argv + 2);
        }
        if (subcommand == "capabilities") {
            return handle_capabilities(argc - 2, argv + 2);
        }
        if (subcommand == "query") {
            return handle_query(argc - 2, argv + 2);
        }
        if (subcommand == "run") {
            return handle_run(argc - 2, argv + 2);
        }
        print_usage();
        return 2;
    } catch (const std::exception& e) {
        std::cerr << "rollio-device-umi: " << e.what() << "\n";
        return 1;
    }
}
