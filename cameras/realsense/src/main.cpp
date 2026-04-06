#include "iox2/iceoryx2.hpp"
#include "rollio/device_config.hpp"
#include "rollio/topic_names.hpp"
#include "rollio/types.h"

#if ROLLIO_HAVE_REALSENSE
#include <librealsense2/rs.hpp>
#endif

#include <chrono>
#include <cstdint>
#include <iostream>
#include <optional>
#include <sstream>
#include <stdexcept>
#include <string>
#include <string_view>
#include <thread>
#include <vector>

namespace {

using SteadyClock = std::chrono::steady_clock;

auto print_usage() -> void {
    std::cerr
        << "Usage: rollio-camera-realsense <probe|validate|capabilities|run> [args...]\n"
        << "  probe\n"
        << "  validate <serial>\n"
        << "  capabilities <serial>\n"
        << "  run (--config <path> | --config-inline <toml>)\n";
}

auto optional_arg(int argc, char* argv[], const std::string& name) -> std::optional<std::string> {
    for (auto index = 0; index + 1 < argc; ++index) {
        if (name == argv[index]) {
            return std::string(argv[index + 1]);
        }
    }

    return std::nullopt;
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
        throw std::runtime_error("realsense driver requires type = \"camera\"");
    }
    if (config.driver != "realsense") {
        throw std::runtime_error("realsense driver requires driver = \"realsense\"");
    }
    if (!config.stream.has_value()) {
        throw std::runtime_error("realsense driver requires a stream field");
    }

    return config;
}

auto pixel_format_byte_size(const rollio::PixelFormat pixel_format) -> uint32_t {
    switch (pixel_format) {
        case rollio::PixelFormat::Rgb24:
        case rollio::PixelFormat::Bgr24:
            return 3;
        case rollio::PixelFormat::Yuyv:
        case rollio::PixelFormat::Depth16:
            return 2;
        case rollio::PixelFormat::Gray8:
            return 1;
        case rollio::PixelFormat::Mjpeg:
            return 1;
    }
    return 1;
}

#if ROLLIO_HAVE_REALSENSE

struct StreamConfig {
    rs2_stream stream;
    int index;
    rs2_format format;
};

auto serialize_device(const rs2::device& device) -> std::string {
    const auto serial = device.get_info(RS2_CAMERA_INFO_SERIAL_NUMBER);
    const auto name = device.get_info(RS2_CAMERA_INFO_NAME);

    std::ostringstream output;
    output << "{\"id\":\"" << serial << "\",\"name\":\"" << name << "\",\"driver\":\"realsense\"}";
    return output.str();
}

auto find_device_by_serial(const rs2::context& context, const std::string& serial) -> std::optional<rs2::device> {
    for (const auto& device : context.query_devices()) {
        if (device.get_info(RS2_CAMERA_INFO_SERIAL_NUMBER) == serial) {
            return device;
        }
    }

    return std::nullopt;
}

auto is_transient_wait_timeout(const rs2::error& error) -> bool {
    constexpr std::string_view timeout_prefix = "Frame didn't arrive within";
    return std::string_view(error.what()).find(timeout_prefix) != std::string_view::npos;
}

auto parse_stream_config(const rollio::CameraDeviceConfig& config) -> StreamConfig {
    const auto& stream_name = *config.stream;
    if (stream_name == "color") {
        return StreamConfig {
            RS2_STREAM_COLOR,
            0,
            RS2_FORMAT_RGB8,
        };
    }
    if (stream_name == "depth") {
        return StreamConfig {
            RS2_STREAM_DEPTH,
            0,
            RS2_FORMAT_Z16,
        };
    }
    if (stream_name == "infrared") {
        return StreamConfig {
            RS2_STREAM_INFRARED,
            static_cast<int>(config.channel.value_or(1U)),
            RS2_FORMAT_Y8,
        };
    }

    throw std::runtime_error("unsupported realsense stream: " + stream_name);
}

auto print_probe_output() -> int {
    rs2::context context;
    auto devices = context.query_devices();

    std::cout << "[";
    auto first = true;
    for (const auto& device : devices) {
        if (!first) {
            std::cout << ",";
        }
        first = false;
        std::cout << serialize_device(device);
    }
    std::cout << "]\n";
    return 0;
}

