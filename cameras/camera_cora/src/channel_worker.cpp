#include "channel_worker.hpp"

#include <algorithm>
#include <chrono>
#include <cstdint>
#include <cstdlib>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <iomanip>
#include <iostream>
#include <sstream>
#include <stdexcept>
#include <vector>

// Cora SDK headers.
#include <cora/channel.h>
#include <cora/dds/dds_qos.h>
#include <foxglove_msgs/msg/CompressedVideo.h>
#include <foxglove_msgs/msg/CompressedVideoPubSubTypes.h>
#include <sensor_msgs/msg/Image.h>
#include <sensor_msgs/msg/ImagePubSubTypes.h>

#include "h264_annexb.hpp"
#include "iox2/iceoryx2.hpp"
#include "rollio/topic_names.hpp"
#include "rollio/types.h"

namespace rollio::coracam {

using SteadyClock = std::chrono::steady_clock;
using SystemClock = std::chrono::system_clock;

using RawImageReader =
    framework::ChannelReader<sensor_msgs::msg::Image, sensor_msgs::msg::ImagePubSubType>;
using H264Reader = framework::ChannelReader<foxglove_msgs::msg::CompressedVideo,
                                            foxglove_msgs::msg::CompressedVideoPubSubType>;

namespace {

constexpr auto kStatusLogInterval = std::chrono::seconds(10);
constexpr uint64_t kFirstIdleLogSeconds = 10;

auto unix_timestamp_us() -> uint64_t {
    return static_cast<uint64_t>(
        std::chrono::duration_cast<std::chrono::microseconds>(SystemClock::now().time_since_epoch())
            .count());
}

auto steady_elapsed_us(SteadyClock::time_point start, SteadyClock::time_point end) -> uint64_t {
    return static_cast<uint64_t>(
        std::chrono::duration_cast<std::chrono::microseconds>(end - start).count());
}

auto steady_elapsed_ms(SteadyClock::time_point start, SteadyClock::time_point end) -> double {
    return static_cast<double>(steady_elapsed_us(start, end)) / 1000.0;
}

// Convert sec/nanosec (ROS2 Time-style) to UNIX microseconds.
auto stamp_to_us(int32_t sec, uint32_t nanosec) -> uint64_t {
    return static_cast<uint64_t>(sec) * 1'000'000ULL + static_cast<uint64_t>(nanosec) / 1'000ULL;
}

auto source_age_ms(uint64_t now_us, uint64_t source_ts_us) -> double {
    return (static_cast<double>(now_us) - static_cast<double>(source_ts_us)) / 1000.0;
}

auto positive_source_age_us(uint64_t now_us, uint64_t source_ts_us) -> uint64_t {
    return now_us > source_ts_us ? now_us - source_ts_us : 0U;
}

struct PublishResult {
    bool ok{false};
    uint64_t elapsed_us{0};
};

struct MetricStats {
    uint64_t count{0};
    double sum{0.0};
    double min{0.0};
    double max{0.0};

    void observe(double value) {
        if (count == 0U) {
            min = value;
            max = value;
        } else {
            min = std::min(min, value);
            max = std::max(max, value);
        }
        sum += value;
        ++count;
    }

    [[nodiscard]] auto avg() const -> double {
        return count == 0U ? 0.0 : sum / static_cast<double>(count);
    }
};

struct PipelineWindowStats {
    MetricStats payload_in_bytes;
    MetricStats payload_out_bytes;
    MetricStats dds_wait_ms;
    MetricStats recv_gap_ms;
    MetricStats source_gap_ms;
    MetricStats source_age_ms;
    MetricStats callback_to_publish_ms;
    MetricStats publish_ms;
    MetricStats raw_convert_ms;
    uint64_t h264_i_frames{0};
    uint64_t h264_p_frames{0};
    uint64_t h264_b_frames{0};
    uint64_t h264_sp_frames{0};
    uint64_t h264_si_frames{0};
    uint64_t h264_mixed_frames{0};
    uint64_t h264_unknown_frames{0};
    uint64_t h264_idr_frames{0};
    uint64_t h264_pattern_total{0};
    std::string h264_pattern;

