#ifndef ROLLIO_DEVICES_CORACAM_DEVICE_DESCRIPTOR_HPP
#define ROLLIO_DEVICES_CORACAM_DEVICE_DESCRIPTOR_HPP

#include <cstdint>
#include <string>

namespace rollio::coracam {

// One descriptor per executable. The three coracam devices share the
// runtime under src/; their entry points (apps/coracam_*.cpp) only differ
// in which descriptor they inject so probe/query/validate/run can produce
// the expected driver/label and the controller-facing default device
// metadata. Topic name suffix conventions are documented in
// signal_other/analysis/rollio-device/cora-topic-to-rollio-bus-device方案.zh.md.
struct DeviceDescriptor {
    // CLI program name (matches BinaryDeviceConfig.executable). Logged in
    // every status / error line so triaging which of the three coracams
    // emitted a message is unambiguous.
    const char* program_name;
    // BinaryDeviceConfig.driver value the run command requires. Setup /
    // controller queries match on this.
    const char* driver;
    // Default device id surfaced by probe (the controller can override
    // via the operator's project config).
    const char* default_id;
    // Default device name -- e.g. "coracam_head". Used to seed the
    // controller's setup wizard; also drives the default bus_root the
    // run command falls back to when the config omits one.
    const char* default_name;
    // Human-readable label returned by query --json.
    const char* device_label;
    // Default cora topic prefix expected for this physical mount point;
    // surfaced via probe / query so the topic mapping file is easier to
    // author by hand. See section 8/阶段 0 of the implementation plan
    // for the canonical name list.
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
//
// Confirmed via camera_node and `ros2 topic info --verbose` on a live robot
// (2026-05-14/16):
//
//   Topic names (12 total, publisher node = _CREATED_BY_BARE_DDS_APP_):
//     rt/robot/camera/head/{left,right}/{image,video_encoded}
//     rt/robot/camera/left_wrist/{left,right}/{image,video_encoded}
//     rt/robot/camera/right_wrist/{left,right}/{image,video_encoded}
//
//   * <prefix>/{left,right}/image  → sensor_msgs/msg/Image  ✓ CONFIRMED
//     DDS type: sensor_msgs::msg::dds_::Image_
//     Publisher QoS: BEST_EFFORT / VOLATILE / AUTOMATIC liveliness
//
//   * <prefix>/{left,right}/video_encoded  → foxglove_msgs/msg/CompressedVideo  ✓ CONFIRMED
//     DDS type: foxglove_msgs::msg::dds_::CompressedVideo_
//     Publisher QoS: RELIABLE / VOLATILE / AUTOMATIC liveliness
//     CDR layout: timestamp (sec+nanosec), frame_id (string),
//                 data (sequence<uint8>), format (string, e.g. "h264")
//     Note: no width/height/is_keyframe fields — keyframe detection via NAL scan only.
//
// ROS2 DDS type-name mangling: <pkg>::msg::dds_::<Type>_ (trailing underscore).
// ---------------------------------------------------------------------------

inline constexpr const char* kLeftRawTopicSuffix = "/left/image";
inline constexpr const char* kRightRawTopicSuffix = "/right/image";
inline constexpr const char* kLeftH264TopicSuffix = "/left/video_encoded";
inline constexpr const char* kRightH264TopicSuffix = "/right/video_encoded";

inline constexpr const char* kRawImageDdsTypeName = "sensor_msgs::msg::dds_::Image_";
inline constexpr const char* kH264PacketDdsTypeName = "foxglove_msgs::msg::dds_::CompressedVideo_";

// DDS domain id used by the cora middleware (default ROS2 domain).
inline constexpr uint32_t kCoraDdsDomainId = 0;

inline constexpr DeviceDescriptor kHeadDescriptor{
    "rollio-device-coracam-head", "coracam-head", "cora-head", "coracam_head", "Coracam Head",
    "rt/robot/camera/head",
};

inline constexpr DeviceDescriptor kLefthandDescriptor{
    "rollio-device-coracam-lefthand",
    "coracam-lefthand",
    "cora-lefthand",
    "coracam_lefthand",
    "Coracam Left Wrist",
    "rt/robot/camera/left_wrist",
};

inline constexpr DeviceDescriptor kRighthandDescriptor{
    "rollio-device-coracam-righthand",
    "coracam-righthand",
    "cora-righthand",
    "coracam_righthand",
    "Coracam Right Wrist",
    "rt/robot/camera/right_wrist",
};

}  // namespace rollio::coracam

#endif  // ROLLIO_DEVICES_CORACAM_DEVICE_DESCRIPTOR_HPP
