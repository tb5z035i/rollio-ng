// SPDX-License-Identifier: Apache-2.0
//
// Hand-rolled equivalent of `fastddsgen -typeros2` output for the six IDLs
// the UMI bridge needs. We hand-write the serialization rather than
// pre-generating because the dev environment can't be assumed to have a
// JDK + fastddsgen, and the on-the-wire types are tiny and stable.
//
// Wire-format compatibility: cora publishes via Fast-DDS 1.x with
// fastddsgen `-typeros2`, which produces XCDRv1 (a.k.a. PLAIN_CDR) on the
// wire with little-endian encapsulation by default. The type names follow
// the ROS2 convention `<pkg>::msg::dds_::<Type>_` so the rollio bridge's
// reader matches against cora's writer when the type names line up.
//
// IDL provenance: copied from
//   https://github.com/.../robot/framework/dds/msg/{foxglove_msgs,sensor_msgs,
//                                                   std_msgs,geometry_msgs,
//                                                   builtin_interfaces}/msg/*.idl
// (see ../idl/ for the verbatim text). To regenerate using fastddsgen
// once a JDK is available, run:
//   fastddsgen -typeros2 -replace -cs -d src/generated idl/.../*.idl

#pragma once

#include <array>
#include <cstdint>
#include <string>
#include <vector>

namespace builtin_interfaces {
namespace msg {
struct Time {
    int32_t sec{0};
    uint32_t nanosec{0};
};
}  // namespace msg
}  // namespace builtin_interfaces

namespace geometry_msgs {
namespace msg {
struct Vector3 {
    double x{0.0};
    double y{0.0};
    double z{0.0};
};

struct Quaternion {
    double x{0.0};
    double y{0.0};
    double z{0.0};
    double w{1.0};
};
}  // namespace msg
}  // namespace geometry_msgs

namespace std_msgs {
namespace msg {
struct Header {
    builtin_interfaces::msg::Time stamp;
    std::string frame_id;
};
}  // namespace msg
}  // namespace std_msgs

namespace sensor_msgs {
namespace msg {
struct Imu {
    std_msgs::msg::Header header;
    geometry_msgs::msg::Quaternion orientation;
    std::array<double, 9> orientation_covariance{};
    geometry_msgs::msg::Vector3 angular_velocity;
    std::array<double, 9> angular_velocity_covariance{};
    geometry_msgs::msg::Vector3 linear_acceleration;
    std::array<double, 9> linear_acceleration_covariance{};
};
}  // namespace msg
}  // namespace sensor_msgs

namespace foxglove_msgs {
namespace msg {
struct CompressedVideo {
    builtin_interfaces::msg::Time timestamp;
    std::string frame_id;
    std::vector<uint8_t> data;
    std::string format;
};
}  // namespace msg
}  // namespace foxglove_msgs
