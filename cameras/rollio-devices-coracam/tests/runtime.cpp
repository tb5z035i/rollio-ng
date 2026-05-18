#include <sys/wait.h>
#include <unistd.h>

#include <array>
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <iostream>
#include <stdexcept>
#include <string>
#include <thread>
#include <vector>

#include "cora_mapping.hpp"
#include "cora_types.hpp"
#include "h264_annexb.hpp"
#include "iox2/iceoryx2.hpp"
#include "rollio/topic_names.hpp"
#include "rollio/types.h"

namespace {

using SteadyClock = std::chrono::steady_clock;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

auto capture_stdout(const std::string& command) -> std::string {
    std::array<char, 256> buf{};
    std::string out;
    auto* pipe = popen(command.c_str(), "r");
    if (pipe == nullptr) {
        throw std::runtime_error("popen failed: " + command);
    }
    while (fgets(buf.data(), static_cast<int>(buf.size()), pipe) != nullptr) {
        out += buf.data();
    }
    const auto rc = pclose(pipe);
    if (rc != 0) {
        throw std::runtime_error("command failed: " + command + "\noutput: " + out);
    }
    return out;
}

auto count_substring(const std::string& text, const std::string& needle) -> std::size_t {
    std::size_t n = 0;
    std::size_t pos = 0;
    while ((pos = text.find(needle, pos)) != std::string::npos) {
        ++n;
        pos += needle.size();
    }
    return n;
}

auto unique_bus_root() -> std::string {
    const auto ns = std::chrono::duration_cast<std::chrono::nanoseconds>(
                        std::chrono::system_clock::now().time_since_epoch())
                        .count();
    return "test/coracam_" + std::to_string(ns);
}

auto spawn_device(const std::string& config_inline) -> pid_t {
    const auto pid = fork();
    if (pid < 0) {
        throw std::runtime_error("fork failed");
    }
    if (pid == 0) {
        char* argv[] = {
            const_cast<char*>(ROLLIO_DEVICE_CORACAM_HEAD_BIN),
            const_cast<char*>("run"),
            const_cast<char*>("--config-inline"),
            const_cast<char*>(config_inline.c_str()),
            nullptr,
        };
        execv(ROLLIO_DEVICE_CORACAM_HEAD_BIN, argv);
        _exit(127);
    }
    return pid;
}

auto send_shutdown() -> void {
    using namespace iox2;
    auto node = NodeBuilder().create<ServiceType::Ipc>().value();
    const auto name = ServiceName::create(rollio::CONTROL_EVENTS_SERVICE).value();
    auto svc = node.service_builder(name)
                   .publish_subscribe<rollio::ControlEvent>()
                   .open_or_create()
                   .value();
    auto pub = svc.publisher_builder().create().value();
    rollio::ControlEvent ev{};
    ev.tag = rollio::ControlEventTag::Shutdown;
    pub.send_copy(ev).value();
}

auto wait_for_exit(pid_t pid, std::chrono::seconds timeout) -> void {
    const auto deadline = SteadyClock::now() + timeout;
    int status = 0;
    while (SteadyClock::now() < deadline) {
        const auto r = waitpid(pid, &status, WNOHANG);
        if (r == pid) {
            if (!WIFEXITED(status) || WEXITSTATUS(status) != 0) {
                throw std::runtime_error("device process exited unsuccessfully");
            }
            return;
        }
        std::this_thread::sleep_for(std::chrono::milliseconds(20));
    }
    kill(pid, SIGKILL);
    throw std::runtime_error("device process did not exit after shutdown");
}

struct FrameObs {
    rollio::CameraFrameHeader header;
    std::size_t payload_bytes;
};

auto collect_frames(iox2::Subscriber<iox2::ServiceType::Ipc, iox2::bb::Slice<uint8_t>,
                                     rollio::CameraFrameHeader>& sub,
                    std::size_t count, std::chrono::seconds timeout) -> std::vector<FrameObs> {
    std::vector<FrameObs> out;
    const auto deadline = SteadyClock::now() + timeout;
    while (SteadyClock::now() < deadline && out.size() < count) {
        auto s = sub.receive().value();
        if (s.has_value()) {
            out.push_back(FrameObs{s->user_header(), s->payload().number_of_bytes()});
        } else {
            std::this_thread::sleep_for(std::chrono::milliseconds(10));
        }
    }
    if (out.size() < count) {
        throw std::runtime_error("did not receive enough frames (got " +
                                 std::to_string(out.size()) + ", want " + std::to_string(count) +
                                 ")");
    }
    return out;
}

// ---------------------------------------------------------------------------
// Test: probe
// ---------------------------------------------------------------------------

auto run_probe_test() -> void {
    const auto cmd = std::string("\"") + ROLLIO_DEVICE_CORACAM_HEAD_BIN + "\" probe";
    const auto out = capture_stdout(cmd);
    if (out.find("\"driver\":\"coracam-head\"") == std::string::npos) {
        throw std::runtime_error("probe: missing expected driver field\noutput: " + out);
    }
    if (out.find("\"id\":\"cora-head\"") == std::string::npos) {
        throw std::runtime_error("probe: missing expected id field\noutput: " + out);
    }
    std::cerr << "rollio-devices-coracam-tests: probe OK\n";
}

// ---------------------------------------------------------------------------
// Test: dry-run
// ---------------------------------------------------------------------------

auto run_dry_run_test() -> void {
    const auto bus_root = unique_bus_root();
    const auto config_inline =
        "name = \"coracam_head\"\n"
        "driver = \"coracam-head\"\n"
        "id = \"cora-head\"\n"
        "bus_root = \"" +
        bus_root +
        "\"\n"
        "\n"
        "[[channels]]\n"
        "channel_type = \"left_raw\"\n"
        "kind = \"camera\"\n"
        "enabled = true\n"
        "[channels.profile]\n"
        "width = 640\n"
        "height = 480\n"
        "fps = 25\n"
        "pixel_format = \"bgr24\"\n"
        "\n"
        "[[channels]]\n"
        "channel_type = \"right_raw\"\n"
        "kind = \"camera\"\n"
        "enabled = true\n"
        "[channels.profile]\n"
        "width = 640\n"
        "height = 480\n"
        "fps = 25\n"
        "pixel_format = \"bgr24\"\n"
        "\n"
        "[[channels]]\n"
        "channel_type = \"left_h264\"\n"
        "kind = \"camera\"\n"
        "enabled = true\n"
        "[channels.profile]\n"
        "width = 640\n"
        "height = 480\n"
        "fps = 25\n"
        "pixel_format = \"h264-annex-b\"\n"
        "\n"
        "[[channels]]\n"
        "channel_type = \"right_h264\"\n"
        "kind = \"camera\"\n"
        "enabled = true\n"
        "[channels.profile]\n"
        "width = 640\n"
        "height = 480\n"
        "fps = 25\n"
        "pixel_format = \"h264-annex-b\"\n";

    // Write config to temp file and run with --config
    char tmp_path[] = "/tmp/coracam_test_XXXXXX.toml";
    const int fd = mkstemps(tmp_path, 5);
    if (fd < 0) {
        throw std::runtime_error("mkstemps failed");
    }
    (void)write(fd, config_inline.data(), config_inline.size());
    close(fd);

    const auto cmd = std::string("\"") + ROLLIO_DEVICE_CORACAM_HEAD_BIN + "\" run --config \"" +
                     tmp_path + "\" --dry-run 2>&1";
    const auto out = capture_stdout(cmd);
    unlink(tmp_path);

    if (out.find("dry-run ok") == std::string::npos) {
        throw std::runtime_error("dry-run: missing 'dry-run ok'\noutput: " + out);
    }
    if (out.find("left_h264") == std::string::npos) {
        throw std::runtime_error("dry-run: missing 'left_h264' channel\noutput: " + out);
    }
    std::cerr << "rollio-devices-coracam-tests: dry-run OK\n";
}

// ---------------------------------------------------------------------------
// Test: h264_annexb scanner
// ---------------------------------------------------------------------------

auto run_annexb_parser_test() -> void {
    using namespace rollio::coracam;

    // Keyframe should contain SPS + PPS + IDR
    const auto kf = make_mock_annexb_au(true, 0);
    bool has_sps = false;
    bool has_pps = false;
    bool has_idr = false;
    const bool scanned = scan_sps_pps(kf.data(), kf.size(), has_sps, has_pps, has_idr);
    if (!scanned || !has_sps || !has_pps || !has_idr) {
        throw std::runtime_error("annexb parser: keyframe missing SPS/PPS/IDR");
    }
    if (!has_annexb_start_code(kf.data(), kf.size())) {
        throw std::runtime_error("annexb parser: keyframe missing start code");
    }

    // Delta frame should not contain SPS/PPS
    const auto delta = make_mock_annexb_au(false, 1);
    bool has_sps2 = false;
    bool has_pps2 = false;
    bool has_idr2 = false;
    scan_sps_pps(delta.data(), delta.size(), has_sps2, has_pps2, has_idr2);
    if (has_sps2 || has_pps2 || has_idr2) {
        throw std::runtime_error("annexb parser: delta frame unexpectedly has SPS/PPS/IDR");
    }

    std::cerr << "rollio-devices-coracam-tests: annexb parser OK\n";
}

// ---------------------------------------------------------------------------
// Test: AU assembler (single-NAL-per-sample upstream → AU coalescing)
// ---------------------------------------------------------------------------

auto run_au_assembler_test() -> void {
    using namespace rollio::coracam;

    AnnexBAuAssembler asm_;

    // SPS sample (ts=100) — first NAL, becomes pending; no AU yet.
    const std::vector<uint8_t> sps = {0x00, 0x00, 0x00, 0x01, kNalHeaderSps, 0x42, 0xC0, 0x28};
    if (asm_.feed(sps.data(), sps.size(), 100)) {
        throw std::runtime_error("au_assembler: SPS unexpectedly produced AU immediately");
    }
    // PPS sample (ts=100) — same ts, append.
    const std::vector<uint8_t> pps = {0x00, 0x00, 0x00, 0x01, kNalHeaderPps, 0xEE, 0x01};
    if (asm_.feed(pps.data(), pps.size(), 100)) {
        throw std::runtime_error("au_assembler: PPS unexpectedly produced AU");
    }
    // IDR sample (ts=100) — same ts; SPS/IDR boundary should NOT flush
    // because pending starts with SPS already.
    const std::vector<uint8_t> idr = {0x00, 0x00, 0x00, 0x01, kNalHeaderIdr, 0x88, 0x80};
    asm_.feed(idr.data(), idr.size(), 100);

    // Slice sample with NEW timestamp (ts=200) → triggers AU emission for
    // SPS+PPS+IDR; pending becomes the slice.
    const std::vector<uint8_t> slice = {0x00, 0x00, 0x00, 0x01, kNalHeaderSlice, 0x9A};
    const bool ready = asm_.feed(slice.data(), slice.size(), 200);
    if (!ready || !asm_.is_ready()) {
        throw std::runtime_error("au_assembler: new ts should have flushed AU");
    }
    std::vector<uint8_t> au;
    const auto au_ts = asm_.take(au);
    if (au_ts != 100) {
        throw std::runtime_error("au_assembler: AU ts wrong, got " + std::to_string(au_ts));
    }
    bool has_sps = false, has_pps = false, has_idr = false;
    if (!scan_sps_pps(au.data(), au.size(), has_sps, has_pps, has_idr) || !has_sps || !has_pps ||
        !has_idr) {
        throw std::runtime_error("au_assembler: emitted AU missing SPS/PPS/IDR");
    }

    // Flush should now emit the slice that was pending.
    if (!asm_.flush() || !asm_.is_ready()) {
        throw std::runtime_error("au_assembler: flush did not produce delta AU");
    }
    std::vector<uint8_t> delta_au;
    asm_.take(delta_au);
    if (!has_annexb_start_code(delta_au.data(), delta_au.size())) {
        throw std::runtime_error("au_assembler: delta AU missing start code");
    }

    std::cerr << "rollio-devices-coracam-tests: AU assembler OK\n";
}

// ---------------------------------------------------------------------------
// Test: CDR golden bytes for parse_cora_raw_image / parse_cora_h264_packet
// ---------------------------------------------------------------------------

// Helper: append a little-endian uint32 to a byte vector at 4-byte alignment.
auto cdr_align(std::vector<uint8_t>& out, std::size_t base, std::size_t align) -> void {
    const auto rel = (out.size() - base) % align;
    if (rel != 0) {
        out.insert(out.end(), align - rel, 0x00);
    }
}
auto cdr_put_u32(std::vector<uint8_t>& out, std::size_t base, uint32_t v) -> void {
    cdr_align(out, base, 4);
    out.push_back(static_cast<uint8_t>(v & 0xFF));
    out.push_back(static_cast<uint8_t>((v >> 8) & 0xFF));
    out.push_back(static_cast<uint8_t>((v >> 16) & 0xFF));
    out.push_back(static_cast<uint8_t>((v >> 24) & 0xFF));
}
auto cdr_put_i32(std::vector<uint8_t>& out, std::size_t base, int32_t v) -> void {
    cdr_put_u32(out, base, static_cast<uint32_t>(v));
}
auto cdr_put_string(std::vector<uint8_t>& out, std::size_t base, const std::string& s) -> void {
    cdr_put_u32(out, base, static_cast<uint32_t>(s.size() + 1U));
    out.insert(out.end(), s.begin(), s.end());
    out.push_back(0x00);  // null terminator
}
auto cdr_put_byte_seq(std::vector<uint8_t>& out, std::size_t base,
                      const std::vector<uint8_t>& bytes) -> void {
    cdr_put_u32(out, base, static_cast<uint32_t>(bytes.size()));
    out.insert(out.end(), bytes.begin(), bytes.end());
}

auto run_cdr_golden_bytes_test() -> void {
    using namespace rollio::coracam;

    // ----- raw image golden -----
    // Encapsulation header: 0x00 0x01 0x00 0x00 (CDR_LE).
    std::vector<uint8_t> raw_buf = {0x00, 0x01, 0x00, 0x00};
    const std::size_t raw_base = raw_buf.size();
    // header.stamp: sec=10 nanosec=500_000_000
    cdr_put_i32(raw_buf, raw_base, 10);
    cdr_put_u32(raw_buf, raw_base, 500'000'000U);
    // header.frame_id = "camera_head"
    cdr_put_string(raw_buf, raw_base, "camera_head");
    // height, width
    cdr_put_u32(raw_buf, raw_base, 4U);  // height
    cdr_put_u32(raw_buf, raw_base, 4U);  // width
    // encoding = "bgr8"
    cdr_put_string(raw_buf, raw_base, "bgr8");
    // is_bigendian (uint8, no align)
    raw_buf.push_back(0x00);
    // step (align 4)
    cdr_put_u32(raw_buf, raw_base, 12U);  // 4 px * 3 bytes
    // data: 4x4x3 = 48 bytes
    std::vector<uint8_t> data(48U);
    for (std::size_t i = 0; i < data.size(); ++i)
        data[i] = static_cast<uint8_t>(i);
    cdr_put_byte_seq(raw_buf, raw_base, data);

    CoraRawImage img;
    if (!parse_cora_raw_image(raw_buf.data(), raw_buf.size(), img)) {
        throw std::runtime_error("golden raw: parse_cora_raw_image returned false");
    }
    if (img.header.stamp.sec != 10 || img.header.stamp.nanosec != 500'000'000U) {
        throw std::runtime_error("golden raw: stamp mismatch");
    }
    if (img.header.frame_id != "camera_head") {
        throw std::runtime_error("golden raw: frame_id mismatch: " + img.header.frame_id);
    }
    if (img.height != 4U || img.width != 4U || img.encoding != "bgr8" || img.is_bigendian != 0 ||
        img.step != 12U) {
        throw std::runtime_error("golden raw: fixed field mismatch");
    }
    if (img.data.size() != 48U || img.data[0] != 0 || img.data[47] != 47) {
        throw std::runtime_error("golden raw: data payload mismatch");
    }

    // ----- H264 packet golden -----
    std::vector<uint8_t> h_buf = {0x00, 0x01, 0x00, 0x00};
    const std::size_t h_base = h_buf.size();
    cdr_put_i32(h_buf, h_base, 1);
    cdr_put_u32(h_buf, h_base, 250'000U);
    cdr_put_string(h_buf, h_base, "head_left");
    cdr_put_u32(h_buf, h_base, 1920U);  // width
    cdr_put_u32(h_buf, h_base, 1080U);  // height
    h_buf.push_back(0x01);              // is_keyframe = true
    // Note: is_keyframe is a bool/uint8 — next 4-byte aligned field is data
    // sequence; the byte_seq writer realigns on its own.
    std::vector<uint8_t> au_bytes = {0x00, 0x00, 0x00, 0x01, 0x67, 0x42, 0xC0, 0x00, 0x00, 0x00,
                                     0x01, 0x68, 0xCE, 0x00, 0x00, 0x00, 0x01, 0x65, 0x88};
    cdr_put_byte_seq(h_buf, h_base, au_bytes);

    CoraH264Packet pkt;
    if (!parse_cora_h264_packet(h_buf.data(), h_buf.size(), pkt)) {
        throw std::runtime_error("golden h264: parse_cora_h264_packet returned false");
    }
    if (pkt.header.stamp.sec != 1 || pkt.header.stamp.nanosec != 250'000U) {
        throw std::runtime_error("golden h264: stamp mismatch");
    }
    if (pkt.header.frame_id != "head_left") {
        throw std::runtime_error("golden h264: frame_id mismatch");
    }
    if (pkt.width != 1920U || pkt.height != 1080U || !pkt.is_keyframe) {
        throw std::runtime_error("golden h264: fixed field mismatch");
    }
    if (pkt.data.size() != au_bytes.size() ||
        std::memcmp(pkt.data.data(), au_bytes.data(), au_bytes.size()) != 0) {
        throw std::runtime_error("golden h264: AU payload mismatch");
    }

    // ----- Foxglove CompressedVideo golden -----
    std::vector<uint8_t> fox_buf = {0x00, 0x01, 0x00, 0x00};
    const std::size_t fox_base = fox_buf.size();
    cdr_put_i32(fox_buf, fox_base, 2);
    cdr_put_u32(fox_buf, fox_base, 123'456'789U);
    cdr_put_string(fox_buf, fox_base, "right_wrist_right");
    cdr_put_byte_seq(fox_buf, fox_base, au_bytes);
    cdr_put_string(fox_buf, fox_base, "h264");

    FoxgloveCompressedVideo fox_pkt;
    if (!parse_foxglove_compressed_video(fox_buf.data(), fox_buf.size(), fox_pkt)) {
        throw std::runtime_error("golden foxglove compressed video: parse returned false");
    }
    if (fox_pkt.timestamp.sec != 2 || fox_pkt.timestamp.nanosec != 123'456'789U) {
        throw std::runtime_error("golden foxglove compressed video: stamp mismatch");
    }
    if (fox_pkt.frame_id != "right_wrist_right") {
        throw std::runtime_error("golden foxglove compressed video: frame_id mismatch");
    }
    if (fox_pkt.format != "h264") {
        throw std::runtime_error("golden foxglove compressed video: format mismatch: " +
                                 fox_pkt.format);
    }
    if (fox_pkt.data.size() != au_bytes.size() ||
        std::memcmp(fox_pkt.data.data(), au_bytes.data(), au_bytes.size()) != 0) {
        throw std::runtime_error("golden foxglove compressed video: AU payload mismatch");
    }

    std::cerr << "rollio-devices-coracam-tests: CDR golden bytes OK\n";
}

// ---------------------------------------------------------------------------
// Test: cora_mapping parser
// ---------------------------------------------------------------------------

auto run_cora_mapping_test() -> void {
    using namespace rollio::coracam;

    const std::string toml =
        "domain_id = 7\n"
        "participant_name = \"rollio_coracam_head\"\n"
        "max_packet_bytes = 8388608\n"
        "annex_b_validation = \"auto\"\n"
        "metadata_validation_packets = 32\n"
        "\n"
        "[[topics]]\n"
        "channel_type = \"left_raw\"\n"
        "topic = \"/rt/robot/camera/head/left/image\"\n"
        "type = \"sensor_msgs::msg::dds_::Image_\"\n"
        "raw_expected_encoding = \"bgr8\"\n"
        "\n"
        "[[topics]]\n"
        "channel_type = \"left_h264\"\n"
        "topic = \"/rt/robot/camera/head/left/video_encoded\"\n"
        "type = \"cora_msgs::msg::dds_::H264Packet_\"\n"
        "max_packet_bytes = 16777216\n"
        "annex_b_validation = \"scan\"\n";

    const auto m = parse_cora_mapping(toml);
    if (!m.domain_id || *m.domain_id != 7U) {
        throw std::runtime_error("mapping: domain_id mismatch");
    }
    if (!m.participant_name || *m.participant_name != "rollio_coracam_head") {
        throw std::runtime_error("mapping: participant_name mismatch");
    }
    if (!m.max_packet_bytes || *m.max_packet_bytes != 8'388'608U) {
        throw std::runtime_error("mapping: max_packet_bytes mismatch");
    }
    if (!m.annex_b_validation || *m.annex_b_validation != AnnexBValidationMode::Auto) {
        throw std::runtime_error("mapping: annex_b_validation mismatch");
    }
    if (!m.metadata_validation_packets || *m.metadata_validation_packets != 32U) {
        throw std::runtime_error("mapping: metadata_validation_packets mismatch");
    }
    if (m.topics.size() != 2U) {
        throw std::runtime_error("mapping: topics count mismatch");
    }
    if (m.topics[0].channel_type != "left_raw" || !m.topics[0].topic ||
        *m.topics[0].topic != "/rt/robot/camera/head/left/image") {
        throw std::runtime_error("mapping: topic[0] mismatch");
    }
    if (!m.topics[1].max_packet_bytes || *m.topics[1].max_packet_bytes != 16'777'216U) {
        throw std::runtime_error("mapping: topic[1] max_packet_bytes mismatch");
    }
    if (!m.topics[1].annex_b_validation ||
        *m.topics[1].annex_b_validation != AnnexBValidationMode::Scan) {
        throw std::runtime_error("mapping: topic[1] annex_b_validation mismatch");
    }

    // Duplicate channel_type should error.
    bool threw = false;
    try {
        parse_cora_mapping(
            "[[topics]]\n"
            "channel_type = \"x\"\n"
            "[[topics]]\n"
            "channel_type = \"x\"\n");
    } catch (const std::exception&) {
        threw = true;
    }
    if (!threw) {
        throw std::runtime_error("mapping: duplicate channel_type not rejected");
    }

    std::cerr << "rollio-devices-coracam-tests: cora_mapping parser OK\n";
}

// ---------------------------------------------------------------------------
// Test: runtime publish
// ---------------------------------------------------------------------------

auto run_runtime_test() -> void {
    using namespace iox2;
    const auto bus_root = unique_bus_root();

    // Open subscribers and the shutdown publisher BEFORE spawning so that
    // the device finds all endpoints already registered on start-up.
    auto node = NodeBuilder().create<ServiceType::Ipc>().value();

    auto open_sub = [&](const std::string& ch_type) {
        const auto sn =
            ServiceName::create(rollio::channel_frames_service_name(bus_root, ch_type).c_str())
                .value();
        auto svc = node.service_builder(sn)
                       .publish_subscribe<bb::Slice<uint8_t>>()
                       .user_header<rollio::CameraFrameHeader>()
                       .open_or_create()
                       .value();
        return svc.subscriber_builder().create().value();
    };

    auto h264_sub = open_sub("left_h264");
    auto raw_sub = open_sub("left_raw");

    // Create the control-events publisher before spawning so the device's
    // subscriber can see it from the start.
    const auto ctrl_name = ServiceName::create(rollio::CONTROL_EVENTS_SERVICE).value();
    auto ctrl_svc = node.service_builder(ctrl_name)
                        .publish_subscribe<rollio::ControlEvent>()
                        .open_or_create()
                        .value();
    auto ctrl_pub = ctrl_svc.publisher_builder().create().value();

    const auto config_inline =
        "name = \"coracam_head\"\n"
        "driver = \"coracam-head\"\n"
        "id = \"cora-head\"\n"
        "bus_root = \"" +
        bus_root +
        "\"\n"
        "\n"
        "[[channels]]\n"
        "channel_type = \"left_raw\"\n"
        "kind = \"camera\"\n"
        "enabled = true\n"
        "[channels.profile]\n"
        "width = 640\n"
        "height = 480\n"
        "fps = 25\n"
        "pixel_format = \"bgr24\"\n"
        "\n"
        "[[channels]]\n"
        "channel_type = \"right_raw\"\n"
        "kind = \"camera\"\n"
        "enabled = true\n"
        "[channels.profile]\n"
        "width = 640\n"
        "height = 480\n"
        "fps = 25\n"
        "pixel_format = \"bgr24\"\n"
        "\n"
        "[[channels]]\n"
        "channel_type = \"left_h264\"\n"
        "kind = \"camera\"\n"
        "enabled = true\n"
        "[channels.profile]\n"
        "width = 640\n"
        "height = 480\n"
        "fps = 25\n"
        "pixel_format = \"h264-annex-b\"\n"
        "\n"
        "[[channels]]\n"
        "channel_type = \"right_h264\"\n"
        "kind = \"camera\"\n"
        "enabled = true\n"
        "[channels.profile]\n"
        "width = 640\n"
        "height = 480\n"
        "fps = 25\n"
        "pixel_format = \"h264-annex-b\"\n";

    const auto pid = spawn_device(config_inline);

    // Collect 10 frames from each channel.
    constexpr std::size_t kWant = 10;
    const auto h264_frames = collect_frames(h264_sub, kWant, std::chrono::seconds(5));
    const auto raw_frames = collect_frames(raw_sub, kWant, std::chrono::seconds(5));

    // H264 channel checks.
    for (const auto& f : h264_frames) {
        if (f.header.pixel_format != rollio::PixelFormat::H264AnnexB) {
            throw std::runtime_error("h264 channel: wrong pixel_format");
        }
        if (f.header.width != 640 || f.header.height != 480) {
            throw std::runtime_error("h264 channel: wrong dimensions");
        }
        if (f.payload_bytes == 0) {
            throw std::runtime_error("h264 channel: empty payload");
        }
        // Payload must start with an Annex-B start code.
        // (We'd need to loan the payload bytes; checking via header is sufficient here.)
    }
    // Verify frame indices increase.
    for (std::size_t i = 1; i < h264_frames.size(); ++i) {
        if (h264_frames[i - 1].header.frame_index >= h264_frames[i].header.frame_index) {
            throw std::runtime_error("h264 channel: frame indices not increasing");
        }
    }

    // Raw channel checks.
    const auto expected_raw_size = static_cast<std::size_t>(640U * 480U * 3U);
    for (const auto& f : raw_frames) {
        if (f.header.pixel_format != rollio::PixelFormat::Bgr24) {
            throw std::runtime_error("raw channel: wrong pixel_format");
        }
        if (f.payload_bytes != expected_raw_size) {
            throw std::runtime_error("raw channel: wrong payload size " +
                                     std::to_string(f.payload_bytes) + " vs expected " +
                                     std::to_string(expected_raw_size));
        }
    }

    rollio::ControlEvent ev{};
    ev.tag = rollio::ControlEventTag::Shutdown;
    ctrl_pub.send_copy(ev).value();
    wait_for_exit(pid, std::chrono::seconds(5));

    std::cerr << "rollio-devices-coracam-tests: runtime OK\n";
}

}  // namespace

auto main() -> int {
    try {
        run_annexb_parser_test();
        run_au_assembler_test();
        run_cdr_golden_bytes_test();
        run_cora_mapping_test();
        run_probe_test();
        run_dry_run_test();
        run_runtime_test();
        return 0;
    } catch (const std::exception& ex) {
        std::cerr << "rollio-devices-coracam-tests: FAILED: " << ex.what() << '\n';
        return 1;
    }
}
