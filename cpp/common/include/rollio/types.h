#ifndef ROLLIO_TYPES_H
#define ROLLIO_TYPES_H

#include <cstdint>
#include <cstring>

namespace rollio {

constexpr uint32_t MAX_JOINTS = 16;

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

// ---------------------------------------------------------------------------
// CameraFrameHeader — user header for raw-frame publish-subscribe
// ---------------------------------------------------------------------------

struct CameraFrameHeader {
    static constexpr const char* IOX2_TYPE_NAME = "CameraFrameHeader";
    uint64_t timestamp_ns;
    uint32_t width;
    uint32_t height;
    PixelFormat pixel_format;
    uint64_t frame_index;
};

// ---------------------------------------------------------------------------
// RobotState
// ---------------------------------------------------------------------------

struct RobotState {
    static constexpr const char* IOX2_TYPE_NAME = "RobotState";
    uint64_t timestamp_ns;
    uint32_t num_joints;
    double positions[MAX_JOINTS];
    double velocities[MAX_JOINTS];
    double efforts[MAX_JOINTS];
    double ee_pose[7];  // [x, y, z, qx, qy, qz, qw]
    bool has_ee_pose;
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

struct ControlEvent {
    static constexpr const char* IOX2_TYPE_NAME = "ControlEvent";
    ControlEventTag tag;
    union {
        struct {
            uint32_t episode_index;
        } recording_start;
        struct {
            uint32_t episode_index;
        } recording_stop;
        struct {
            uint32_t episode_index;
        } episode_keep;
        struct {
            uint32_t episode_index;
        } episode_discard;
        struct {
            uint32_t target_mode;
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
