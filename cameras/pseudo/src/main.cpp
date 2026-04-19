#include "iox2/iceoryx2.hpp"
#include "rollio/device_config.hpp"
#include "rollio/topic_names.hpp"
#include "rollio/types.h"

#include <algorithm>
#include <array>
#include <chrono>
#include <cmath>
#include <cstdint>
#include <iomanip>
#include <iostream>
#include <optional>
#include <sstream>
#include <stdexcept>
#include <string>
#include <thread>
#include <vector>

namespace {

using SteadyClock = std::chrono::steady_clock;
using SystemClock = std::chrono::system_clock;

constexpr std::array<std::array<uint8_t, 3>, 8> BAR_COLORS {{
    {255, 255, 255},
    {255, 255, 0},
    {0, 255, 255},
    {0, 255, 0},
    {255, 0, 255},
    {255, 0, 0},
    {0, 0, 255},
    {0, 0, 0},
}};

constexpr std::array<std::array<uint8_t, 7>, 12> DIGIT_FONT {{
    std::array<uint8_t, 7> {0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110},
    std::array<uint8_t, 7> {0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110},
    std::array<uint8_t, 7> {0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111},
    std::array<uint8_t, 7> {0b11111, 0b00010, 0b00100, 0b00010, 0b00001, 0b10001, 0b01110},
    std::array<uint8_t, 7> {0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010},
    std::array<uint8_t, 7> {0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110},
    std::array<uint8_t, 7> {0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110},
    std::array<uint8_t, 7> {0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000},
    std::array<uint8_t, 7> {0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110},
    std::array<uint8_t, 7> {0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100},
    std::array<uint8_t, 7> {0b00000, 0b00100, 0b00000, 0b00000, 0b00000, 0b00100, 0b00000},
    std::array<uint8_t, 7> {0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00100},
}};

constexpr uint32_t FONT_W = 5;
constexpr uint32_t FONT_H = 7;
constexpr uint32_t FONT_SCALE = 2;
constexpr uint32_t CHAR_W = (FONT_W + 1) * FONT_SCALE;
constexpr uint32_t CHAR_H = FONT_H * FONT_SCALE;

auto print_usage() -> void {
    std::cerr
        << "Usage: rollio-device-pseudo-camera <probe|validate|capabilities|run> [args...]\n"
        << "  probe [--count N]\n"
        << "  validate <id>\n"
        << "  capabilities <id>\n"
        << "  run (--config <path> | --config-inline <toml>)\n";
}

auto optional_arg(int argc, char* argv[], const std::string& name) -> std::optional<std::string> {
    for (auto index = 0; index + 1 < argc; ++index) {
        if (argv[index] == name) {
            return std::string(argv[index + 1]);
        }
    }

    return std::nullopt;
}

auto parse_u32_arg(int argc, char* argv[], const std::string& name, uint32_t default_value) -> uint32_t {
    const auto value = optional_arg(argc, argv, name);
    if (!value.has_value()) {
        return default_value;
    }

    return static_cast<uint32_t>(std::stoul(*value));
}

auto timestamp_ns() -> uint64_t {
    return static_cast<uint64_t>(
        std::chrono::duration_cast<std::chrono::nanoseconds>(
            SystemClock::now().time_since_epoch()
        )
            .count()
    );
}

auto format_overlay_text(const double elapsed_secs, const uint64_t frame_index) -> std::string {
    const auto total_seconds = static_cast<uint64_t>(elapsed_secs);
    const auto hours = total_seconds / 3600;
    const auto minutes = (total_seconds % 3600) / 60;
    const auto seconds = total_seconds % 60;
    const auto centiseconds = static_cast<uint64_t>((elapsed_secs - std::floor(elapsed_secs)) * 100.0);

    std::ostringstream output;
    output << std::setfill('0') << std::setw(2) << hours << ':'
           << std::setw(2) << minutes << ':'
           << std::setw(2) << seconds << '.'
           << std::setw(2) << centiseconds << " #"
           << frame_index;
    return output.str();
}

auto glyph_index_for_char(const char ch) -> std::optional<std::size_t> {
    if (ch >= '0' && ch <= '9') {
        return static_cast<std::size_t>(ch - '0');
    }
    if (ch == ':') {
        return 10;
    }
    if (ch == '.') {
        return 11;
    }
    return std::nullopt;
}

auto draw_text(
    std::vector<uint8_t>& buffer,
    const uint32_t width,
    const uint32_t height,
    const uint32_t start_x,
    const uint32_t start_y,
    const std::string& text
) -> void {
    for (std::size_t text_idx = 0; text_idx < text.size(); ++text_idx) {
        const auto glyph_index = glyph_index_for_char(text[text_idx]);
        if (!glyph_index.has_value()) {
            continue;
        }

        const auto& glyph = DIGIT_FONT[*glyph_index];
        const auto glyph_x = start_x + static_cast<uint32_t>(text_idx) * CHAR_W;
        for (uint32_t gy = 0; gy < FONT_H; ++gy) {
            const auto row_bits = glyph[gy];
            for (uint32_t gx = 0; gx < FONT_W; ++gx) {
                if ((row_bits & (1U << (FONT_W - 1U - gx))) == 0) {
                    continue;
                }

                for (uint32_t sy = 0; sy < FONT_SCALE; ++sy) {
                    for (uint32_t sx = 0; sx < FONT_SCALE; ++sx) {
                        const auto px_x = glyph_x + gx * FONT_SCALE + sx;
                        const auto px_y = start_y + gy * FONT_SCALE + sy;
                        if (px_x >= width || px_y >= height) {
                            continue;
                        }

                        const auto pixel = (static_cast<std::size_t>(px_y) * width + px_x) * 3U;
                        buffer[pixel] = 220;
                        buffer[pixel + 1] = 255;
                        buffer[pixel + 2] = 220;
                    }
                }
            }
        }
    }
}

auto burn_overlay(
    std::vector<uint8_t>& buffer,
    const uint32_t width,
    const uint32_t height,
    const double elapsed_secs,
    const uint64_t frame_index
) -> void {
    const auto text = format_overlay_text(elapsed_secs, frame_index);
    const auto margin = FONT_SCALE * 2U;
    const auto box_w = static_cast<uint32_t>(text.size()) * CHAR_W + margin * 2U;
    const auto box_h = CHAR_H + margin * 2U;
    if (box_w >= width || box_h >= height) {
        return;
    }

    const auto box_x = (width - box_w) / 2U;
    const auto box_y = height - box_h - margin;
    for (uint32_t y = 0; y < box_h; ++y) {
        for (uint32_t x = 0; x < box_w; ++x) {
            const auto px = ((static_cast<std::size_t>(box_y + y) * width) + box_x + x) * 3U;
            buffer[px] /= 3U;
            buffer[px + 1] /= 3U;
            buffer[px + 2] /= 3U;
        }
    }

    draw_text(buffer, width, height, box_x + margin, box_y + margin, text);
}

auto generate_frame(
    std::vector<uint8_t>& buffer,
    const uint32_t width,
    const uint32_t height,
    const double elapsed_secs,
    const uint64_t frame_index
) -> void {
    const auto bar_width = std::max<std::size_t>(1U, width / BAR_COLORS.size());
    const auto scroll = static_cast<std::size_t>(elapsed_secs * static_cast<double>(bar_width));

    for (uint32_t y = 0; y < height; ++y) {
        const auto row_offset = static_cast<std::size_t>(y) * width * 3U;
        const auto v_mod = std::sin((static_cast<double>(y) / std::max(1U, height)) * 0.3 + elapsed_secs * 0.1) * 0.15 + 0.85;
        for (uint32_t x = 0; x < width; ++x) {
            const auto shifted_x = (x + scroll) % width;
            const auto bar_idx = std::min<std::size_t>(shifted_x / bar_width, BAR_COLORS.size() - 1U);
            const auto pixel = row_offset + static_cast<std::size_t>(x) * 3U;
            buffer[pixel] = static_cast<uint8_t>(std::min(255.0, BAR_COLORS[bar_idx][0] * v_mod));
            buffer[pixel + 1] = static_cast<uint8_t>(std::min(255.0, BAR_COLORS[bar_idx][1] * v_mod));
            buffer[pixel + 2] = static_cast<uint8_t>(std::min(255.0, BAR_COLORS[bar_idx][2] * v_mod));
        }
    }

    burn_overlay(buffer, width, height, elapsed_secs, frame_index);
}

auto load_run_config(int argc, char* argv[]) -> rollio::CameraDeviceConfig {
    const auto config_path = optional_arg(argc, argv, "--config");
    const auto config_inline = optional_arg(argc, argv, "--config-inline");
    if (config_path.has_value() == config_inline.has_value()) {
        throw std::runtime_error("run requires exactly one of --config or --config-inline");
    }

    auto config = config_inline.has_value()
        ? rollio::parse_camera_device_config(*config_inline)
        : rollio::load_camera_device_config_from_file(*config_path);

    if (config.type != "camera") {
        throw std::runtime_error("pseudo camera requires type = \"camera\"");
    }
    if (config.driver != "pseudo") {
        throw std::runtime_error("pseudo camera requires driver = \"pseudo\"");
    }
    if (config.pixel_format != rollio::PixelFormat::Rgb24) {
        throw std::runtime_error("pseudo camera currently supports only rgb24 output");
    }

    return config;
}

auto run_camera(const rollio::CameraDeviceConfig& config) -> int {
    using namespace iox2;

    set_log_level_from_env_or(LogLevel::Info);
    auto node = NodeBuilder()
                    .create<ServiceType::Ipc>()
                    .value();

    const auto service_name_str = rollio::camera_frames_service_name(config.name);
    const auto service_name = ServiceName::create(service_name_str.c_str()).value();
    auto frame_service = node.service_builder(service_name)
                             .publish_subscribe<bb::Slice<uint8_t>>()
                             .user_header<rollio::CameraFrameHeader>()
                             .open_or_create()
                             .value();
    const auto payload_size = static_cast<uint64_t>(config.width) * config.height * 3U;
    auto publisher = frame_service.publisher_builder()
                         .initial_max_slice_len(payload_size)
                         .allocation_strategy(AllocationStrategy::PowerOfTwo)
                         .create()
                         .value();

    const auto control_service_name = ServiceName::create(rollio::CONTROL_EVENTS_SERVICE).value();
    auto control_service = node.service_builder(control_service_name)
                               .publish_subscribe<rollio::ControlEvent>()
                               .open_or_create()
                               .value();
    auto control_subscriber = control_service.subscriber_builder().create().value();

    std::vector<uint8_t> frame_buffer(static_cast<std::size_t>(payload_size), 0U);
    const auto frame_period = std::chrono::duration<double>(1.0 / std::max<uint32_t>(1U, config.fps));
    auto next_frame = SteadyClock::now();
    const auto start_time = SteadyClock::now();
    auto last_status = SteadyClock::now();
    auto frame_index = uint64_t {0};

    std::cerr << "rollio-device-pseudo-camera: device=" << config.name
              << " size=" << config.width << "x" << config.height
              << " fps=" << config.fps << '\n';

    while (true) {
        auto control_sample = control_subscriber.receive().value();
        while (control_sample.has_value()) {
            if (control_sample->payload().tag == rollio::ControlEventTag::Shutdown) {
                std::cerr << "rollio-device-pseudo-camera: shutdown received for " << config.name << '\n';
                return 0;
            }
            control_sample = control_subscriber.receive().value();
        }

        const auto elapsed_secs = std::chrono::duration<double>(SteadyClock::now() - start_time).count();
        generate_frame(frame_buffer, config.width, config.height, elapsed_secs, frame_index);

        auto sample = publisher.loan_slice_uninit(payload_size).value();
        auto& header = sample.user_header_mut();
        header.timestamp_ns = timestamp_ns();
        header.width = config.width;
        header.height = config.height;
        header.pixel_format = config.pixel_format;
        header.frame_index = frame_index;

        const auto latest_timestamp_ns = header.timestamp_ns;
        auto initialized_sample = sample.write_from_fn(
            [&](const uint64_t byte_idx) -> uint8_t { return frame_buffer[static_cast<std::size_t>(byte_idx)]; }
        );
        send(std::move(initialized_sample)).value();

        frame_index += 1U;
        if (SteadyClock::now() - last_status >= std::chrono::seconds(1)) {
    std::cerr << "rollio-device-pseudo-camera: device=" << config.name
              << " frame_index=" << frame_index
                      << " latest_timestamp_ns=" << latest_timestamp_ns
                      << " active=true\n";
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
}

} // namespace

auto main(int argc, char* argv[]) -> int {
    try {
        if (argc < 2) {
            print_usage();
            return 1;
        }

        const std::string command = argv[1];
        if (command == "probe") {
            const auto count = parse_u32_arg(argc - 1, argv + 1, "--count", 1);
            std::cout << "[";
            for (uint32_t index = 0; index < count; ++index) {
                if (index != 0) {
                    std::cout << ",";
                }
                std::cout
                    << "{\"id\":\"pseudo_cam_" << index
                    << "\",\"name\":\"pseudo_cam_" << index
                    << "\",\"driver\":\"pseudo\",\"type\":\"camera\"}";
            }
            std::cout << "]\n";
            return 0;
        }

        if (command == "validate") {
            if (argc < 3) {
                throw std::runtime_error("validate requires an id");
            }
            std::cout << "{\"valid\":true,\"id\":\"" << argv[2] << "\"}\n";
            return 0;
        }

        if (command == "capabilities") {
            if (argc < 3) {
                throw std::runtime_error("capabilities requires an id");
            }
            std::cout
                << "{\"id\":\"" << argv[2]
                << "\",\"pixel_formats\":[\"rgb24\"],\"streams\":[\"color\"],\"profiles\":["
                << "{\"width\":640,\"height\":480,\"fps\":30},"
                << "{\"width\":1280,\"height\":720,\"fps\":30}"
                << "]}\n";
            return 0;
        }

        if (command == "run") {
            return run_camera(load_run_config(argc - 1, argv + 1));
        }

        throw std::runtime_error("unknown subcommand: " + command);
    } catch (const std::exception& error) {
        std::cerr << "rollio-device-pseudo-camera: " << error.what() << '\n';
        return 1;
    }
}
