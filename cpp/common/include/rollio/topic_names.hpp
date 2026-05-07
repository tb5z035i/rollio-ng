#ifndef ROLLIO_TOPIC_NAMES_HPP
#define ROLLIO_TOPIC_NAMES_HPP

#include <string>

namespace rollio {

inline constexpr const char* CONTROL_EVENTS_SERVICE = "control/events";
inline constexpr const char* VIDEO_READY_SERVICE = "encoder/video-ready";
inline constexpr const char* BACKPRESSURE_SERVICE = "encoder/backpressure";

inline auto camera_frames_service_name(const std::string& device_name) -> std::string {
    return "camera/" + device_name + "/frames";
}

/// Hierarchical IPC name: `{bus_root}/{channel_type}/frames` (matches
/// rollio_bus::channel_frames_service_name).
inline auto channel_frames_service_name(const std::string& bus_root,
                                        const std::string& channel_type) -> std::string {
    return bus_root + "/" + channel_type + "/frames";
}

inline auto channel_mode_info_service_name(const std::string& bus_root,
                                           const std::string& channel_type) -> std::string {
    return bus_root + "/" + channel_type + "/info/mode";
}

inline auto channel_mode_control_service_name(const std::string& bus_root,
                                              const std::string& channel_type) -> std::string {
    return bus_root + "/" + channel_type + "/control/mode";
}

/// IMU state service: `{bus_root}/{channel_type}/states/imu`. Mirror of
/// rollio_bus::channel_imu_service_name.
inline auto channel_imu_service_name(const std::string& bus_root,
                                     const std::string& channel_type) -> std::string {
    return bus_root + "/" + channel_type + "/states/imu";
}

inline auto robot_state_service_name(const std::string& device_name) -> std::string {
    return "robot/" + device_name + "/state";
}

inline auto robot_command_service_name(const std::string& device_name) -> std::string {
    return "robot/" + device_name + "/command";
}

}  // namespace rollio

#endif  // ROLLIO_TOPIC_NAMES_HPP
