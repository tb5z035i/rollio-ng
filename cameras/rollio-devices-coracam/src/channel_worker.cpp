#include "channel_worker.hpp"

#include <algorithm>
#include <chrono>
#include <cstdint>
#include <cstdlib>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <iostream>
#include <sstream>
#include <stdexcept>
#include <vector>

#include "cora_subscriber.hpp"
#include "cora_types.hpp"
#include "h264_annexb.hpp"
#include "iox2/iceoryx2.hpp"
#include "rollio/topic_names.hpp"
#include "rollio/types.h"

namespace rollio::coracam {

using SteadyClock = std::chrono::steady_clock;
using SystemClock = std::chrono::system_clock;

namespace {

auto unix_timestamp_us() -> uint64_t {
    return static_cast<uint64_t>(
        std::chrono::duration_cast<std::chrono::microseconds>(SystemClock::now().time_since_epoch())
            .count());
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
                           bool has_idr) -> void {
    if (frame_index >= 20U && !has_sps && !has_pps && !has_idr) {
        return;
    }
    std::cerr << "[coracam] " << channel_type << ": h264 sample idx=" << frame_index
              << " bytes=" << data.size() << " nals=["
              << h264_nal_type_list(data.data(), data.size()) << "] sps=" << has_sps
              << " pps=" << has_pps << " idr=" << has_idr << '\n';
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

auto nv12_to_bgr24(const CoraRawImage& img, uint32_t width, uint32_t height,
                   std::vector<uint8_t>& out) -> bool {
    if (width == 0 || height == 0 || (height % 2U) != 0) {
        return false;
    }
    const uint32_t y_step = img.step != 0 ? img.step : width;
    if (y_step < width) {
        return false;
    }
    const uint64_t y_len = static_cast<uint64_t>(height) * y_step;
    const uint64_t uv_len = static_cast<uint64_t>(height / 2U) * y_step;
    if (img.data.size() < y_len + uv_len) {
        return false;
    }

    const auto* y_plane = img.data.data();
    const auto* uv_plane = img.data.data() + y_len;
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

auto mono8_to_bgr24(const CoraRawImage& img, uint32_t width, uint32_t height,
                    std::vector<uint8_t>& out) -> bool {
    if (width == 0 || height == 0) {
        return false;
    }
    const uint32_t step = img.step != 0 ? img.step : width;
    if (step < width) {
        return false;
    }
    const uint64_t expected = static_cast<uint64_t>(height) * step;
    if (img.data.size() < expected) {
        return false;
    }

    out.resize(static_cast<std::size_t>(width) * height * 3U);
    for (uint32_t y = 0; y < height; ++y) {
        const auto* src_row = img.data.data() + static_cast<std::size_t>(y) * step;
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
                   const std::string& channel_type) -> bool {
    auto sample_result = publisher.loan_slice_uninit(payload_len);
    if (!sample_result.has_value()) {
        std::cerr << "[coracam] loan failed, dropping frame"
                  << " channel=" << channel_type << " frame_index=" << frame_index << '\n';
        return false;
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
    return true;
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
// DDS subscriber path — real Cora / Fast-DDS topics
// ---------------------------------------------------------------------------

void ChannelWorker::worker_loop_dds() {
    using namespace iox2;

    const bool is_h264 = (cfg_.kind == ChannelKind::H264AnnexB);
    const auto prog = std::string("[coracam] ") + cfg_.channel_type;
    const auto pf = is_h264 ? rollio::PixelFormat::H264AnnexB : rollio::PixelFormat::Bgr24;

    // Initial iceoryx2 slice capacity. For raw we size to the configured
    // width*height*3; for H264 to the max packet budget.
    const uint64_t initial_slice_len = is_h264
                                           ? static_cast<uint64_t>(cfg_.max_payload_bytes)
                                           : static_cast<uint64_t>(cfg_.width) * cfg_.height * 3U;

    // Per-worker mutable state for discontinuity + codec-config tracking.
    // We keep these out of ChannelStatus because they are pure scratch.
    uint64_t last_ts_us = 0;
    bool have_last_ts = false;
    const double frame_period_us =
        cfg_.fps > 0 ? (1'000'000.0 / static_cast<double>(cfg_.fps)) : 0.0;
    // Reusable raw conversion buffer.
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

        // Create the Fast-DDS subscriber.
        CoraSubscriber dds_sub(cfg_.dds_topic_name, cfg_.dds_type_name, cfg_.dds_domain_id);

        std::cerr << prog << ": dds worker started"
                  << " topic=" << cfg_.dds_topic_name << " type=" << cfg_.dds_type_name
                  << " bus=" << cfg_.service_name
                  << " kind=" << (is_h264 ? "h264-annex-b" : "bgr24") << " size=" << cfg_.width
                  << "x" << cfg_.height << '\n';

        auto last_status = SteadyClock::now();
        auto last_sample = SteadyClock::now();
        uint64_t frame_index = 0;

        while (!stop_requested_.load(std::memory_order_acquire)) {
            // Block for up to 200 ms waiting for a DDS sample.
            CoraSample sample;
            const bool got = dds_sub.take_next(sample, std::chrono::milliseconds(200));

            if (stop_requested_.load(std::memory_order_acquire)) {
                break;
            }
            if (!got) {
                // Update idle-seconds counter without spamming.
                const auto idle = std::chrono::duration_cast<std::chrono::seconds>(
                                      SteadyClock::now() - last_sample)
                                      .count();
                if (idle > 0 && static_cast<uint64_t>(idle) != status_.idle_seconds) {
                    status_.idle_seconds = static_cast<uint64_t>(idle);
                    if (status_.idle_seconds == 5 || status_.idle_seconds == 30 ||
                        (status_.idle_seconds % 60) == 0) {
                        std::cerr << prog << ": no DDS samples for " << status_.idle_seconds
                                  << "s\n";
                    }
                }
                continue;
            }
            last_sample = SteadyClock::now();
            status_.idle_seconds = 0;

            status_.frames_received += 1;

            // Basic payload sanity checks.
            if (sample.payload.empty()) {
                std::cerr << prog << ": empty payload, dropping\n";
                status_.frames_dropped += 1;
                continue;
            }
            if (sample.payload.size() > cfg_.max_payload_bytes + 4U /* encap */) {
                std::cerr << prog << ": oversized payload " << sample.payload.size() << " > "
                          << cfg_.max_payload_bytes << ", dropping\n";
                status_.frames_dropped += 1;
                continue;
            }

            uint64_t ts_us = sample.source_timestamp_us;
            uint32_t width = cfg_.width;
            uint32_t height = cfg_.height;
            const uint8_t* payload_ptr = nullptr;
            uint64_t payload_len = 0;

            if (is_h264) {
                // Parse foxglove_msgs/msg/CompressedVideo.
                // This type carries no width/height/is_keyframe metadata;
                // keyframe detection relies exclusively on NAL scan.
                FoxgloveCompressedVideo pkt;
                if (!parse_foxglove_compressed_video(sample.payload.data(), sample.payload.size(),
                                                     pkt)) {
                    std::cerr << prog << ": h264 CDR parse failed, dropping\n";
                    dump_cdr_hex("h264 raw", sample.payload.data(), sample.payload.size());
                    status_.frames_dropped += 1;
                    continue;
                }

                // Validate Annex-B start code.
                if (!has_annexb_start_code(pkt.data.data(), pkt.data.size())) {
                    std::cerr << prog << ": missing Annex-B start code, dropping\n";
                    dump_cdr_hex("h264 data", pkt.data.data(), pkt.data.size());
                    status_.frames_dropped += 1;
                    continue;
                }

                // Always scan NALs — foxglove provides no is_keyframe metadata.
                bool has_sps = false, has_pps = false, has_idr = false;
                scan_sps_pps(pkt.data.data(), pkt.data.size(), has_sps, has_pps, has_idr);
                if (const auto dir = h264_dump_dir(); !dir.empty()) {
                    append_h264_dump(dir, cfg_.channel_type, pkt.data);
                    log_h264_sample_debug(cfg_.channel_type, frame_index, pkt.data, has_sps,
                                          has_pps, has_idr);
                }

                if (has_idr) {
                    status_.keyframes += 1;
                }
                if (has_sps && has_pps) {
                    status_.sps_pps_seen += 1;
                    status_.codec_ready = true;
                }

                // Codec-config gating.
                if (has_idr && !status_.codec_ready && !(has_sps && has_pps)) {
                    std::cerr << prog
                              << ": IDR without prior SPS/PPS, dropping until codec config seen\n";
                    status_.pre_codec_drops += 1;
                    status_.frames_dropped += 1;
                    continue;
                }

                // Use packet timestamp if available, else DDS source ts.
                if (pkt.timestamp.sec > 0 || pkt.timestamp.nanosec > 0) {
                    ts_us = pkt.timestamp.to_us();
                }

                payload_ptr = pkt.data.data();
                payload_len = pkt.data.size();

                // Timestamp regression / gap check.
                if (have_last_ts && ts_us != 0) {
                    if (ts_us < last_ts_us) {
                        std::cerr << prog << ": timestamp regression " << last_ts_us << " -> "
                                  << ts_us << '\n';
                        status_.timestamp_regressions += 1;
                        status_.discontinuities += 1;
                    } else if (frame_period_us > 0.0 &&
                               static_cast<double>(ts_us - last_ts_us) > 3.0 * frame_period_us) {
                        std::cerr << prog << ": timestamp gap " << (ts_us - last_ts_us)
                                  << "us > 3 frame periods\n";
                        status_.timestamp_gaps += 1;
                        status_.discontinuities += 1;
                    }
                }
                if (ts_us != 0) {
                    last_ts_us = ts_us;
                    have_last_ts = true;
                }

                if (!publish_frame(publisher, ts_us > 0 ? ts_us : unix_timestamp_us(), width,
                                   height, pf, frame_index, payload_ptr, payload_len,
                                   cfg_.channel_type)) {
                    status_.frames_dropped += 1;
                } else {
                    status_.frames_published += 1;
                }

            } else {
                // Parse raw image.
                CoraRawImage img;
                if (!parse_cora_raw_image(sample.payload.data(), sample.payload.size(), img)) {
                    std::cerr << prog << ": raw CDR parse failed, dropping\n";
                    dump_cdr_hex("raw image", sample.payload.data(), sample.payload.size());
                    status_.frames_dropped += 1;
                    continue;
                }

                if (!cfg_.raw_expected_encoding.empty() && !img.encoding.empty() &&
                    img.encoding != cfg_.raw_expected_encoding) {
                    std::cerr << prog << ": raw encoding mismatch '" << img.encoding
                              << "' vs expected '" << cfg_.raw_expected_encoding << "', dropping\n";
                    status_.encoding_mismatches += 1;
                    status_.frames_dropped += 1;
                    continue;
                }

                // Dimension drift.
                if (img.width != 0 && img.width != cfg_.width) {
                    status_.dimension_drifts += 1;
                }
                if (img.height != 0 && img.height != cfg_.height) {
                    status_.dimension_drifts += 1;
                }

                // Validate pixel count + optional stride re-packing.
                if (img.width != 0 && img.height != 0) {
                    if (img.encoding == "nv12") {
                        if (!nv12_to_bgr24(img, img.width, img.height, raw_convert_buf)) {
                            std::cerr << prog << ": nv12 conversion failed"
                                      << " (payload=" << img.data.size() << " w=" << img.width
                                      << " h=" << img.height << " step=" << img.step
                                      << "), dropping\n";
                            status_.frames_dropped += 1;
                            continue;
                        }
                        img.data.swap(raw_convert_buf);
                        status_.stride_repacks += 1;
                    } else if (img.encoding == "mono8") {
                        if (!mono8_to_bgr24(img, img.width, img.height, raw_convert_buf)) {
                            std::cerr << prog << ": mono8 conversion failed"
                                      << " (payload=" << img.data.size() << " w=" << img.width
                                      << " h=" << img.height << " step=" << img.step
                                      << "), dropping\n";
                            status_.frames_dropped += 1;
                            continue;
                        }
                        img.data.swap(raw_convert_buf);
                        status_.stride_repacks += 1;
                    } else if (img.encoding == "bgr8" || img.encoding.empty()) {
                        const uint32_t natural_step = img.width * 3U;
                        const uint32_t step = img.step != 0 ? img.step : natural_step;
                        const uint64_t expected_with_step =
                            static_cast<uint64_t>(img.height) * step;
                        if (img.data.size() != expected_with_step) {
                            std::cerr << prog << ": raw payload size mismatch " << img.data.size()
                                      << " vs expected " << expected_with_step
                                      << " (w=" << img.width << " h=" << img.height
                                      << " step=" << step << " enc=" << img.encoding
                                      << "), dropping\n";
                            status_.frames_dropped += 1;
                            continue;
                        }
                        if (step != natural_step) {
                            // Row-copy strip the row padding so downstream gets a
                            // contiguous width*height*3 buffer.
                            raw_convert_buf.resize(static_cast<std::size_t>(img.height) *
                                                   natural_step);
                            for (uint32_t y = 0; y < img.height; ++y) {
                                std::memcpy(raw_convert_buf.data() +
                                                static_cast<std::size_t>(y) * natural_step,
                                            img.data.data() + static_cast<std::size_t>(y) * step,
                                            natural_step);
                            }
                            img.data.swap(raw_convert_buf);
                            status_.stride_repacks += 1;
                        }
                    } else {
                        std::cerr << prog << ": unsupported raw encoding '" << img.encoding
                                  << "', dropping\n";
                        status_.encoding_mismatches += 1;
                        status_.frames_dropped += 1;
                        continue;
                    }
                }

                if (img.header.stamp.sec > 0 || img.header.stamp.nanosec > 0) {
                    ts_us = img.header.stamp.to_us();
                }
                if (img.width != 0) {
                    width = img.width;
                }
                if (img.height != 0) {
                    height = img.height;
                }

                payload_ptr = img.data.data();
                payload_len = img.data.size();

                // Timestamp regression / gap check.
                if (have_last_ts && ts_us != 0) {
                    if (ts_us < last_ts_us) {
                        std::cerr << prog << ": timestamp regression " << last_ts_us << " -> "
                                  << ts_us << '\n';
                        status_.timestamp_regressions += 1;
                        status_.discontinuities += 1;
                    } else if (frame_period_us > 0.0 &&
                               static_cast<double>(ts_us - last_ts_us) > 3.0 * frame_period_us) {
                        std::cerr << prog << ": timestamp gap " << (ts_us - last_ts_us)
                                  << "us > 3 frame periods\n";
                        status_.timestamp_gaps += 1;
                        status_.discontinuities += 1;
                    }
                }
                if (ts_us != 0) {
                    last_ts_us = ts_us;
                    have_last_ts = true;
                }

                if (!publish_frame(publisher, ts_us > 0 ? ts_us : unix_timestamp_us(), width,
                                   height, pf, frame_index, payload_ptr, payload_len,
                                   cfg_.channel_type)) {
                    status_.frames_dropped += 1;
                } else {
                    status_.frames_published += 1;
                }
            }

            ++frame_index;

            // Periodic status log every 5 seconds.
            if (SteadyClock::now() - last_status >= std::chrono::seconds(5)) {
                std::cerr << prog << ": status"
                          << " received=" << status_.frames_received
                          << " published=" << status_.frames_published
                          << " dropped=" << status_.frames_dropped;
                if (is_h264) {
                    std::cerr << " keyframes=" << status_.keyframes
                              << " sps_pps=" << status_.sps_pps_seen
                              << " pre_codec_drops=" << status_.pre_codec_drops;
                }
                if (status_.discontinuities > 0) {
                    std::cerr << " ts_regr=" << status_.timestamp_regressions
                              << " ts_gaps=" << status_.timestamp_gaps;
                }
                if (status_.encoding_mismatches > 0 || status_.stride_repacks > 0 ||
                    status_.dimension_drifts > 0) {
                    std::cerr << " enc_mism=" << status_.encoding_mismatches
                              << " stride_repacks=" << status_.stride_repacks
                              << " dim_drifts=" << status_.dimension_drifts;
                }
                std::cerr << '\n';
                last_status = SteadyClock::now();
            }
        }

        dds_sub.stop();
        std::cerr << prog << ": dds worker stopped"
                  << " published=" << status_.frames_published << '\n';

    } catch (const std::exception& ex) {
        std::cerr << "[coracam] " << cfg_.channel_type << ": dds worker error: " << ex.what()
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

            if (!publish_frame(
                    publisher, unix_timestamp_us(), cfg_.width, cfg_.height,
                    is_h264 ? rollio::PixelFormat::H264AnnexB : rollio::PixelFormat::Bgr24,
                    frame_index, payload_ptr, payload_len, cfg_.channel_type)) {
                status_.frames_dropped += 1;
            } else {
                status_.frames_published += 1;
            }

            ++frame_index;

            if (SteadyClock::now() - last_status >= std::chrono::seconds(5)) {
                std::cerr << prog << ": status"
                          << " published=" << status_.frames_published
                          << " dropped=" << status_.frames_dropped;
                if (is_h264) {
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
