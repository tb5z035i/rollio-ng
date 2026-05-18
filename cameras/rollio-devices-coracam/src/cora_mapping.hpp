#ifndef ROLLIO_DEVICES_CORACAM_CORA_MAPPING_HPP
#define ROLLIO_DEVICES_CORACAM_CORA_MAPPING_HPP

// cora_mapping.hpp — per-device Cora topic mapping file loader.
//
// The mapping file lets operators override the Coracam → Cora topic / type /
// QoS contract without recompiling the device.  Schema (see方案文档 §5):
//
//   domain_id = 0
//   participant_name = "rollio_coracam_head"
//   max_packet_bytes = 4194304
//   annex_b_validation = "scan"          # scan | metadata | auto
//   metadata_validation_packets = 16
//
//   [[topics]]
//   channel_type = "left_raw"
//   topic        = "/rt/robot/camera/head/left/image"
//   type         = "sensor_msgs::msg::dds_::Image_"
//   max_packet_bytes = 8388608           # per-channel override (optional)
//   raw_expected_encoding = "bgr8"        # raw only
//
// Every field is optional; the loader fills missing values from the device
// descriptor's defaults. Unknown keys are ignored to keep the schema
// forward-compatible. Validation (4 channels present, profile matches, etc.)
// happens in device_main.cpp after merging with the BinaryDeviceConfig.

#include <cstdint>
#include <optional>
#include <string>
#include <vector>

#include "channel_worker.hpp"

namespace rollio::coracam {

struct CoraTopicEntry {
    std::string channel_type;  // "left_raw" | "right_raw" | ...
    std::optional<std::string> topic;
    std::optional<std::string> type;
    std::optional<uint32_t> max_packet_bytes;
    std::optional<std::string> raw_expected_encoding;
    std::optional<AnnexBValidationMode> annex_b_validation;
    std::optional<uint32_t> metadata_validation_packets;
};

struct CoraMapping {
    std::optional<uint32_t> domain_id;
    std::optional<std::string> participant_name;
    std::optional<uint32_t> max_packet_bytes;
    std::optional<AnnexBValidationMode> annex_b_validation;
    std::optional<uint32_t> metadata_validation_packets;
    std::vector<CoraTopicEntry> topics;
};

// Parse a mapping file from in-memory TOML text.
// Throws std::runtime_error on malformed input.
auto parse_cora_mapping(std::string_view text) -> CoraMapping;

// Load and parse a mapping file from disk. Throws on I/O or parse error.
auto load_cora_mapping_from_file(const std::string& path) -> CoraMapping;

// Convert a string into an AnnexBValidationMode.
// Throws std::runtime_error on unknown value.
auto parse_annex_b_validation(std::string_view value) -> AnnexBValidationMode;

// Stringify (for logging / dry-run output).
auto annex_b_validation_to_string(AnnexBValidationMode mode) -> const char*;

}  // namespace rollio::coracam

#endif  // ROLLIO_DEVICES_CORACAM_CORA_MAPPING_HPP
