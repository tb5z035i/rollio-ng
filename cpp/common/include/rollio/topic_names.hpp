#ifndef ROLLIO_TOPIC_NAMES_HPP
#define ROLLIO_TOPIC_NAMES_HPP

#include <string>

namespace rollio {

inline constexpr const char* CONTROL_EVENTS_SERVICE = "control/events";

inline auto camera_frames_service_name(const std::string& device_name) -> std::string {
    return "camera/" + device_name + "/frames";
}

inline auto robot_state_service_name(const std::string& device_name) -> std::string {
    return "robot/" + device_name + "/state";
}

inline auto robot_command_service_name(const std::string& device_name) -> std::string {
    return "robot/" + device_name + "/command";
}

}  // namespace rollio

#endif  // ROLLIO_TOPIC_NAMES_HPP