auto print_validate_output(const std::string& serial) -> int {
    rs2::context context;
    if (!find_device_by_serial(context, serial).has_value()) {
        throw std::runtime_error("unknown realsense device: " + serial);
    }

    std::cout << "{\"valid\":true,\"id\":\"" << serial << "\"}\n";
    return 0;
}

auto print_capabilities_output(const std::string& serial) -> int {
    rs2::context context;
    const auto device = find_device_by_serial(context, serial);
    if (!device.has_value()) {
        throw std::runtime_error("unknown realsense device: " + serial);
    }

    std::cout << "{\"id\":\"" << serial << "\",\"profiles\":[";
    auto first = true;
    for (const auto& sensor : device->query_sensors()) {
        for (const auto& profile : sensor.get_stream_profiles()) {
            const auto video_profile = profile.as<rs2::video_stream_profile>();
            if (!video_profile) {
                continue;
            }

            const auto stream_type = profile.stream_type();
            if (stream_type != RS2_STREAM_COLOR && stream_type != RS2_STREAM_DEPTH && stream_type != RS2_STREAM_INFRARED) {
                continue;
            }

            const auto stream_name = stream_type == RS2_STREAM_COLOR
                ? "color"
                : stream_type == RS2_STREAM_DEPTH ? "depth" : "infrared";
            if (!first) {
                std::cout << ",";
            }
            first = false;
            std::cout << "{"
                      << "\"stream\":\"" << stream_name << "\","
                      << "\"index\":" << profile.stream_index() << ","
                      << "\"width\":" << video_profile.width() << ","
                      << "\"height\":" << video_profile.height() << ","
                      << "\"fps\":" << profile.fps()
                      << "}";
        }
    }
    std::cout << "]}\n";
    return 0;
}

