#ifndef ROLLIO_DEVICES_CORACAM_DEVICE_DESCRIPTOR_HPP
#define ROLLIO_DEVICES_CORACAM_DEVICE_DESCRIPTOR_HPP

#include <cstddef>
#include <cstdint>
#include <string_view>

namespace rollio::coracam {

// One descriptor per physical Coracam mount point. A single
// `rollio-device-camera-cora` executable exposes all descriptors through
// probe/query and selects one at run time from BinaryDeviceConfig.id.
// Topic name suffix conventions are documented in
// signal_other/analysis/rollio-device/cora-topic-to-rollio-bus-device方案.zh.md.
struct DeviceDescriptor {
    // BinaryDeviceConfig.id surfaced by probe and used by run/validate/query
    // to select the physical Coracam mount point.
    const char* id;
    // Default device name -- e.g. "coracam_head". Used to seed the
    // controller's setup wizard; also drives the default bus_root the
    // run command falls back to when the config omits one.
    const char* default_name;
    // Human-readable label returned by query --json.
    const char* device_label;
    // Default cora topic prefix expected for this physical mount point;
    // surfaced via probe / query so the topic mapping file is easier to
    // author by hand.
    const char* default_cora_topic_prefix;
};

// ---------------------------------------------------------------------------
// Cora DDS channel mapping
//
// The cora publisher names every Fast-DDS wire topic as
//     <prefix>/{left,right}/{image,video_encoded}
// where <prefix> is DeviceDescriptor::default_cora_topic_prefix.
//
// camera_node creates writers as `rt/` + configured public topic. ROS 2 CLI
// displays those names with a leading slash, but Fast-DDS EDP matching uses
// the exact wire topic string without it.
// ---------------------------------------------------------------------------

inline constexpr const char* kLeftRawTopicSuffix = "/left/image";
inline constexpr const char* kRightRawTopicSuffix = "/right/image";
inline constexpr const char* kLeftH264TopicSuffix = "/left/video_encoded";
inline constexpr const char* kRightH264TopicSuffix = "/right/video_encoded";

inline constexpr const char* kRawImageDdsTypeName = "sensor_msgs::msg::dds_::Image_";
inline constexpr const char* kH264PacketDdsTypeName = "foxglove_msgs::msg::dds_::CompressedVideo_";

// DDS domain id used by the cora middleware (default ROS2 domain).
inline constexpr uint32_t kCoraDdsDomainId = 0;
// Single executable + single driver name for all three physical mounts.
inline constexpr const char* kCoracamProgramName = "rollio-device-camera-cora";
inline constexpr const char* kCoracamDriver = "camera-cora";

inline constexpr DeviceDescriptor kHeadDescriptor{
    "cora-head",
    "coracam_head",
    "Coracam Head",
    "rt/robot/camera/head",
};

inline constexpr DeviceDescriptor kLefthandDescriptor{
    "cora-lefthand",
    "coracam_lefthand",
    "Coracam Left Wrist",
    "rt/robot/camera/left_wrist",
};

inline constexpr DeviceDescriptor kRighthandDescriptor{
    "cora-righthand",
    "coracam_righthand",
    "Coracam Right Wrist",
    "rt/robot/camera/right_wrist",
};

inline constexpr const DeviceDescriptor* kAllDescriptors[] = {
    &kHeadDescriptor,
    &kLefthandDescriptor,
    &kRighthandDescriptor,
};
inline constexpr std::size_t kDescriptorCount =
    sizeof(kAllDescriptors) / sizeof(kAllDescriptors[0]);

// Resolve a descriptor by id. Returns nullptr when the id does not match any
// known physical Coracam mount point.
inline constexpr const DeviceDescriptor* find_descriptor_by_id(std::string_view id) {
    for (std::size_t i = 0; i < kDescriptorCount; ++i) {
        if (id == kAllDescriptors[i]->id) {
            return kAllDescriptors[i];
        }
    }
    return nullptr;
}

}  // namespace rollio::coracam

#endif  // ROLLIO_DEVICES_CORACAM_DEVICE_DESCRIPTOR_HPP
