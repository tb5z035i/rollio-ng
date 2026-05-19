#ifndef ROLLIO_DEVICES_CORACAM_CHANNEL_WORKER_HPP
#define ROLLIO_DEVICES_CORACAM_CHANNEL_WORKER_HPP

#include <atomic>
#include <cstdint>
#include <string>
#include <thread>

namespace rollio::coracam {

// Per-channel kind. Only the two kinds used by coracam are modeled.
enum class ChannelKind : uint32_t {
    RawBgr24 = 0,    // raw left/right eye frame (bgr24, 640x480)
    H264AnnexB = 1,  // pre-encoded H264 Annex-B access unit
};

// H264 Annex-B validation strategy (per方案文档 §5.3 annex_b_validation).
//   Scan      — every sample is start-code + NAL scanned (default; safest).
//   Metadata  — trust upstream is_keyframe metadata after warm-up; only spot
//               check the first byte for a start code.
//   Auto      — scan the first N samples (metadata_validation_packets);
//               if they agree with is_keyframe metadata switch to Metadata,
//               otherwise stay in Scan mode for the lifetime of the worker.
enum class AnnexBValidationMode : uint32_t {
    Scan = 0,
    Metadata = 1,
    Auto = 2,
};

// Configuration for a single channel worker thread.
struct ChannelWorkerConfig {
    // Iceoryx2 service name, e.g. "device/coracam_head/left_h264/frames".
    std::string service_name;
    // channel_type label used in log messages, e.g. "left_h264".
    std::string channel_type;
    // bus_root label used in log messages, e.g. "device/coracam_head".
    std::string bus_root;
    // What kind of data this channel carries.
    ChannelKind kind{ChannelKind::RawBgr24};
    // Frame dimensions; fixed at 640x480 for the current coracam revision.
    uint32_t width{0};
    uint32_t height{0};
    // Target frame rate in Hz; used to set the inter-frame sleep interval.
    uint32_t fps{0};

    // --- DDS subscriber fields ---
    // DDS wire topic name to subscribe to (e.g. "rt/robot/camera/head/left/image").
    // When empty the worker uses the internal mock generator (no DDS stack
    // required; useful for unit tests and offline validation).
    std::string dds_topic_name;
    // DDS type name that must match the publisher (e.g.
    // "sensor_msgs::msg::dds_::Image_" or "cora_msgs::msg::dds_::H264Packet_").
    // Ignored when dds_topic_name is empty.
    std::string dds_type_name;
    // DDS domain id (cora default: 0).
    uint32_t dds_domain_id{0};
    // Maximum accepted payload size in bytes (default 4 MiB, matches
    // max_packet_bytes in the Coracam mapping config doc).
    uint32_t max_payload_bytes{4U * 1024U * 1024U};

    // H264-only: validation mode (see AnnexBValidationMode). Ignored for raw
    // channels.
    AnnexBValidationMode annex_b_validation{AnnexBValidationMode::Scan};
    // H264-only: how many samples to scan before letting Auto switch to
    // metadata-trust mode.
    uint32_t metadata_validation_packets{16};

    // Raw-only: encoding string we expect to see in sensor_msgs/Image.encoding
    // (e.g. "bgr8" for BGR24). Empty disables encoding-string validation.
    std::string raw_expected_encoding;
};

// Per-channel telemetry counters — written by the worker thread and read
// (best-effort, no lock) by the main thread for the periodic log line.
struct ChannelStatus {
    uint64_t frames_received{0};          // DDS samples received (raw + h264)
    uint64_t frames_published{0};         // iceoryx2 samples successfully published
    uint64_t frames_dropped{0};           // dropped: loan failure or parse/validate error
    uint64_t keyframes{0};                // H264 only: IDR frames seen
    uint64_t sps_pps_seen{0};             // H264 only: samples carrying SPS+PPS
    uint64_t discontinuities{0};          // timestamp / sequence backward jumps
    uint64_t timestamp_regressions{0};    // timestamp went backwards
    uint64_t timestamp_gaps{0};           // timestamp jumped > 3 * frame_period
    uint64_t encoding_mismatches{0};      // raw: encoding != expected
    uint64_t stride_repacks{0};           // raw: step != width*bpp, row-copy taken
    uint64_t dimension_drifts{0};         // width/height differs from configured
    uint64_t pre_codec_drops{0};          // H264: IDR seen before any SPS+PPS
    uint64_t codec_config_recoveries{0};  // H264: came back to ready after loss
    uint64_t idle_seconds{0};             // consecutive seconds without new sample
    bool codec_ready{false};              // H264: SPS+PPS were seen at least once
};

// One running channel worker. Create via start(); stop via stop() (RAII).
class ChannelWorker {
public:
    explicit ChannelWorker(ChannelWorkerConfig cfg);
    ~ChannelWorker();

    ChannelWorker(const ChannelWorker&) = delete;
    ChannelWorker& operator=(const ChannelWorker&) = delete;

    // Begin the publish loop in a background thread.
    void start();

    // Ask the worker to stop at the next opportunity and wait for it.
    void stop();

    // Snapshot of current telemetry (lock-free best-effort read).
    [[nodiscard]] ChannelStatus status() const noexcept;

private:
    void worker_loop();
    void worker_loop_dds();        // dispatch to cora typed reader path
    void worker_loop_cora_raw();   // Cora SDK raw image typed reader path
    void worker_loop_cora_h264();  // Cora SDK foxglove h264 typed reader path
    void worker_loop_mock();       // internal mock generator (no DDS required)

    ChannelWorkerConfig cfg_;
    ChannelStatus status_{};
    std::atomic<bool> stop_requested_{false};
    std::thread thread_;
};

}  // namespace rollio::coracam

#endif  // ROLLIO_DEVICES_CORACAM_CHANNEL_WORKER_HPP