auto run_realsense(const rollio::CameraDeviceConfig& config) -> int {
    using namespace iox2;

    rs2::context context;
    if (!find_device_by_serial(context, config.id).has_value()) {
        throw std::runtime_error("unknown realsense device: " + config.id);
    }

    const auto stream = parse_stream_config(config);
    const auto expected_pixel_format = stream.stream == RS2_STREAM_COLOR
        ? rollio::PixelFormat::Rgb24
        : stream.stream == RS2_STREAM_DEPTH ? rollio::PixelFormat::Depth16 : rollio::PixelFormat::Gray8;
    if (config.pixel_format != expected_pixel_format) {
        throw std::runtime_error(
            "realsense stream \"" + *config.stream + "\" requires pixel_format \"" +
            std::string(rollio::pixel_format_to_string(expected_pixel_format)) + "\""
        );
    }

    set_log_level_from_env_or(LogLevel::Info);
    auto node = NodeBuilder().create<ServiceType::Ipc>().value();

    const auto service_name_str = rollio::camera_frames_service_name(config.name);
    const auto service_name = ServiceName::create(service_name_str.c_str()).value();
    auto frame_service = node.service_builder(service_name)
                             .publish_subscribe<bb::Slice<uint8_t>>()
                             .user_header<rollio::CameraFrameHeader>()
                             .open_or_create()
                             .value();
    const auto initial_payload_size =
        static_cast<uint64_t>(config.width) * config.height * pixel_format_byte_size(config.pixel_format);
    auto publisher = frame_service.publisher_builder()
                         .initial_max_slice_len(initial_payload_size)
                         .allocation_strategy(AllocationStrategy::PowerOfTwo)
                         .create()
                         .value();

    const auto control_service_name = ServiceName::create(rollio::CONTROL_EVENTS_SERVICE).value();
    auto control_service = node.service_builder(control_service_name)
                               .publish_subscribe<rollio::ControlEvent>()
                               .open_or_create()
                               .value();
    auto control_subscriber = control_service.subscriber_builder().create().value();

    rs2::config rs_config;
    rs_config.enable_device(config.id);
    rs_config.enable_stream(stream.stream, stream.index, static_cast<int>(config.width), static_cast<int>(config.height), stream.format, static_cast<int>(config.fps));

    rs2::pipeline pipeline;
    pipeline.start(rs_config);

    auto frame_index = uint64_t {0};
    auto last_status = SteadyClock::now();
    auto last_timeout_log = SteadyClock::now() - std::chrono::seconds(5);
    std::cerr << "rollio-camera-realsense: device=" << config.id
              << " stream=" << *config.stream
              << " size=" << config.width << "x" << config.height
              << " fps=" << config.fps << '\n';

    while (true) {
        auto control_sample = control_subscriber.receive().value();
        while (control_sample.has_value()) {
            if (control_sample->payload().tag == rollio::ControlEventTag::Shutdown) {
                pipeline.stop();
                return 0;
            }
            control_sample = control_subscriber.receive().value();
        }

        rs2::frameset frames;
        try {
            frames = pipeline.wait_for_frames(1000);
        } catch (const rs2::error& error) {
            if (!is_transient_wait_timeout(error)) {
                throw;
            }
            if (SteadyClock::now() - last_timeout_log >= std::chrono::seconds(1)) {
                std::cerr << "rollio-camera-realsense: device=" << config.id
                          << " stream=" << *config.stream
                          << " waiting for next frame after timeout\n";
                last_timeout_log = SteadyClock::now();
            }
            continue;
        }
        rs2::frame frame = stream.stream == RS2_STREAM_COLOR
            ? frames.get_color_frame()
            : stream.stream == RS2_STREAM_DEPTH ? frames.get_depth_frame() : frames.get_infrared_frame(stream.index);
        if (!frame) {
            continue;
        }

        const auto* frame_data = static_cast<const uint8_t*>(frame.get_data());
        const auto payload_size = static_cast<uint64_t>(frame.get_data_size());
        const auto video_frame = frame.as<rs2::video_frame>();

        auto sample = publisher.loan_slice_uninit(payload_size).value();
        auto& header = sample.user_header_mut();
        header.timestamp_ns = static_cast<uint64_t>(frame.get_timestamp() * 1000000.0);
        header.width = static_cast<uint32_t>(video_frame.get_width());
        header.height = static_cast<uint32_t>(video_frame.get_height());
        header.pixel_format = config.pixel_format;
        header.frame_index = frame_index;

        const auto latest_timestamp_ns = header.timestamp_ns;
        auto initialized_sample = sample.write_from_fn(
            [&](const uint64_t byte_idx) -> uint8_t { return frame_data[static_cast<std::size_t>(byte_idx)]; }
        );
        send(std::move(initialized_sample)).value();

        frame_index += 1U;
        if (SteadyClock::now() - last_status >= std::chrono::seconds(1)) {
            std::cerr << "rollio-camera-realsense: device=" << config.id
                      << " stream=" << *config.stream
                      << " frame_index=" << frame_index
                      << " latest_timestamp_ns=" << latest_timestamp_ns
                      << " active=true\n";
            last_status = SteadyClock::now();
        }
    }
}

#else

auto print_probe_output() -> int {
    std::cout << "[]\n";
    return 0;
}

auto print_validate_output(const std::string& serial) -> int {
    throw std::runtime_error("realsense support is not compiled in for device: " + serial);
}

auto print_capabilities_output(const std::string& serial) -> int {
    throw std::runtime_error("realsense support is not compiled in for device: " + serial);
}

auto run_realsense(const rollio::CameraDeviceConfig& config) -> int {
    throw std::runtime_error("realsense support is not compiled in for device: " + config.id);
}

#endif

} // namespace

auto main(int argc, char* argv[]) -> int {
    try {
        if (argc < 2) {
            print_usage();
            return 1;
        }

        const std::string command = argv[1];
        if (command == "probe") {
            return print_probe_output();
        }
        if (command == "validate") {
            if (argc < 3) {
                throw std::runtime_error("validate requires a serial number");
            }
            return print_validate_output(argv[2]);
        }
        if (command == "capabilities") {
            if (argc < 3) {
                throw std::runtime_error("capabilities requires a serial number");
            }
            return print_capabilities_output(argv[2]);
        }
        if (command == "run") {
            return run_realsense(load_run_config(argc - 1, argv + 1));
        }

        throw std::runtime_error("unknown subcommand: " + command);
    } catch (const std::exception& error) {
        std::cerr << "rollio-camera-realsense: " << error.what() << '\n';
        return 1;
    }
}