    void reset() {
        *this = PipelineWindowStats{};
    }
};

auto metric_summary(const char* name, const MetricStats& metric) -> std::string {
    std::ostringstream out;
    out << name << '=';
    if (metric.count == 0U) {
        out << "n/a";
        return out.str();
    }
    out << std::fixed << std::setprecision(1) << metric.avg() << '/' << metric.min << '/'
        << metric.max;
    return out.str();
}

auto metric_avg_max(const char* name, const MetricStats& metric) -> std::string {
    std::ostringstream out;
    out << name << '=';
    if (metric.count == 0U) {
        out << "n/a";
        return out.str();
    }
    out << std::fixed << std::setprecision(1) << metric.avg() << '/' << metric.max;
    return out.str();
}

auto h264_slice_summary(const H264SliceTypeStats& stats) -> std::string {
    std::ostringstream out;
    out << "slices=vcl:" << stats.vcl_nalus << "/idr:" << stats.idr_nalus << "/I:" << stats.i_slices
        << "/P:" << stats.p_slices << "/B:" << stats.b_slices << "/SP:" << stats.sp_slices
        << "/SI:" << stats.si_slices << "/?:" << stats.unknown_slices;
    return out.str();
}

auto update_h264_type_stats(ChannelStatus& status, PipelineWindowStats& window,
                            const H264SliceTypeStats& slice_stats) -> char {
    const char label = h264_picture_type_label(slice_stats);
    switch (label) {
        case 'I':
            ++status.h264_frames_i;
            ++window.h264_i_frames;
            break;
        case 'P':
            ++status.h264_frames_p;
            ++window.h264_p_frames;
            break;
        case 'B':
            ++status.h264_frames_b;
            ++window.h264_b_frames;
            break;
        case 'S':
            ++status.h264_frames_sp;
            ++window.h264_sp_frames;
            break;
        case 'T':
            ++status.h264_frames_si;
            ++window.h264_si_frames;
            break;
        case 'M':
            ++status.h264_frames_mixed;
            ++window.h264_mixed_frames;
            break;
        default:
            ++status.h264_frames_unknown;
            ++window.h264_unknown_frames;
            break;
    }
    if (slice_stats.idr_nalus > 0U) {
        ++status.h264_idr_frames;
        ++window.h264_idr_frames;
    }
    if (window.h264_pattern.size() < 120U) {
        window.h264_pattern.push_back(label);
    }
    ++window.h264_pattern_total;
    return label;
}

auto fps(uint64_t frames, double elapsed_sec) -> double {
    return elapsed_sec > 0.0 ? static_cast<double>(frames) / elapsed_sec : 0.0;
}

auto log_pipeline_status(const std::string& prog, const ChannelStatus& status,
                         const ChannelStatus& previous, const PipelineWindowStats& window,
                         double elapsed_sec, bool is_h264) -> void {
    const auto received = status.frames_received - previous.frames_received;
    const auto published = status.frames_published - previous.frames_published;
    const auto dropped = status.frames_dropped - previous.frames_dropped;
    std::ostringstream out;
    out << prog << ": pipeline interval_s=" << std::fixed << std::setprecision(1) << elapsed_sec
        << " recv=" << received << " pub=" << published << " drop=" << dropped
        << " recv_fps=" << fps(received, elapsed_sec) << " pub_fps=" << fps(published, elapsed_sec)
        << " total_recv=" << status.frames_received << " total_pub=" << status.frames_published
        << " total_drop=" << status.frames_dropped
        << " fallback_ts=" << (status.timestamp_fallbacks - previous.timestamp_fallbacks) << " "
        << metric_summary("reader_wait_ms", window.dds_wait_ms) << " "
        << metric_summary("recv_gap_ms", window.recv_gap_ms) << " "
        << metric_summary("source_gap_ms", window.source_gap_ms) << " "
        << metric_summary("source_age_ms", window.source_age_ms) << " "
        << metric_summary("callback_to_publish_ms", window.callback_to_publish_ms) << " "
        << metric_summary("publish_ms", window.publish_ms) << " "
        << metric_summary("payload_in_bytes", window.payload_in_bytes) << " "
        << metric_summary("payload_out_bytes", window.payload_out_bytes);
    if (window.raw_convert_ms.count > 0U) {
        out << " " << metric_summary("raw_convert_ms", window.raw_convert_ms);
    }
    if (status.discontinuities > previous.discontinuities) {
        out << " ts_regr=" << (status.timestamp_regressions - previous.timestamp_regressions)
            << " ts_gaps=" << (status.timestamp_gaps - previous.timestamp_gaps);
    }
    if (status.encoding_mismatches > previous.encoding_mismatches ||
        status.stride_repacks > previous.stride_repacks ||
        status.dimension_drifts > previous.dimension_drifts) {
        out << " enc_mism=" << (status.encoding_mismatches - previous.encoding_mismatches)
            << " stride_repacks=" << (status.stride_repacks - previous.stride_repacks)
            << " dim_drifts=" << (status.dimension_drifts - previous.dimension_drifts);
    }
    if (is_h264) {
        out << " keyframes=" << (status.keyframes - previous.keyframes)
            << " sps_pps=" << (status.sps_pps_seen - previous.sps_pps_seen)
            << " pre_codec_drops=" << (status.pre_codec_drops - previous.pre_codec_drops)
            << " codec_ready=" << (status.codec_ready ? 1 : 0)
            << " frame_types=I:" << window.h264_i_frames << "/P:" << window.h264_p_frames
            << "/B:" << window.h264_b_frames << "/SP:" << window.h264_sp_frames
            << "/SI:" << window.h264_si_frames << "/M:" << window.h264_mixed_frames
            << "/?:" << window.h264_unknown_frames << " idr=" << window.h264_idr_frames
            << " gop=" << (window.h264_pattern.empty() ? "n/a" : window.h264_pattern);
        if (window.h264_pattern_total > window.h264_pattern.size()) {
            out << "(+" << (window.h264_pattern_total - window.h264_pattern.size()) << ")";
        }
    }
    out << " max_source_age_ms=" << static_cast<double>(status.max_source_age_us) / 1000.0
        << " max_callback_to_publish_ms="
        << static_cast<double>(status.max_callback_to_publish_us) / 1000.0
        << " max_publish_ms=" << static_cast<double>(status.max_publish_us) / 1000.0;
    std::cerr << out.str() << '\n';
}

auto log_pipeline_summary(const std::string& prog, const ChannelStatus& status,
                          const ChannelStatus& previous, const PipelineWindowStats& window,
                          double elapsed_sec, bool is_h264) -> void {
    const auto received = status.frames_received - previous.frames_received;
    const auto published = status.frames_published - previous.frames_published;
    const auto dropped = status.frames_dropped - previous.frames_dropped;
    std::ostringstream out;
    out << prog << ": pipeline summary interval_s=" << std::fixed << std::setprecision(1)
        << elapsed_sec << " recv=" << received << " pub=" << published << " drop=" << dropped
        << " recv_fps=" << fps(received, elapsed_sec)
        << " pub_fps=" << fps(published, elapsed_sec)
        << " total_drop=" << status.frames_dropped << " "
        << metric_avg_max("source_age_ms", window.source_age_ms) << " "
        << metric_avg_max("callback_to_publish_ms", window.callback_to_publish_ms) << " "
        << metric_avg_max("publish_ms", window.publish_ms);
    if (is_h264) {
        out << " keyframes=" << (status.keyframes - previous.keyframes)
            << " sps_pps=" << (status.sps_pps_seen - previous.sps_pps_seen)
            << " pre_codec_drops=" << (status.pre_codec_drops - previous.pre_codec_drops)
            << " codec_ready=" << (status.codec_ready ? 1 : 0);
    } else {
        const auto enc_mism = status.encoding_mismatches - previous.encoding_mismatches;
        const auto stride_repacks = status.stride_repacks - previous.stride_repacks;
        const auto dim_drifts = status.dimension_drifts - previous.dimension_drifts;
        if (enc_mism > 0U || stride_repacks > 0U || dim_drifts > 0U) {
            out << " enc_mism=" << enc_mism << " stride_repacks=" << stride_repacks
                << " dim_drifts=" << dim_drifts;
        }
    }
    std::cerr << out.str() << '\n';
}

auto env_is_enabled(const char* name) -> bool {
    const char* value = std::getenv(name);
    if (value == nullptr || *value == '\0') {
        return false;
    }
    return std::strcmp(value, "0") != 0 && std::strcmp(value, "false") != 0 &&
           std::strcmp(value, "FALSE") != 0 && std::strcmp(value, "off") != 0 &&
           std::strcmp(value, "OFF") != 0;
}

auto advanced_pipeline_logs_enabled() -> bool {
    return env_is_enabled("ROLLIO_ADVANCED_PIPELINE_LOGS");
}

auto h264_dump_dir() -> std::filesystem::path {
    const char* explicit_dir = std::getenv("ROLLIO_CORACAM_H264_DUMP_DIR");
    if (explicit_dir != nullptr && *explicit_dir != '\0') {
        return explicit_dir;
    }
    if (!env_is_enabled("ROLLIO_CORACAM_H264_DUMP")) {
        return {};
    }
    const char* log_dir = std::getenv("ROLLIO_LOG_DIR");
    if (log_dir == nullptr || *log_dir == '\0') {
        return {};
    }
    return std::filesystem::path(log_dir) / "h264-coracam";
}

auto h264_nal_type_list(const uint8_t* data, std::size_t size) -> std::string {
    const auto offsets = find_nal_offsets(data, size);
    std::ostringstream out;
    for (std::size_t i = 0; i < offsets.size(); ++i) {
        if (i != 0U) {
            out << ',';
        }
        out << static_cast<int>(data[offsets[i]] & 0x1FU);
    }
    return out.str();
}

auto append_h264_dump(const std::filesystem::path& dir, const std::string& channel_type,
                      const std::vector<uint8_t>& data) -> void {
    if (dir.empty()) {
        return;
    }
    std::error_code ec;
    std::filesystem::create_directories(dir, ec);
    if (ec) {
        std::cerr << "[coracam] " << channel_type << ": failed to create h264 dump dir "
                  << dir.string() << ": " << ec.message() << '\n';
        return;
    }
    const auto path = dir / (channel_type + ".h264");
    std::ofstream out(path, std::ios::binary | std::ios::app);
    if (!out.is_open()) {
        std::cerr << "[coracam] " << channel_type << ": failed to open h264 dump " << path.string()
                  << '\n';
        return;
    }
    out.write(reinterpret_cast<const char*>(data.data()),
              static_cast<std::streamsize>(data.size()));
}

auto log_h264_sample_debug(const std::string& channel_type, uint64_t frame_index,
                           const std::vector<uint8_t>& data, bool has_sps, bool has_pps,
                           bool has_idr, char picture_type, const H264SliceTypeStats& slice_stats)
    -> void {
    if (frame_index >= 20U && !has_sps && !has_pps && !has_idr) {
        return;
    }
    std::cerr << "[coracam] " << channel_type << ": h264 sample idx=" << frame_index
              << " bytes=" << data.size() << " nals=["
              << h264_nal_type_list(data.data(), data.size()) << "] sps=" << has_sps
              << " pps=" << has_pps << " idr=" << has_idr << " type=" << picture_type << " "
              << h264_slice_summary(slice_stats) << '\n';
}

// Generate a simple BGR24 test pattern for the mock path.
auto generate_raw_bgr24(uint32_t width, uint32_t height, uint64_t frame_index,
                        std::vector<uint8_t>& out) -> void {
    out.resize(static_cast<std::size_t>(width) * height * 3U);
    for (uint32_t y = 0; y < height; ++y) {
        const auto row = static_cast<std::size_t>(y) * width * 3U;
        const uint8_t blue = static_cast<uint8_t>((y + frame_index) & 0xFFU);
        const uint8_t green = static_cast<uint8_t>((y * 2U + frame_index / 2U) & 0xFFU);
        const uint8_t red = static_cast<uint8_t>((frame_index) & 0xFFU);
        for (uint32_t x = 0; x < width; ++x) {
            const auto px = row + static_cast<std::size_t>(x) * 3U;
            out[px] = blue;
            out[px + 1] = green;
            out[px + 2] = red;
        }
    }
}

auto clamp_u8(int value) -> uint8_t {
    return static_cast<uint8_t>(std::clamp(value, 0, 255));
}

// NV12 → BGR24 conversion (read directly from the Cora SDK message buffer).
auto nv12_to_bgr24(const std::vector<uint8_t>& src, uint32_t width, uint32_t height, uint32_t step,
                   std::vector<uint8_t>& out) -> bool {
    if (width == 0 || height == 0 || (height % 2U) != 0) {
        return false;
    }
    const uint32_t y_step = step != 0 ? step : width;
    if (y_step < width) {
        return false;
    }
    const uint64_t y_len = static_cast<uint64_t>(height) * y_step;
    const uint64_t uv_len = static_cast<uint64_t>(height / 2U) * y_step;
    if (src.size() < y_len + uv_len) {
        return false;
    }

    const auto* y_plane = src.data();
    const auto* uv_plane = src.data() + y_len;
    out.resize(static_cast<std::size_t>(width) * height * 3U);
    for (uint32_t y = 0; y < height; ++y) {
        for (uint32_t x = 0; x < width; ++x) {
            const int y_val = static_cast<int>(y_plane[static_cast<std::size_t>(y) * y_step + x]);
            const std::size_t uv_index = static_cast<std::size_t>(y / 2U) * y_step + (x / 2U) * 2U;
            const int u = static_cast<int>(uv_plane[uv_index]) - 128;
            const int v = static_cast<int>(uv_plane[uv_index + 1]) - 128;
            const int c = std::max(0, y_val - 16);

            const int red = (298 * c + 409 * v + 128) >> 8;
            const int green = (298 * c - 100 * u - 208 * v + 128) >> 8;
            const int blue = (298 * c + 516 * u + 128) >> 8;

            auto* dst = &out[(static_cast<std::size_t>(y) * width + x) * 3U];
            dst[0] = clamp_u8(blue);
            dst[1] = clamp_u8(green);
            dst[2] = clamp_u8(red);
        }
    }
    return true;
}

auto mono8_to_bgr24(const std::vector<uint8_t>& src, uint32_t width, uint32_t height, uint32_t step,
                    std::vector<uint8_t>& out) -> bool {
    if (width == 0 || height == 0) {
        return false;
    }
    const uint32_t s = step != 0 ? step : width;
    if (s < width) {
        return false;
    }
    const uint64_t expected = static_cast<uint64_t>(height) * s;
    if (src.size() < expected) {
        return false;
    }

    out.resize(static_cast<std::size_t>(width) * height * 3U);
    for (uint32_t y = 0; y < height; ++y) {
        const auto* src_row = src.data() + static_cast<std::size_t>(y) * s;
        auto* dst_row = out.data() + static_cast<std::size_t>(y) * width * 3U;
        for (uint32_t x = 0; x < width; ++x) {
            const uint8_t value = src_row[x];
            dst_row[static_cast<std::size_t>(x) * 3U] = value;
            dst_row[static_cast<std::size_t>(x) * 3U + 1U] = value;
            dst_row[static_cast<std::size_t>(x) * 3U + 2U] = value;
        }
    }
    return true;
}

// Publish one frame to iceoryx2. Returns true on success.
auto publish_frame(iox2::Publisher<iox2::ServiceType::Ipc, iox2::bb::Slice<uint8_t>,
                                   rollio::CameraFrameHeader>& publisher,
                   uint64_t timestamp_us, uint32_t width, uint32_t height,
                   rollio::PixelFormat pixel_format, uint64_t frame_index,
                   const uint8_t* payload_ptr, uint64_t payload_len,
                   const std::string& channel_type) -> PublishResult {
    const auto publish_start = SteadyClock::now();
    auto sample_result = publisher.loan_slice_uninit(payload_len);
    if (!sample_result.has_value()) {
        std::cerr << "[coracam] loan failed, dropping frame"
                  << " channel=" << channel_type << " frame_index=" << frame_index << '\n';
        return PublishResult{false, steady_elapsed_us(publish_start, SteadyClock::now())};
    }
    auto& sample = *sample_result;
    auto& header = sample.user_header_mut();
    header.timestamp_us = timestamp_us;
    header.width = width;
    header.height = height;
    header.pixel_format = pixel_format;
    header.frame_index = frame_index;

    auto frame_slice = iox2::bb::ImmutableSlice<uint8_t>(payload_ptr, payload_len);
    auto initialized = sample.write_from_slice(frame_slice);
    send(std::move(initialized)).value();
    return PublishResult{true, steady_elapsed_us(publish_start, SteadyClock::now())};
}

// Update the idle_seconds counter when no sample arrived during the last
// reader.receive() call. Emits a periodic log at 10s / 30s / minute marks.
auto update_idle_seconds(const std::string& prog, ChannelStatus& status,
                         SteadyClock::time_point last_sample) -> void {
    const auto idle =
        std::chrono::duration_cast<std::chrono::seconds>(SteadyClock::now() - last_sample).count();
    if (idle > 0 && static_cast<uint64_t>(idle) != status.idle_seconds) {
        status.idle_seconds = static_cast<uint64_t>(idle);
        if (status.idle_seconds == kFirstIdleLogSeconds || status.idle_seconds == 30 ||
            (status.idle_seconds % 60) == 0) {
            std::cerr << prog << ": no Cora samples for " << status.idle_seconds << "s\n";
        }
    }
}

}  // namespace

// ---------------------------------------------------------------------------
// ChannelWorker
// ---------------------------------------------------------------------------

ChannelWorker::ChannelWorker(ChannelWorkerConfig cfg) : cfg_(std::move(cfg)) {}

ChannelWorker::~ChannelWorker() {
    stop();
}

void ChannelWorker::start() {
    thread_ = std::thread(&ChannelWorker::worker_loop, this);
}

void ChannelWorker::stop() {
    stop_requested_.store(true, std::memory_order_release);
    if (thread_.joinable()) {
        thread_.join();
    }
}

ChannelStatus ChannelWorker::status() const noexcept {
    return status_;
}

void ChannelWorker::worker_loop() {
    if (cfg_.dds_topic_name.empty()) {
        worker_loop_mock();
    } else {
        worker_loop_dds();
    }
}

// ---------------------------------------------------------------------------
// Cora SDK typed reader path
// ---------------------------------------------------------------------------

void ChannelWorker::worker_loop_dds() {
    if (cfg_.kind == ChannelKind::H264AnnexB) {
        worker_loop_cora_h264();
    } else {
        worker_loop_cora_raw();
    }
}

void ChannelWorker::worker_loop_cora_h264() {
    using namespace iox2;

    // TODO(coracam-temp): the Cora SDK currently delivers one H264 NAL per
    // DDS sample; we coalesce SPS+PPS+IDR triples (and pass non-IDR slices
    // straight through) into Annex-B access units before publishing to
    // iceoryx. Drop the EagerCoraSdkNalAssembler path once the SDK emits
    // AU-granular samples.
    const auto prog = std::string("[coracam] ") + cfg_.channel_type;
    const auto pf = rollio::PixelFormat::H264AnnexB;
    const uint64_t initial_slice_len = static_cast<uint64_t>(cfg_.max_payload_bytes);

    uint64_t last_ts_us = 0;
    bool have_last_ts = false;
    const double frame_period_us =
        cfg_.fps > 0 ? (1'000'000.0 / static_cast<double>(cfg_.fps)) : 0.0;

    try {
        set_log_level_from_env_or(LogLevel::Warn);
        auto node = NodeBuilder().create<ServiceType::Ipc>().value();
        const auto sname = ServiceName::create(cfg_.service_name.c_str()).value();
        auto service = node.service_builder(sname)
                           .publish_subscribe<bb::Slice<uint8_t>>()
                           .user_header<rollio::CameraFrameHeader>()
                           .open_or_create()
                           .value();
        auto publisher = service.publisher_builder()
                             .initial_max_slice_len(initial_slice_len)
                             .allocation_strategy(AllocationStrategy::PowerOfTwo)
                             .create()
                             .value();

        H264Reader reader(cfg_.dds_topic_name, framework::dds::QoSConfig::reliableQoS());

        std::cerr << prog << ": cora h264 worker started"
                  << " topic=" << cfg_.dds_topic_name << " bus=" << cfg_.service_name
                  << " size=" << cfg_.width << "x" << cfg_.height << '\n';

        auto last_status = SteadyClock::now();
        ChannelStatus last_logged_status;
        PipelineWindowStats window_stats;
        auto last_sample = SteadyClock::now();
        auto last_arrival = SteadyClock::now();
        bool have_last_arrival = false;
        const bool advanced_logs = advanced_pipeline_logs_enabled();
        const auto dump_dir = h264_dump_dir();
        uint64_t frame_index = 0;
        EagerCoraSdkNalAssembler assembler;
        EagerCoraSdkNalAssembler::Counters last_logged_assembler_counters{};

        // Publish whatever the assembler now has ready. Runs all AU-level
        // bookkeeping (sps/pps/idr scan, slice-type stats, timestamp checks,
        // dump). Sets/uses callback_started for the callback-to-publish metric.
        auto drain_ready_au = [&](SteadyClock::time_point callback_started) {
            while (assembler.is_ready()) {
                std::vector<uint8_t> au_buf;
                const uint64_t au_ts_raw = assembler.take(au_buf);
                if (au_buf.empty()) {
                    continue;
                }

                bool has_sps = false, has_pps = false, has_idr = false;
                scan_sps_pps(au_buf.data(), au_buf.size(), has_sps, has_pps, has_idr);
                if (advanced_logs || !dump_dir.empty()) {
                    const auto slice_stats =
                        scan_h264_slice_types(au_buf.data(), au_buf.size());
                    const char picture_type =
                        update_h264_type_stats(status_, window_stats, slice_stats);
                    if (!dump_dir.empty()) {
                        append_h264_dump(dump_dir, cfg_.channel_type, au_buf);
                        log_h264_sample_debug(cfg_.channel_type, frame_index, au_buf, has_sps,
                                              has_pps, has_idr, picture_type, slice_stats);
                    }
                } else if (has_idr) {
                    ++status_.h264_idr_frames;
                    ++window_stats.h264_idr_frames;
                }

                if (has_idr) {
                    status_.keyframes += 1;
                }
                if (has_sps && has_pps) {
                    status_.sps_pps_seen += 1;
                    status_.codec_ready = true;
                }
                // Defensive: assembler already drops IDR-without-SPS/PPS,
                // but keep the gate so a future regression is visible.
                if (has_idr && !status_.codec_ready && !(has_sps && has_pps)) {
                    std::cerr << prog
                              << ": IDR without prior SPS/PPS, dropping until codec config seen\n";
                    status_.pre_codec_drops += 1;
                    status_.frames_dropped += 1;
                    continue;
                }

                uint64_t au_ts_us = au_ts_raw;
                const auto now_us = unix_timestamp_us();
                if (au_ts_us == 0) {
                    au_ts_us = now_us;
                    status_.timestamp_fallbacks += 1;
                } else {
                    const auto source_age = positive_source_age_us(now_us, au_ts_us);
                    status_.max_source_age_us =
                        std::max(status_.max_source_age_us, source_age);
                    window_stats.source_age_ms.observe(source_age_ms(now_us, au_ts_us));
                }

                if (have_last_ts) {
                    if (au_ts_us < last_ts_us) {
                        std::cerr << prog << ": timestamp regression " << last_ts_us << " -> "
                                  << au_ts_us << '\n';
                        status_.timestamp_regressions += 1;
                        status_.discontinuities += 1;
                    } else {
                        window_stats.source_gap_ms.observe(
                            static_cast<double>(au_ts_us - last_ts_us) / 1000.0);
                        if (frame_period_us > 0.0 &&
                            static_cast<double>(au_ts_us - last_ts_us) > 3.0 * frame_period_us) {
                            std::cerr << prog << ": timestamp gap " << (au_ts_us - last_ts_us)
                                      << "us > 3 frame periods\n";
                            status_.timestamp_gaps += 1;
                            status_.discontinuities += 1;
                        }
                    }
                }
                last_ts_us = au_ts_us;
                have_last_ts = true;

                const auto publish_result =
                    publish_frame(publisher, au_ts_us, cfg_.width, cfg_.height, pf, frame_index,
                                  au_buf.data(), au_buf.size(), cfg_.channel_type);
                status_.max_publish_us =
                    std::max(status_.max_publish_us, publish_result.elapsed_us);
                window_stats.publish_ms.observe(
                    static_cast<double>(publish_result.elapsed_us) / 1000.0);
                const auto callback_to_publish_us =
                    steady_elapsed_us(callback_started, SteadyClock::now());
                status_.max_callback_to_publish_us =
                    std::max(status_.max_callback_to_publish_us, callback_to_publish_us);
                window_stats.callback_to_publish_ms.observe(
                    static_cast<double>(callback_to_publish_us) / 1000.0);
                if (!publish_result.ok) {
                    status_.frames_dropped += 1;
                } else {
                    status_.frames_published += 1;
                    status_.payload_bytes_published += static_cast<uint64_t>(au_buf.size());
                    window_stats.payload_out_bytes.observe(static_cast<double>(au_buf.size()));
                }
                ++frame_index;
            }
        };

        while (!stop_requested_.load(std::memory_order_acquire)) {
            const auto receive_started = SteadyClock::now();
            auto msg = reader.receive(200);
            const auto receive_finished = SteadyClock::now();
            if (stop_requested_.load(std::memory_order_acquire)) {
                break;
            }
            if (!msg) {
                update_idle_seconds(prog, status_, last_sample);
                continue;
            }
            window_stats.dds_wait_ms.observe(steady_elapsed_ms(receive_started, receive_finished));
            if (have_last_arrival) {
                window_stats.recv_gap_ms.observe(steady_elapsed_ms(last_arrival, receive_finished));
            }
            last_arrival = receive_finished;
            have_last_arrival = true;
            last_sample = receive_finished;
            status_.idle_seconds = 0;
            status_.frames_received += 1;

            const auto& pkt = msg->data();
            const auto& data = pkt.data();
            status_.payload_bytes_received += static_cast<uint64_t>(data.size());
            window_stats.payload_in_bytes.observe(static_cast<double>(data.size()));

            if (data.empty()) {
                std::cerr << prog << ": empty payload, dropping\n";
                status_.frames_dropped += 1;
                continue;
            }
            if (data.size() > cfg_.max_payload_bytes) {
                std::cerr << prog << ": oversized payload " << data.size() << " > "
                          << cfg_.max_payload_bytes << ", dropping\n";
                status_.frames_dropped += 1;
                continue;
            }
            if (!has_annexb_start_code(data.data(), data.size())) {
                std::cerr << prog << ": missing Annex-B start code, dropping\n";
                status_.frames_dropped += 1;
                continue;
            }
            // Diagnostic: foxglove `format` should be "h264".
            if (!pkt.format().empty()) {
                const auto& f = pkt.format();
                bool is_h264 = (f.size() == 4) && (f[0] == 'h' || f[0] == 'H') && (f[1] == '2') &&
                               (f[2] == '6') && (f[3] == '4');
                if (!is_h264) {
                    status_.encoding_mismatches += 1;
                }
            }

            // Cap the pending AU buffer against the per-channel max payload
            // before we let the assembler grow further.
            if (assembler.pending_bytes() + data.size() > cfg_.max_payload_bytes) {
                std::cerr << prog << ": assembler pending+sample "
                          << (assembler.pending_bytes() + data.size()) << " > "
                          << cfg_.max_payload_bytes << ", resetting assembler\n";
                assembler.flush();
                if (assembler.is_ready()) {
                    std::vector<uint8_t> discard;
                    assembler.take(discard);
                }
                status_.frames_dropped += 1;
                continue;
            }

            uint64_t sample_ts_us = 0;
            if (pkt.timestamp().sec() > 0 || pkt.timestamp().nanosec() > 0) {
                sample_ts_us = stamp_to_us(pkt.timestamp().sec(), pkt.timestamp().nanosec());
            }

            const auto callback_started = receive_finished;
            assembler.feed(data.data(), data.size(), sample_ts_us);
            drain_ready_au(callback_started);

            const auto now = SteadyClock::now();
            if (now - last_status >= kStatusLogInterval) {
                const auto elapsed_sec = steady_elapsed_ms(last_status, now) / 1000.0;
                if (advanced_logs) {
                    log_pipeline_status(prog, status_, last_logged_status, window_stats, elapsed_sec,
                                        true);
                } else {
                    log_pipeline_summary(prog, status_, last_logged_status, window_stats,
                                         elapsed_sec, true);
                }
                const auto& asm_c = assembler.counters();
                const auto delta_orphan_pps = asm_c.orphan_pps - last_logged_assembler_counters.orphan_pps;
                const auto delta_orphan_idr = asm_c.orphan_idr - last_logged_assembler_counters.orphan_idr;
                const auto delta_slice_break =
                    asm_c.slice_breaks_param_set - last_logged_assembler_counters.slice_breaks_param_set;
                const auto delta_resets =
                    asm_c.param_set_resets - last_logged_assembler_counters.param_set_resets;
                const auto delta_unknown =
                    asm_c.unknown_nal - last_logged_assembler_counters.unknown_nal;
                if (delta_orphan_pps != 0U || delta_orphan_idr != 0U || delta_slice_break != 0U ||
                    delta_resets != 0U || delta_unknown != 0U) {
                    std::cerr << prog << ": assembler"
                              << " orphan_pps=" << delta_orphan_pps
                              << " orphan_idr=" << delta_orphan_idr
                              << " slice_breaks_param_set=" << delta_slice_break
                              << " param_set_resets=" << delta_resets
                              << " unknown_nal=" << delta_unknown << '\n';
                }
                last_logged_assembler_counters = asm_c;
                last_logged_status = status_;
                window_stats.reset();
                last_status = now;
            }
        }

        // Shutdown: drain any partial AU still buffered.
        assembler.flush();
        drain_ready_au(SteadyClock::now());

        std::cerr << prog << ": cora h264 worker stopped"
                  << " received=" << status_.frames_received
                  << " published=" << status_.frames_published
                  << " dropped=" << status_.frames_dropped
                  << " bytes_in=" << status_.payload_bytes_received
                  << " bytes_out=" << status_.payload_bytes_published << '\n';

    } catch (const std::exception& ex) {
        std::cerr << "[coracam] " << cfg_.channel_type << ": cora h264 worker error: " << ex.what()
                  << '\n';
    }
}

void ChannelWorker::worker_loop_cora_raw() {
    using namespace iox2;

    const auto prog = std::string("[coracam] ") + cfg_.channel_type;
    const auto pf = rollio::PixelFormat::Bgr24;
    const uint64_t initial_slice_len = static_cast<uint64_t>(cfg_.width) * cfg_.height * 3U;

    uint64_t last_ts_us = 0;
    bool have_last_ts = false;
    const double frame_period_us =
        cfg_.fps > 0 ? (1'000'000.0 / static_cast<double>(cfg_.fps)) : 0.0;
    std::vector<uint8_t> raw_convert_buf;

    try {
        set_log_level_from_env_or(LogLevel::Warn);
        auto node = NodeBuilder().create<ServiceType::Ipc>().value();
        const auto sname = ServiceName::create(cfg_.service_name.c_str()).value();
        auto service = node.service_builder(sname)
                           .publish_subscribe<bb::Slice<uint8_t>>()
                           .user_header<rollio::CameraFrameHeader>()
                           .open_or_create()
                           .value();
        auto publisher = service.publisher_builder()
                             .initial_max_slice_len(initial_slice_len)
                             .allocation_strategy(AllocationStrategy::PowerOfTwo)
                             .create()
                             .value();

        RawImageReader reader(cfg_.dds_topic_name, framework::dds::QoSConfig::bestEffortQoS());

        std::cerr << prog << ": cora raw worker started"
                  << " topic=" << cfg_.dds_topic_name << " bus=" << cfg_.service_name
                  << " size=" << cfg_.width << "x" << cfg_.height << '\n';

        auto last_status = SteadyClock::now();
        ChannelStatus last_logged_status;
        PipelineWindowStats window_stats;
        auto last_sample = SteadyClock::now();
        auto last_arrival = SteadyClock::now();
        bool have_last_arrival = false;
        const bool advanced_logs = advanced_pipeline_logs_enabled();
        uint64_t frame_index = 0;

        while (!stop_requested_.load(std::memory_order_acquire)) {
            const auto receive_started = SteadyClock::now();
            auto msg = reader.receive(200);
            const auto receive_finished = SteadyClock::now();
            if (stop_requested_.load(std::memory_order_acquire)) {
                break;
            }
            if (!msg) {
                update_idle_seconds(prog, status_, last_sample);
                continue;
            }
            window_stats.dds_wait_ms.observe(steady_elapsed_ms(receive_started, receive_finished));
            if (have_last_arrival) {
                window_stats.recv_gap_ms.observe(steady_elapsed_ms(last_arrival, receive_finished));
            }
            last_arrival = receive_finished;
            have_last_arrival = true;
            last_sample = receive_finished;
            status_.idle_seconds = 0;
            status_.frames_received += 1;

            const auto& img = msg->data();
            const auto& encoding = img.encoding();
            // The Cora SDK message stores bulk data in std::vector<uint8_t>.
            // We need a mutable copy when we have to repack stride or convert
            // colour space. Hold a pointer + length we can rebind to either
            // the original buffer or raw_convert_buf as needed.
            const std::vector<uint8_t>& src_data = img.data();
            const uint8_t* payload_ptr = src_data.data();
            uint64_t payload_len = src_data.size();
            uint32_t width = img.width() != 0 ? img.width() : cfg_.width;
            uint32_t height = img.height() != 0 ? img.height() : cfg_.height;
            const uint32_t step = img.step();
            status_.payload_bytes_received += payload_len;
            window_stats.payload_in_bytes.observe(static_cast<double>(payload_len));

            if (payload_len == 0) {
                std::cerr << prog << ": empty raw payload, dropping\n";
                status_.frames_dropped += 1;
                continue;
            }
            if (payload_len > cfg_.max_payload_bytes) {
                std::cerr << prog << ": oversized raw payload " << payload_len << " > "
                          << cfg_.max_payload_bytes << ", dropping\n";
                status_.frames_dropped += 1;
                continue;
            }

            if (!cfg_.raw_expected_encoding.empty() && !encoding.empty() &&
                encoding != cfg_.raw_expected_encoding) {
                std::cerr << prog << ": raw encoding mismatch '" << encoding << "' vs expected '"
                          << cfg_.raw_expected_encoding << "', dropping\n";
                status_.encoding_mismatches += 1;
                status_.frames_dropped += 1;
                continue;
            }

            if (img.width() != 0 && img.width() != cfg_.width) {
                status_.dimension_drifts += 1;
            }
            if (img.height() != 0 && img.height() != cfg_.height) {
                status_.dimension_drifts += 1;
            }

            if (encoding == "nv12") {
                const auto convert_started = SteadyClock::now();
                if (!nv12_to_bgr24(src_data, width, height, step, raw_convert_buf)) {
                    std::cerr << prog << ": nv12 conversion failed (payload=" << src_data.size()
                              << " w=" << width << " h=" << height << " step=" << step
                              << "), dropping\n";
                    status_.frames_dropped += 1;
                    continue;
                }
                window_stats.raw_convert_ms.observe(
                    steady_elapsed_ms(convert_started, SteadyClock::now()));
                payload_ptr = raw_convert_buf.data();
                payload_len = raw_convert_buf.size();
                status_.stride_repacks += 1;
            } else if (encoding == "mono8") {
                const auto convert_started = SteadyClock::now();
                if (!mono8_to_bgr24(src_data, width, height, step, raw_convert_buf)) {
                    std::cerr << prog << ": mono8 conversion failed (payload=" << src_data.size()
                              << " w=" << width << " h=" << height << " step=" << step
                              << "), dropping\n";
                    status_.frames_dropped += 1;
                    continue;
                }
                window_stats.raw_convert_ms.observe(
                    steady_elapsed_ms(convert_started, SteadyClock::now()));
                payload_ptr = raw_convert_buf.data();
                payload_len = raw_convert_buf.size();
                status_.stride_repacks += 1;
            } else if (encoding == "bgr8" || encoding.empty()) {
                const uint32_t natural_step = width * 3U;
                const uint32_t actual_step = step != 0 ? step : natural_step;
                const uint64_t expected_with_step = static_cast<uint64_t>(height) * actual_step;
                if (src_data.size() != expected_with_step) {
                    std::cerr << prog << ": raw payload size mismatch " << src_data.size()
                              << " vs expected " << expected_with_step << " (w=" << width
                              << " h=" << height << " step=" << actual_step << " enc=" << encoding
                              << "), dropping\n";
                    status_.frames_dropped += 1;
                    continue;
                }
                if (actual_step != natural_step) {
                    const auto convert_started = SteadyClock::now();
                    raw_convert_buf.resize(static_cast<std::size_t>(height) * natural_step);
                    for (uint32_t y = 0; y < height; ++y) {
                        std::memcpy(
                            raw_convert_buf.data() + static_cast<std::size_t>(y) * natural_step,
                            src_data.data() + static_cast<std::size_t>(y) * actual_step,
                            natural_step);
                    }
                    window_stats.raw_convert_ms.observe(
                        steady_elapsed_ms(convert_started, SteadyClock::now()));
                    payload_ptr = raw_convert_buf.data();
                    payload_len = raw_convert_buf.size();
                    status_.stride_repacks += 1;
                }
            } else {
                std::cerr << prog << ": unsupported raw encoding '" << encoding << "', dropping\n";
                status_.encoding_mismatches += 1;
                status_.frames_dropped += 1;
                continue;
            }

            uint64_t ts_us = 0;
            const auto& stamp = img.header().stamp();
            if (stamp.sec() > 0 || stamp.nanosec() > 0) {
                ts_us = stamp_to_us(stamp.sec(), stamp.nanosec());
            }
            const auto now_us = unix_timestamp_us();
            if (ts_us == 0) {
                ts_us = now_us;
                status_.timestamp_fallbacks += 1;
            } else {
                const auto source_age = positive_source_age_us(now_us, ts_us);
                status_.max_source_age_us = std::max(status_.max_source_age_us, source_age);
                window_stats.source_age_ms.observe(source_age_ms(now_us, ts_us));
            }

            if (have_last_ts) {
                if (ts_us < last_ts_us) {
                    std::cerr << prog << ": timestamp regression " << last_ts_us << " -> " << ts_us
                              << '\n';
                    status_.timestamp_regressions += 1;
                    status_.discontinuities += 1;
                } else {
                    window_stats.source_gap_ms.observe(static_cast<double>(ts_us - last_ts_us) /
                                                       1000.0);
                    if (frame_period_us > 0.0 &&
                        static_cast<double>(ts_us - last_ts_us) > 3.0 * frame_period_us) {
                        std::cerr << prog << ": timestamp gap " << (ts_us - last_ts_us)
                                  << "us > 3 frame periods\n";
                        status_.timestamp_gaps += 1;
                        status_.discontinuities += 1;
                    }
                }
            }
            last_ts_us = ts_us;
            have_last_ts = true;

            const auto callback_started = receive_finished;
            const auto publish_result =
                publish_frame(publisher, ts_us, width, height, pf, frame_index, payload_ptr,
                              payload_len, cfg_.channel_type);
            status_.max_publish_us = std::max(status_.max_publish_us, publish_result.elapsed_us);
            window_stats.publish_ms.observe(static_cast<double>(publish_result.elapsed_us) /
                                            1000.0);
            const auto callback_to_publish_us =
                steady_elapsed_us(callback_started, SteadyClock::now());
            status_.max_callback_to_publish_us =
                std::max(status_.max_callback_to_publish_us, callback_to_publish_us);
            window_stats.callback_to_publish_ms.observe(
                static_cast<double>(callback_to_publish_us) / 1000.0);
            if (!publish_result.ok) {
                status_.frames_dropped += 1;
            } else {
                status_.frames_published += 1;
                status_.payload_bytes_published += payload_len;
                window_stats.payload_out_bytes.observe(static_cast<double>(payload_len));
            }

            ++frame_index;

            const auto now = SteadyClock::now();
            if (now - last_status >= kStatusLogInterval) {
                const auto elapsed_sec = steady_elapsed_ms(last_status, now) / 1000.0;
                if (advanced_logs) {
                    log_pipeline_status(prog, status_, last_logged_status, window_stats, elapsed_sec,
                                        false);
                } else {
                    log_pipeline_summary(prog, status_, last_logged_status, window_stats,
                                         elapsed_sec, false);
                }
                last_logged_status = status_;
                window_stats.reset();
                last_status = now;
            }
        }

        std::cerr << prog << ": cora raw worker stopped"
                  << " received=" << status_.frames_received
                  << " published=" << status_.frames_published
                  << " dropped=" << status_.frames_dropped
                  << " bytes_in=" << status_.payload_bytes_received
                  << " bytes_out=" << status_.payload_bytes_published << '\n';

    } catch (const std::exception& ex) {
        std::cerr << "[coracam] " << cfg_.channel_type << ": cora raw worker error: " << ex.what()
                  << '\n';
    }
}

// ---------------------------------------------------------------------------
// Mock generator path — used when dds_topic_name is empty (tests / offline)
// ---------------------------------------------------------------------------

void ChannelWorker::worker_loop_mock() {
    using namespace iox2;

    const bool is_h264 = (cfg_.kind == ChannelKind::H264AnnexB);
    const std::string prog = std::string("[coracam-mock] ") + cfg_.channel_type;

    const uint64_t initial_slice_len = is_h264
                                           ? static_cast<uint64_t>(256U * 1024U)
                                           : static_cast<uint64_t>(cfg_.width) * cfg_.height * 3U;

    try {
        set_log_level_from_env_or(LogLevel::Warn);
        auto node = NodeBuilder().create<ServiceType::Ipc>().value();
        const auto sname = ServiceName::create(cfg_.service_name.c_str()).value();
        auto service = node.service_builder(sname)
                           .publish_subscribe<bb::Slice<uint8_t>>()
                           .user_header<rollio::CameraFrameHeader>()
                           .open_or_create()
                           .value();
        auto publisher = service.publisher_builder()
                             .initial_max_slice_len(initial_slice_len)
                             .allocation_strategy(AllocationStrategy::PowerOfTwo)
                             .create()
                             .value();

        std::cerr << prog << ": mock worker started"
                  << " service=" << cfg_.service_name
                  << " kind=" << (is_h264 ? "h264-annex-b" : "bgr24") << " size=" << cfg_.width
                  << "x" << cfg_.height << " fps=" << cfg_.fps << '\n';

        const auto frame_period = std::chrono::duration<double>(1.0 / std::max(1U, cfg_.fps));
        auto next_frame = SteadyClock::now();
        auto last_status = SteadyClock::now();
        const bool advanced_logs = advanced_pipeline_logs_enabled();
        uint64_t frame_index = 0;
        constexpr uint64_t kKeyframeInterval = 25;

        std::vector<uint8_t> raw_buf;

        while (!stop_requested_.load(std::memory_order_acquire)) {
            uint64_t payload_len = 0;
            const uint8_t* payload_ptr = nullptr;
            std::vector<uint8_t> h264_au;

            if (is_h264) {
                const bool is_keyframe = (frame_index % kKeyframeInterval == 0);
                h264_au = make_mock_annexb_au(is_keyframe, frame_index);
                payload_len = h264_au.size();
                payload_ptr = h264_au.data();
                if (is_keyframe) {
                    status_.keyframes += 1;
                    status_.sps_pps_seen += 1;
                }
            } else {
                generate_raw_bgr24(cfg_.width, cfg_.height, frame_index, raw_buf);
                payload_len = raw_buf.size();
                payload_ptr = raw_buf.data();
            }

            const auto publish_result = publish_frame(
                publisher, unix_timestamp_us(), cfg_.width, cfg_.height,
                is_h264 ? rollio::PixelFormat::H264AnnexB : rollio::PixelFormat::Bgr24, frame_index,
                payload_ptr, payload_len, cfg_.channel_type);
            if (!publish_result.ok) {
                status_.frames_dropped += 1;
            } else {
                status_.frames_published += 1;
            }

            ++frame_index;

            if (SteadyClock::now() - last_status >= kStatusLogInterval) {
                std::cerr << prog << ": status"
                          << (advanced_logs ? "" : " summary")
                          << " published=" << status_.frames_published
                          << " dropped=" << status_.frames_dropped;
                if (advanced_logs && is_h264) {
                    std::cerr << " keyframes=" << status_.keyframes
                              << " sps_pps=" << status_.sps_pps_seen;
                }
                std::cerr << '\n';
                last_status = SteadyClock::now();
            }

            next_frame += std::chrono::duration_cast<SteadyClock::duration>(frame_period);
            const auto now = SteadyClock::now();
            if (next_frame > now) {
                std::this_thread::sleep_until(next_frame);
            } else {
                next_frame = now;
            }
        }

        std::cerr << prog << ": mock worker stopped"
                  << " published=" << status_.frames_published << '\n';

    } catch (const std::exception& ex) {
        std::cerr << "[coracam-mock] " << cfg_.channel_type << ": worker error: " << ex.what()
                  << '\n';
    }
}

}  // namespace rollio::coracam
