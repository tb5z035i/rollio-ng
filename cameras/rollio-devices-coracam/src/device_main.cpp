#include "device_main.hpp"

#include <cora/dds/dds_participant.h>

#include <algorithm>
#include <chrono>
#include <cstdlib>
#include <fstream>
#include <iostream>
#include <memory>
#include <optional>
#include <sstream>
#include <stdexcept>
#include <string>
#include <string_view>
#include <thread>
#include <vector>

#include "channel_worker.hpp"
#include "cora_mapping.hpp"
#include "device_descriptor.hpp"
#include "iox2/iceoryx2.hpp"
#include "rollio/device_config.hpp"
#include "rollio/topic_names.hpp"
#include "rollio/types.h"

namespace rollio::coracam {

using SteadyClock = std::chrono::steady_clock;

namespace {

// ---------------------------------------------------------------------------
// CLI helpers
// ---------------------------------------------------------------------------

auto optional_arg(int argc, char* argv[], std::string_view name) -> std::optional<std::string> {
    for (int i = 0; i + 1 < argc; ++i) {
        if (name == argv[i]) {
            return std::string(argv[i + 1]);
        }
    }
    return std::nullopt;
}

auto has_flag(int argc, char* argv[], std::string_view name) -> bool {
    for (int i = 0; i < argc; ++i) {
        if (name == argv[i]) {
            return true;
        }
    }
    return false;
}

auto json_escape(std::string_view value) -> std::string {
    std::string out;
    out.reserve(value.size());
    for (const char ch : value) {
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

// ROS 2 CLI tools display DDS topic names with a leading slash, e.g.
// "/rt/robot/camera/...", while camera_node registers Fast-DDS writers as
// "rt/robot/camera/...". Fast-DDS EDP matching compares topic_name exactly.
auto normalize_cora_dds_topic(std::string topic) -> std::string {
    constexpr std::string_view ros_rt_prefix = "/rt/";
    if (topic.rfind(ros_rt_prefix, 0) == 0) {
        topic.erase(topic.begin());
    }
    return topic;
}

// ---------------------------------------------------------------------------
// Channel definitions — fixed for the current coracam revision.
// ---------------------------------------------------------------------------

struct CoracamChannel {
    const char* channel_type;
    const char* label;
    ChannelKind kind;
    const char* pixel_format_name;
    // Suffix appended to DeviceDescriptor::default_cora_topic_prefix to form
    // the full DDS topic name. The DDS type name is determined by `kind`.
    const char* dds_topic_suffix;
};

// Four fixed channels per device (section 4.1 of the方案 document).
constexpr CoracamChannel kChannels[] = {
    {"left_raw", "Left Eye Raw", ChannelKind::RawBgr24, "bgr24", kLeftRawTopicSuffix},
    {"right_raw", "Right Eye Raw", ChannelKind::RawBgr24, "bgr24", kRightRawTopicSuffix},
    {"left_h264", "Left Eye H264", ChannelKind::H264AnnexB, "h264-annex-b", kLeftH264TopicSuffix},
    {"right_h264", "Right Eye H264", ChannelKind::H264AnnexB, "h264-annex-b",
     kRightH264TopicSuffix},
};
constexpr std::size_t kChannelCount = sizeof(kChannels) / sizeof(kChannels[0]);

constexpr uint32_t kDefaultWidth = 640;
constexpr uint32_t kDefaultHeight = 480;
constexpr uint32_t kDefaultFps = 25;
constexpr uint32_t kDefaultDdsShmSegmentSize = 2U * 1024U * 1024U;

// Forward declaration so cmd_validate (defined earlier in the file) can call
// the strict channel-config builder, which is the canonical place schema
// checks live.
auto build_channel_configs(const rollio::BinaryDeviceConfig& config, const DeviceDescriptor& desc,
                           const std::optional<CoraMapping>& mapping)
    -> std::vector<ChannelWorkerConfig>;

// Look up the descriptor for an id, throwing a uniform error message on miss.
auto require_descriptor(std::string_view id) -> const DeviceDescriptor& {
    if (const auto* desc = find_descriptor_by_id(id); desc != nullptr) {
        return *desc;
    }
    std::string msg = std::string(kCoracamProgramName) + ": unknown coracam id '";
    msg.append(id);
    msg += "' (expected one of:";
    for (std::size_t i = 0; i < kDescriptorCount; ++i) {
        msg += " ";
        msg += kAllDescriptors[i]->id;
    }
    msg += ")";
    throw std::runtime_error(msg);
}

// ---------------------------------------------------------------------------
// probe
// ---------------------------------------------------------------------------

auto cmd_probe() -> int {
    // Single executable exposes all physical Coracam mount points. The
    // controller's discovery layer picks each entry up as a distinct device.
    std::cout << '[';
    for (std::size_t i = 0; i < kDescriptorCount; ++i) {
        const auto& desc = *kAllDescriptors[i];
        if (i > 0) {
            std::cout << ',';
        }
        std::cout << "{"
                  << "\"id\":\"" << json_escape(desc.id) << "\","
                  << "\"name\":\"" << json_escape(desc.default_name) << "\","
                  << "\"driver\":\"" << json_escape(kCoracamDriver) << "\","
                  << "\"type\":\"camera\""
                  << "}";
    }
    std::cout << "]\n";
    return 0;
}

// ---------------------------------------------------------------------------
// validate
// ---------------------------------------------------------------------------

auto cmd_validate(int argc, char* argv[]) -> int {
    const bool json = has_flag(argc, argv, "--json");
    const auto config_path = optional_arg(argc, argv, "--config");
    const auto mapping_path = optional_arg(argc, argv, "--mapping");

    // The id to validate is the last non-flag positional argument.
    std::string id;
    bool skip_next = false;
    for (int i = 0; i < argc; ++i) {
        const std::string_view arg(argv[i]);
        if (skip_next) {
            skip_next = false;
            continue;
        }
        if (arg == "--config" || arg == "--mapping") {
            skip_next = true;
            continue;
        }
        if (!arg.empty() && arg.front() != '-') {
            id = std::string(arg);
        }
    }
    if (id.empty()) {
        throw std::runtime_error("validate requires a device id");
    }
    const auto& desc = require_descriptor(id);

    // If --config is supplied perform full schema + mapping validation; this
    // makes `validate` useful to the controller's collect path. Without
    // --config we still respond valid (back-compat with the early skeleton),
    // but with valid=true,checks=skeleton so the operator knows the check
    // was shallow.
    std::vector<std::string> errors;
    std::vector<std::string> warnings;
    std::string mode = "skeleton";

    if (config_path.has_value()) {
        mode = "config";
        try {
            const auto config = rollio::load_binary_device_config_from_file(*config_path);
            std::optional<CoraMapping> mapping;
            if (mapping_path.has_value()) {
                mapping = load_cora_mapping_from_file(*mapping_path);
                mode = "config+mapping";
            }
            // build_channel_configs throws on every schema violation.
            (void)build_channel_configs(config, desc, mapping);
        } catch (const std::exception& ex) {
            errors.emplace_back(ex.what());
        }
    } else if (mapping_path.has_value()) {
        mode = "mapping";
        try {
            (void)load_cora_mapping_from_file(*mapping_path);
        } catch (const std::exception& ex) {
            errors.emplace_back(ex.what());
        }
    }

    const bool valid = errors.empty();
    if (json) {
        std::cout << "{"
                  << "\"valid\":" << (valid ? "true" : "false") << ","
                  << "\"mode\":\"" << json_escape(mode) << "\","
                  << "\"id\":\"" << json_escape(id) << "\","
                  << "\"driver\":\"" << json_escape(kCoracamDriver) << "\","
                  << "\"errors\":[";
        for (std::size_t i = 0; i < errors.size(); ++i) {
            if (i > 0)
                std::cout << ",";
            std::cout << "\"" << json_escape(errors[i]) << "\"";
        }
        std::cout << "]}\n";
    } else if (valid) {
        std::cout << id << " is valid (mode=" << mode << ")\n";
    } else {
        std::cout << id << " is INVALID (mode=" << mode << ")\n";
        for (const auto& e : errors) {
            std::cout << "  - " << e << '\n';
        }
    }
    return valid ? 0 : 1;
}

// ---------------------------------------------------------------------------
// query
// ---------------------------------------------------------------------------

// Emit the DeviceQueryResponse JSON expected by the controller's live query.
// Fixed 4 channels, two raw + two h264, all 640x480@25Hz.
auto cmd_query(int argc, char* argv[]) -> int {
    std::string id;
    for (int i = 0; i < argc; ++i) {
        const std::string_view arg(argv[i]);
        if (!arg.empty() && arg.front() != '-') {
            id = std::string(arg);
        }
    }
    if (id.empty()) {
        throw std::runtime_error("query requires a device id");
    }
    const auto& desc = require_descriptor(id);

    auto emit_channel = [&](const CoracamChannel& ch, bool last) {
        std::cout << "{"
                  << "\"channel_type\":\"" << ch.channel_type << "\","
                  << "\"kind\":\"camera\","
                  << "\"available\":true,"
                  << "\"channel_label\":\"" << ch.label << "\","
                  << "\"default_name\":\"" << desc.default_name << "_" << ch.channel_type << "\","
                  << "\"modes\":[\"enabled\",\"disabled\"],"
                  << "\"profiles\":[{"
                  << "\"width\":" << kDefaultWidth << ","
                  << "\"height\":" << kDefaultHeight << ","
                  << "\"fps\":" << kDefaultFps << ","
                  << "\"pixel_format\":\"" << ch.pixel_format_name << "\""
                  << "}],"
                  << "\"supported_states\":[],"
                  << "\"supported_commands\":[],"
                  << "\"supports_fk\":false,"
                  << "\"supports_ik\":false,"
                  << "\"dof\":null,"
                  << "\"default_control_frequency_hz\":null,"
                  << "\"direct_joint_compatibility\":{\"can_lead\":[],\"can_follow\":[]},"
                  << "\"defaults\":{},"
                  << "\"optional_info\":{\"cora_topic_prefix\":\"" << desc.default_cora_topic_prefix
                  << "\"}"
                  << "}" << (last ? "" : ",");
    };

    std::cout << "{"
              << "\"driver\":\"" << kCoracamDriver << "\","
              << "\"devices\":[{"
              << "\"id\":\"" << desc.id << "\","
              << "\"device_class\":\"coracam\","
              << "\"device_label\":\"" << desc.device_label << "\","
              << "\"default_device_name\":\"" << desc.default_name << "\","
              << "\"optional_info\":{\"cora_topic_prefix\":\"" << desc.default_cora_topic_prefix
              << "\"},"
              << "\"channels\":[";
    for (std::size_t i = 0; i < kChannelCount; ++i) {
        emit_channel(kChannels[i], i + 1 == kChannelCount);
    }
    std::cout << "]}]}\n";
    return 0;
}

// ---------------------------------------------------------------------------
// run
// ---------------------------------------------------------------------------

// Locate the configuration entry for a specific channel_type. Returns
// nullptr if not present (used by strict validation to detect missing
// required channels).
auto find_channel_cfg(const rollio::BinaryDeviceConfig& config, std::string_view channel_type)
    -> const rollio::DeviceChannelConfigV2* {
    for (const auto& ch : config.channels) {
        if (ch.channel_type == channel_type) {
            return &ch;
        }
    }
    return nullptr;
}

// Locate a topic entry in the mapping by channel_type.
auto find_mapping_topic(const CoraMapping& mapping, std::string_view channel_type)
    -> const CoraTopicEntry* {
    for (const auto& t : mapping.topics) {
        if (t.channel_type == channel_type) {
            return &t;
        }
    }
    return nullptr;
}

// Resolve channel worker configuration from a BinaryDeviceConfig. Validates
// that the config has the expected 4 channels and matches the coracam driver.
//
// Strict checks (target方案 §4.1):
//   - driver == descriptor.driver
//   - every channel in config has kind = camera
//   - any enabled fixed channel has the expected camera kind / pixel format
//   - raw channels have pixel_format = bgr24
//   - h264 channels have pixel_format = h264-annex-b
//   - 640 x 480 @ 25 Hz (warn-only for now; preserved via defaults)
//   - channel_type uniqueness
auto build_channel_configs(const rollio::BinaryDeviceConfig& config, const DeviceDescriptor& desc,
                           const std::optional<CoraMapping>& mapping)
    -> std::vector<ChannelWorkerConfig> {
    if (config.driver != kCoracamDriver) {
        throw std::runtime_error(std::string(kCoracamProgramName) + " requires driver = \"" +
                                 kCoracamDriver + "\", got \"" + config.driver + "\"");
    }

    // Channel-type uniqueness inside the BinaryDeviceConfig.
    for (std::size_t i = 0; i < config.channels.size(); ++i) {
        for (std::size_t j = i + 1; j < config.channels.size(); ++j) {
            if (config.channels[i].channel_type == config.channels[j].channel_type) {
                throw std::runtime_error("duplicate channel_type in config: " +
                                         config.channels[i].channel_type);
            }
        }
    }

    // All channels must be camera-kind. Unknown channel_types are tolerated
    // (they will just not be wired) so a future Coracam revision can add new
    // channels without breaking older binaries.
    for (const auto& ch : config.channels) {
        if (ch.kind != rollio::DeviceKind::Camera) {
            throw std::runtime_error("channel '" + ch.channel_type + "' must have kind = camera");
        }
    }

    std::vector<ChannelWorkerConfig> out;
    out.reserve(kChannelCount);

    const bool no_dds = (std::getenv("ROLLIO_CORACAM_NO_DDS") != nullptr);

    // Pull device-level defaults from mapping (if provided).
    const uint32_t mapping_max_packet =
        mapping && mapping->max_packet_bytes ? *mapping->max_packet_bytes : 4U * 1024U * 1024U;
    const uint32_t dds_domain =
        mapping && mapping->domain_id
            ? *mapping->domain_id
            : (config.dds_domain_id ? *config.dds_domain_id : kCoraDdsDomainId);
    const auto mapping_annexb_mode = mapping && mapping->annex_b_validation
                                         ? *mapping->annex_b_validation
                                         : AnnexBValidationMode::Scan;
    const uint32_t mapping_metadata_packets = mapping && mapping->metadata_validation_packets
                                                  ? *mapping->metadata_validation_packets
                                                  : 16U;

    for (const auto& kch : kChannels) {
        const auto* found = find_channel_cfg(config, kch.channel_type);
        if (found == nullptr) {
            continue;
        }
        if (!found->enabled) {
            continue;
        }
        // Profile must be present and match the channel kind.
        if (!found->profile.has_value()) {
            throw std::runtime_error(std::string(kCoracamProgramName) + ": channel '" +
                                     kch.channel_type + "' requires a [channels.profile] table");
        }
        const auto& prof = *found->profile;
        const auto expected_pf = (kch.kind == ChannelKind::H264AnnexB)
                                     ? rollio::PixelFormat::H264AnnexB
                                     : rollio::PixelFormat::Bgr24;
        if (prof.pixel_format != expected_pf) {
            throw std::runtime_error(std::string(kCoracamProgramName) + ": channel '" +
                                     kch.channel_type + "' pixel_format mismatch (expected " +
                                     rollio::pixel_format_to_string(expected_pf) + ")");
        }

        ChannelWorkerConfig wcfg;
        wcfg.channel_type = kch.channel_type;
        wcfg.bus_root = config.bus_root;
        wcfg.service_name = rollio::channel_frames_service_name(config.bus_root, kch.channel_type);
        wcfg.kind = kch.kind;
        wcfg.width = prof.width ? prof.width : kDefaultWidth;
        wcfg.height = prof.height ? prof.height : kDefaultHeight;
        wcfg.fps = prof.fps ? prof.fps : kDefaultFps;

        wcfg.max_payload_bytes = mapping_max_packet;
        wcfg.annex_b_validation = mapping_annexb_mode;
        wcfg.metadata_validation_packets = mapping_metadata_packets;
        // Wire the DDS subscriber side. ROLLIO_CORACAM_NO_DDS=1 disables
        // (used by tests that run without a live Fast-DDS stack).
        if (!no_dds) {
            // Defaults from descriptor:
            std::string topic = std::string(desc.default_cora_topic_prefix) + kch.dds_topic_suffix;
            std::string type = (kch.kind == ChannelKind::H264AnnexB) ? kH264PacketDdsTypeName
                                                                     : kRawImageDdsTypeName;
            uint32_t per_channel_max = mapping_max_packet;

            // Apply per-channel mapping overrides if present.
            if (mapping) {
                if (const auto* t = find_mapping_topic(*mapping, kch.channel_type)) {
                    if (t->topic) {
                        topic = *t->topic;
                    }
                    if (t->type) {
                        type = *t->type;
                    }
                    if (t->max_packet_bytes) {
                        per_channel_max = *t->max_packet_bytes;
                    }
                    if (t->raw_expected_encoding && kch.kind == ChannelKind::RawBgr24) {
                        wcfg.raw_expected_encoding = *t->raw_expected_encoding;
                    }
                    if (t->annex_b_validation && kch.kind == ChannelKind::H264AnnexB) {
                        wcfg.annex_b_validation = *t->annex_b_validation;
                    }
                    if (t->metadata_validation_packets && kch.kind == ChannelKind::H264AnnexB) {
                        wcfg.metadata_validation_packets = *t->metadata_validation_packets;
                    }
                }
            }

            wcfg.dds_topic_name = normalize_cora_dds_topic(std::move(topic));
            wcfg.dds_type_name = type;
            wcfg.dds_domain_id = dds_domain;
            wcfg.max_payload_bytes = per_channel_max;
        }

        out.push_back(std::move(wcfg));
    }

    if (out.empty()) {
        throw std::runtime_error(std::string(kCoracamProgramName) +
                                 ": at least one fixed coracam channel must be enabled");
    }

    // Topic uniqueness across enabled channels (DDS topic + iceoryx2 service).
    for (std::size_t i = 0; i < out.size(); ++i) {
        for (std::size_t j = i + 1; j < out.size(); ++j) {
            if (!out[i].dds_topic_name.empty() && out[i].dds_topic_name == out[j].dds_topic_name) {
                throw std::runtime_error("duplicate DDS topic '" + out[i].dds_topic_name +
                                         "' between channels '" + out[i].channel_type + "' and '" +
                                         out[j].channel_type + "'");
            }
            if (out[i].service_name == out[j].service_name) {
                throw std::runtime_error("duplicate iceoryx2 service '" + out[i].service_name +
                                         "' between channels '" + out[i].channel_type + "' and '" +
                                         out[j].channel_type + "'");
            }
        }
    }

    return out;
}

// Initialize the global Cora DDSParticipant exactly once before any
// ChannelReader is created. Domain id comes from mapping, then generated
// BinaryDeviceConfig, then descriptor default; participant name comes from
// mapping when available, otherwise from the device descriptor.
auto initialize_cora_participant(const rollio::BinaryDeviceConfig& config,
                                 const std::optional<CoraMapping>& mapping,
                                 const DeviceDescriptor& desc) -> void {
    framework::dds::DDSConfig dds_cfg;
    dds_cfg.domain_id =
        static_cast<int>(mapping && mapping->domain_id
                             ? *mapping->domain_id
                             : (config.dds_domain_id ? *config.dds_domain_id : kCoraDdsDomainId));
    dds_cfg.participant_name = (mapping && mapping->participant_name)
                                   ? *mapping->participant_name
                                   : std::string(desc.default_name);
    dds_cfg.use_shared_memory = true;
    dds_cfg.use_udp = true;
    if (config.dds_shm_segment_size && *config.dds_shm_segment_size == 0U) {
        throw std::runtime_error("dds_shm_segment_size must be > 0 when set");
    }
    dds_cfg.shm_segment_size =
        config.dds_shm_segment_size ? *config.dds_shm_segment_size : kDefaultDdsShmSegmentSize;
    if (config.dds_callback_threads) {
        dds_cfg.callback_threads = static_cast<std::size_t>(*config.dds_callback_threads);
    }

    auto& participant = framework::dds::DDSParticipant::instance();
    if (participant.isInitialized()) {
        return;
    }
    std::cerr << kCoracamProgramName << ": Cora DDS participant config domain=" << dds_cfg.domain_id
              << " shm_segment_size=" << dds_cfg.shm_segment_size
              << " callback_threads=" << dds_cfg.callback_threads << '\n';
    if (!participant.initialize(dds_cfg)) {
        throw std::runtime_error("failed to initialize Cora DDSParticipant");
    }
}

auto cmd_run(int argc, char* argv[]) -> int {
    using namespace iox2;

    const bool dry_run = has_flag(argc, argv, "--dry-run");

    const auto config_path = optional_arg(argc, argv, "--config");
    const auto config_inline = optional_arg(argc, argv, "--config-inline");
    if (config_path.has_value() == config_inline.has_value()) {
        throw std::runtime_error("run requires exactly one of --config or --config-inline");
    }

    const auto config = config_inline.has_value()
                            ? rollio::parse_binary_device_config(*config_inline)
                            : rollio::load_binary_device_config_from_file(*config_path);

    const auto& desc = require_descriptor(config.id);

    // Optional Cora mapping file. Path may come from --mapping CLI flag or
    // ROLLIO_CORACAM_MAPPING_FILE env var.
    std::optional<CoraMapping> mapping;
    auto mapping_path = optional_arg(argc, argv, "--mapping");
    if (!mapping_path.has_value()) {
        const char* env = std::getenv("ROLLIO_CORACAM_MAPPING_FILE");
        if (env != nullptr && *env != '\0') {
            mapping_path = std::string(env);
        }
    }
    if (mapping_path.has_value()) {
        mapping = load_cora_mapping_from_file(*mapping_path);
    }

    const auto channel_configs = build_channel_configs(config, desc, mapping);

    if (dry_run) {
        std::cerr << kCoracamProgramName << ": dry-run ok"
                  << " device=" << config.id << " bus_root=" << config.bus_root
                  << " channels=" << channel_configs.size();
        if (mapping_path.has_value()) {
            std::cerr << " mapping=" << *mapping_path;
        }
        std::cerr << '\n';
        for (const auto& wc : channel_configs) {
            std::cerr << "  - " << wc.channel_type << " service=" << wc.service_name
                      << " kind=" << (wc.kind == ChannelKind::H264AnnexB ? "h264-annex-b" : "bgr24")
                      << " size=" << wc.width << "x" << wc.height << " fps=" << wc.fps
                      << " max_packet=" << wc.max_payload_bytes;
            if (wc.kind == ChannelKind::H264AnnexB) {
                std::cerr << " annex_b=" << annex_b_validation_to_string(wc.annex_b_validation)
                          << " meta_pkts=" << wc.metadata_validation_packets;
            } else if (!wc.raw_expected_encoding.empty()) {
                std::cerr << " enc=" << wc.raw_expected_encoding;
            }
            if (!wc.dds_topic_name.empty()) {
                std::cerr << " dds_topic=" << wc.dds_topic_name << " dds_type=" << wc.dds_type_name
                          << " dds_domain=" << wc.dds_domain_id;
            } else {
                std::cerr << " dds=mock";
            }
            std::cerr << '\n';
        }
        return 0;
    }

    // Start control event subscriber to detect shutdown.
    set_log_level_from_env_or(LogLevel::Warn);
    auto node = NodeBuilder().create<ServiceType::Ipc>().value();
    const auto ctrl_name = ServiceName::create(rollio::CONTROL_EVENTS_SERVICE).value();
    auto ctrl_service = node.service_builder(ctrl_name)
                            .publish_subscribe<rollio::ControlEvent>()
                            .open_or_create()
                            .value();
    auto ctrl_sub = ctrl_service.subscriber_builder().create().value();

    // Initialize the Cora SDK DDS participant exactly once before any
    // ChannelReader is constructed by a worker thread. Skipped in the
    // mock path (ROLLIO_CORACAM_NO_DDS=1) where no real reader is created.
    const bool any_dds_channel =
        std::any_of(channel_configs.begin(), channel_configs.end(),
                    [](const ChannelWorkerConfig& wc) { return !wc.dds_topic_name.empty(); });
    if (any_dds_channel) {
        initialize_cora_participant(config, mapping, desc);
    }

    // Start one worker thread per channel.
    std::vector<std::unique_ptr<ChannelWorker>> workers;
    workers.reserve(channel_configs.size());
    for (const auto& wc : channel_configs) {
        auto w = std::make_unique<ChannelWorker>(wc);
        w->start();
        workers.push_back(std::move(w));
    }

    std::cerr << kCoracamProgramName << ": running"
              << " device=" << config.id << " bus_root=" << config.bus_root
              << " channels=" << workers.size() << '\n';

    // Main loop: poll for shutdown control event.
    while (true) {
        auto sample = ctrl_sub.receive().value();
        while (sample.has_value()) {
            if (sample->payload().tag == rollio::ControlEventTag::Shutdown) {
                std::cerr << kCoracamProgramName << ": shutdown received, stopping workers\n";
                for (auto& w : workers) {
                    w->stop();
                }
                // Workers are joined; ChannelReader instances are destroyed.
                // Safe to shut down the global Cora participant now.
                if (any_dds_channel) {
                    framework::dds::DDSParticipant::instance().shutdown();
                }
                std::cerr << kCoracamProgramName << ": stopped\n";
                return 0;
            }
            sample = ctrl_sub.receive().value();
        }
        std::this_thread::sleep_for(std::chrono::milliseconds(50));
    }
}

// ---------------------------------------------------------------------------
// usage
// ---------------------------------------------------------------------------

auto print_usage() -> void {
    std::cerr << "Usage: " << kCoracamProgramName << " <command> [args...]\n"
              << "  probe [--json]\n"
              << "  validate [--json] [--config <path>] [--mapping <path>] <id>\n"
              << "  query [--json] <id>\n"
              << "  run (--config <path> | --config-inline <toml>) [--mapping <path>] [--dry-run]\n"
              << "\nEnvironment:\n"
              << "  ROLLIO_CORACAM_MAPPING_FILE   default --mapping path if unset\n"
              << "  ROLLIO_CORACAM_NO_DDS=1       run with internal mock generator\n";
}

}  // namespace

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

int coracam_main(int argc, char* argv[]) {
    try {
        if (argc < 2) {
            print_usage();
            return 1;
        }

        const std::string command = argv[1];

        if (command == "probe") {
            return cmd_probe();
        }
        if (command == "validate") {
            return cmd_validate(argc - 2, argv + 2);
        }
        if (command == "query") {
            return cmd_query(argc - 2, argv + 2);
        }
        if (command == "run") {
            return cmd_run(argc - 1, argv + 1);
        }

        throw std::runtime_error("unknown subcommand: " + command);
    } catch (const std::exception& ex) {
        std::cerr << kCoracamProgramName << ": " << ex.what() << '\n';
        return 1;
    }
}

}  // namespace rollio::coracam
