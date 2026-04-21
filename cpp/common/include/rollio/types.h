#ifndef ROLLIO_TYPES_H
#define ROLLIO_TYPES_H

#include <cstdint>
#include <cstring>
#include <optional>
#include <string_view>

namespace rollio {

constexpr uint32_t MAX_JOINTS = 16;

// ---------------------------------------------------------------------------
// Device channel kind (matches rollio-types DeviceType serde: lowercase)
// ---------------------------------------------------------------------------

enum class DeviceKind : uint32_t {
    Camera = 0,
    Robot = 1,
};

inline auto device_kind_from_string(const std::string_view value) -> std::optional<DeviceKind> {
    if (value == "camera") {
        return DeviceKind::Camera;
    }
    if (value == "robot") {
        return DeviceKind::Robot;
    }
    return std::nullopt;
}

inline auto device_kind_to_string(const DeviceKind kind) -> const char* {
    switch (kind) {
        case DeviceKind::Camera:
            return "camera";
        case DeviceKind::Robot:
            return "robot";
    }
    return "camera";
}

// ---------------------------------------------------------------------------
// Fixed-size strings (matching Rust FixedString64 / FixedString256)
// ---------------------------------------------------------------------------

struct FixedString64 {
    static constexpr const char* IOX2_TYPE_NAME = "FixedString64";
    uint8_t data[64];
    uint32_t len;
};

struct FixedString256 {
    static constexpr const char* IOX2_TYPE_NAME = "FixedString256";
    uint8_t data[256];
    uint32_t len;
};

// ---------------------------------------------------------------------------
// PixelFormat
// ---------------------------------------------------------------------------

enum class PixelFormat : uint32_t {
    Rgb24 = 0,
    Bgr24 = 1,
    Yuyv = 2,
    Mjpeg = 3,
    Depth16 = 4,
    Gray8 = 5,
};

inline auto pixel_format_to_string(const PixelFormat pixel_format) -> const char* {
    switch (pixel_format) {
        case PixelFormat::Rgb24:
            return "rgb24";
        case PixelFormat::Bgr24:
            return "bgr24";
        case PixelFormat::Yuyv:
            return "yuyv";
        case PixelFormat::Mjpeg:
            return "mjpeg";
        case PixelFormat::Depth16:
            return "depth16";
        case PixelFormat::Gray8:
            return "gray8";
    }

    return "rgb24";
}

inline auto pixel_format_from_string(const std::string_view value) -> std::optional<PixelFormat> {
    if (value == "rgb24") {
        return PixelFormat::Rgb24;
    }
    if (value == "bgr24") {
        return PixelFormat::Bgr24;
    }
    if (value == "yuyv") {
        return PixelFormat::Yuyv;
    }
    if (value == "mjpeg") {
        return PixelFormat::Mjpeg;
    }
    if (value == "depth16") {
        return PixelFormat::Depth16;
    }
    if (value == "gray8") {
        return PixelFormat::Gray8;
    }

    return std::nullopt;
}

// ---------------------------------------------------------------------------
// CameraFrameHeader — user header for raw-frame publish-subscribe
// ---------------------------------------------------------------------------

struct CameraFrameHeader {
    static constexpr const char* IOX2_TYPE_NAME = "CameraFrameHeader";
    uint64_t timestamp_us;
    uint32_t width;
    uint32_t height;
    PixelFormat pixel_format;
    uint64_t frame_index;
};

// ---------------------------------------------------------------------------
// RobotState
// ---------------------------------------------------------------------------

enum class EndEffectorStatus : uint32_t {
    Unknown = 0,
    Disabled = 1,
    Enabled = 2,
};

struct RobotState {
    static constexpr const char* IOX2_TYPE_NAME = "RobotState";
    uint64_t timestamp_ns;
    uint32_t num_joints;
    double positions[MAX_JOINTS];
    double velocities[MAX_JOINTS];
    double efforts[MAX_JOINTS];
    double ee_pose[7];  // [x, y, z, qx, qy, qz, qw]
    bool has_ee_pose;
    EndEffectorStatus end_effector_status;
    bool has_end_effector_status;
    bool end_effector_feedback_valid;
};

// ---------------------------------------------------------------------------
// RobotCommand
// ---------------------------------------------------------------------------

enum class CommandMode : uint32_t {
    Joint = 0,
    Cartesian = 1,
};

struct RobotCommand {
    static constexpr const char* IOX2_TYPE_NAME = "RobotCommand";
    uint64_t timestamp_ns;
    CommandMode mode;
    uint32_t num_joints;
    double joint_targets[MAX_JOINTS];
    double cartesian_target[7];
};

// ---------------------------------------------------------------------------
// ControlEvent
// ---------------------------------------------------------------------------

// ControlEvent is a tagged union in Rust.  For C++ interop we represent it
// as a struct with a discriminant tag and a union of payloads so the memory
// layout matches the Rust #[repr(C)] enum.

enum class ControlEventTag : uint32_t {
    RecordingStart = 0,
    RecordingStop = 1,
    EpisodeKeep = 2,
    EpisodeDiscard = 3,
    Shutdown = 4,
    ModeSwitch = 5,
};

// Mirrors the Rust `#[repr(C)] enum ControlEvent`. With `RecordingStart` /
// `RecordingStop` now carrying `(u32 episode_index, u64 controller_ts_us)`,
// the largest variant's alignment is 8, so the union is laid out at offset
// 8 (4-byte tag + 4-byte padding). Every variant inside the union is sized
// to 16 bytes to match the Rust layout — smaller variants are padded with
// `_pad`.
struct ControlEvent {
    static constexpr const char* IOX2_TYPE_NAME = "ControlEvent";
    ControlEventTag tag;
    uint32_t _tag_padding;
    union {
        struct {
            uint32_t episode_index;
            uint32_t _pad;
            uint64_t controller_ts_us;
        } recording_start;
        struct {
            uint32_t episode_index;
            uint32_t _pad;
            uint64_t controller_ts_us;
        } recording_stop;
        struct {
            uint32_t episode_index;
            uint32_t _pad[3];
        } episode_keep;
        struct {
            uint32_t episode_index;
            uint32_t _pad[3];
        } episode_discard;
        struct {
            uint32_t target_mode;
            uint32_t _pad[3];
        } mode_switch;
    } payload;
};

// ---------------------------------------------------------------------------
// MetricEntry / MetricsReport
// ---------------------------------------------------------------------------

constexpr uint32_t MAX_METRICS = 32;

struct MetricEntry {
    static constexpr const char* IOX2_TYPE_NAME = "MetricEntry";
    FixedString64 name;
    double value;
};

struct MetricsReport {
    static constexpr const char* IOX2_TYPE_NAME = "MetricsReport";
    FixedString64 process_id;
    uint64_t timestamp_ns;
    uint32_t num_entries;
    MetricEntry entries[MAX_METRICS];
};

// ---------------------------------------------------------------------------
// WarningEvent
// ---------------------------------------------------------------------------

struct WarningEvent {
    static constexpr const char* IOX2_TYPE_NAME = "WarningEvent";
    FixedString64 process_id;
    FixedString64 metric_name;
    double current_value;
    FixedString256 explanation;
};

// ---------------------------------------------------------------------------
// VideoReady
// ---------------------------------------------------------------------------

struct VideoReady {
    static constexpr const char* IOX2_TYPE_NAME = "VideoReady";
    FixedString64 process_id;
    uint32_t episode_index;
    FixedString256 file_path;
};

// ---------------------------------------------------------------------------
// BackpressureEvent
// ---------------------------------------------------------------------------

struct BackpressureEvent {
    static constexpr const char* IOX2_TYPE_NAME = "BackpressureEvent";
    FixedString64 process_id;
    FixedString64 queue_name;
};

}  // namespace rollio

#endif  // ROLLIO_TYPES_H
