#ifndef ROLLIO_DEVICES_CORACAM_CORA_TYPES_HPP
#define ROLLIO_DEVICES_CORACAM_CORA_TYPES_HPP

// cora_types.hpp — C++ structs and CDR deserializers for the two Cora camera
// message types.
//
// The CDR layout implemented here is XCDR1 little-endian, which is the default
// encoding used by ROS2 / cora / eProsima Fast-DDS.  A 4-byte representation-
// identifier header (0x00 0x01 0x00 0x00) precedes the CDR payload in the DDS
// SerializedPayload_t.  Our deserialize() implementations receive the full
// payload (header + data) and strip the 4 header bytes before parsing.
//
// Actual wire format must be confirmed through on-device testing (see 阶段0 of
// signal_other/analysis/rollio-device/coracam-h264-annexb实施方案.zh.md).
// The parsers are written defensively; a mismatch will cause parse_* to return
// false and the channel worker will log the raw payload for diagnosis.

#include <cstdint>
#include <string>
#include <vector>

namespace rollio::coracam {

// ROS2-style timestamp (sec + nanosec).
struct CoraStamp {
    int32_t sec{0};
    uint32_t nanosec{0};

    // Convert to UNIX microseconds (integer, not floating point).
    [[nodiscard]] uint64_t to_us() const noexcept {
        return static_cast<uint64_t>(sec) * 1'000'000ULL +
               static_cast<uint64_t>(nanosec) / 1'000ULL;
    }
};

// ROS2-style message header.
struct CoraHeader {
    CoraStamp stamp;
    std::string frame_id;
};

// Parsed sensor_msgs/Image (or equivalent cora raw image message).
// Corresponds to cora_raw_image.idl / sensor_msgs::msg::dds_::Image_.
struct CoraRawImage {
    CoraHeader header;
    uint32_t height{0};
    uint32_t width{0};
    std::string encoding;  // e.g. "bgr8"
    uint8_t is_bigendian{0};
    uint32_t step{0};  // row stride in bytes
    std::vector<uint8_t> data;
};

// Parsed cora H264Packet (cora_msgs::msg::dds_::H264Packet_).
// Corresponds to cora_h264_packet.idl.
struct CoraH264Packet {
    CoraHeader header;
    uint32_t width{0};
    uint32_t height{0};
    bool is_keyframe{false};
    std::vector<uint8_t> data;  // complete Annex-B AU bytes
};

// Parsed foxglove_msgs/msg/CompressedVideo (foxglove_msgs::msg::dds_::CompressedVideo_).
// CDR layout (XCDR1 LE): timestamp (sec+nanosec), frame_id (string),
// data (sequence<uint8>), format (string, e.g. "h264").
// No width/height/is_keyframe — keyframe detection relies on NAL scan.
struct FoxgloveCompressedVideo {
    CoraStamp timestamp;
    std::string frame_id;
    std::string format;         // e.g. "h264"
    std::vector<uint8_t> data;  // raw encoded bytes (Annex-B for H.264)
};

// Parse a CoraRawImage from a raw Fast-DDS SerializedPayload_t buffer.
// 'bytes' points to payload.data, 'len' is payload.length.
// The first 4 bytes are the CDR representation identifier (skipped).
// Returns true on success; 'out' is populated.
// Returns false on truncation or parse error; 'out' is left partially filled.
bool parse_cora_raw_image(const uint8_t* bytes, size_t len, CoraRawImage& out);

// Parse a CoraH264Packet from a raw Fast-DDS SerializedPayload_t buffer.
bool parse_cora_h264_packet(const uint8_t* bytes, size_t len, CoraH264Packet& out);

// Parse a FoxgloveCompressedVideo from a raw Fast-DDS SerializedPayload_t buffer.
bool parse_foxglove_compressed_video(const uint8_t* bytes, size_t len,
                                     FoxgloveCompressedVideo& out);

// Dump the first min(len,64) bytes of 'bytes' as hex to stderr.
// Used for diagnostics when a parse fails.
void dump_cdr_hex(const char* label, const uint8_t* bytes, size_t len);

}  // namespace rollio::coracam

#endif  // ROLLIO_DEVICES_CORACAM_CORA_TYPES_HPP
