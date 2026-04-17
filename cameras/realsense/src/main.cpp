#include "iox2/iceoryx2.hpp"
#include "rollio/device_config.hpp"
#include "rollio/topic_names.hpp"
#include "rollio/types.h"

#if ROLLIO_HAVE_REALSENSE
#include <librealsense2/rs.hpp>
#endif

#include <algorithm>
#include <chrono>
#include <cstdint>
#include <iostream>
#include <memory>
#include <optional>
#include <sstream>
#include <stdexcept>
#include <string>
#include <string_view>
#include <thread>
#include <unordered_set>
#include <vector>

namespace {

using SteadyClock = std::chrono::steady_clock;

auto print_usage() -> void {
    std::cerr
        << "Usage: rollio-camera-realsense <probe|validate|capabilities|query|run> [args...]\n"
        << "  probe\n"
        << "  validate [--json] [--channel-type <type>]... <serial>\n"
        << "  capabilities <serial>\n"
        << "  query [--json] <serial>\n"
        << "  run (--config <path> | --config-inline <toml>) [--dry-run]\n";
}

auto optional_arg(int argc, char* argv[], const std::string& name) -> std::optional<std::string> {
    for (auto index = 0; index + 1 < argc; ++index) {
        if (name == argv[index]) {
            return std::string(argv[index + 1]);
        }
    }
    return std::nullopt;
}

auto has_flag(int argc, char* argv[], const std::string& name) -> bool {
    for (auto index = 0; index < argc; ++index) {
        if (name == argv[index]) {
            return true;
        }
    }
    return false;
}

struct ValidateCli {
    std::string id;
    std::vector<std::string> channel_types;
    bool json{false};
};

struct QueryCli {
    std::string id;
    bool json{false};
};

auto parse_validate_cli(int argc, char* argv[]) -> ValidateCli {
    ValidateCli out;
    for (int i = 0; i < argc; ++i) {
        const std::string_view arg(argv[i]);
        if (arg == "--json") {
            out.json = true;
        } else if (arg == "--channel-type") {
            if (i + 1 >= argc) {
                throw std::runtime_error("--channel-type requires a value");
            }
            out.channel_types.emplace_back(argv[i + 1]);
            ++i;
        } else if (!arg.empty() && arg.front() == '-') {
            throw std::runtime_error(std::string("unknown flag: ") + std::string(arg));
        } else {
            if (!out.id.empty()) {
                throw std::runtime_error("validate expects a single device id (serial)");
            }
            out.id = std::string(arg);
        }
    }
    if (out.id.empty()) {
        throw std::runtime_error("validate requires a serial number");
    }
    return out;
}

auto parse_query_cli(int argc, char* argv[]) -> QueryCli {
    QueryCli out;
    for (int i = 0; i < argc; ++i) {
        const std::string_view arg(argv[i]);
        if (arg == "--json") {
            out.json = true;
        } else if (!arg.empty() && arg.front() == '-') {
            throw std::runtime_error(std::string("unknown flag: ") + std::string(arg));
        } else {
            if (!out.id.empty()) {
                throw std::runtime_error("query expects a single device id (serial)");
            }
            out.id = std::string(arg);
        }
    }
    if (out.id.empty()) {
        throw std::runtime_error("query requires a serial number");
    }
    return out;
}

auto load_run_binary_config(int argc, char* argv[]) -> rollio::BinaryDeviceConfig {
    const auto config_path = optional_arg(argc, argv, "--config");
    const auto config_inline = optional_arg(argc, argv, "--config-inline");
    if (config_path.has_value() == config_inline.has_value()) {
        throw std::runtime_error("run requires exactly one of --config or --config-inline");
    }

    return config_inline.has_value() ? rollio::parse_binary_device_config(*config_inline)
                                     : rollio::load_binary_device_config_from_file(*config_path);
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

auto json_escape(std::string_view value) -> std::string {
    std::string out;
    out.reserve(value.size());
    for (const char ch : value) {
        switch (ch) {
            case '\\':
                out += "\\\\";
                break;
            case '"':
                out += "\\\"";
                break;
            case '\n':
                out += "\\n";
                break;
            case '\r':
                out += "\\r";
                break;
            case '\t':
                out += "\\t";
                break;
            default:
                out.push_back(ch);
        }
    }
    return out;
}

auto pixel_format_json_name(const rollio::PixelFormat fmt) -> const char* {
    switch (fmt) {
        case rollio::PixelFormat::Rgb24:
            return "rgb24";
        case rollio::PixelFormat::Bgr24:
            return "bgr24";
        case rollio::PixelFormat::Yuyv:
            return "yuyv";
        case rollio::PixelFormat::Mjpeg:
            return "mjpeg";
        case rollio::PixelFormat::Depth16:
            return "depth16";
        case rollio::PixelFormat::Gray8:
            return "gray8";
    }
    return "rgb24";
}

auto channel_type_supported(std::string_view t) -> bool {
    return t == "color" || t == "depth" || t == "infrared";
}

auto validate_channel_types(const std::vector<std::string>& requested) -> bool {
    return std::all_of(requested.begin(), requested.end(), [](const std::string& t) {
        return channel_type_supported(t);
    });
}

auto expected_pixel_format_for_channel_type(const std::string& channel_type) -> rollio::PixelFormat {
    if (channel_type == "color") {
        return rollio::PixelFormat::Rgb24;
    }
    if (channel_type == "depth") {
        return rollio::PixelFormat::Depth16;
    }
    if (channel_type == "infrared") {
        return rollio::PixelFormat::Gray8;
    }
    throw std::runtime_error("unsupported realsense channel_type: " + channel_type);
}

struct ResolvedRealsenseCamera {
    std::string channel_type;
    rollio::CameraChannelProfile profile;
    std::optional<uint32_t> stream_index;
};

auto resolve_realsense_camera_channels(const rollio::BinaryDeviceConfig& config) -> std::vector<ResolvedRealsenseCamera> {
    if (config.driver != "realsense") {
        throw std::runtime_error("realsense driver requires driver = \"realsense\"");
    }

    std::vector<ResolvedRealsenseCamera> out;
    std::unordered_set<std::string> seen_types;
    std::unordered_set<std::string> seen_streams;

    for (const auto& ch : config.channels) {
        if (!ch.enabled) {
            continue;
        }
        if (ch.kind != rollio::DeviceKind::Camera) {
            throw std::runtime_error("realsense binary device may only enable camera channels");
        }
        if (!ch.profile.has_value()) {
            throw std::runtime_error("enabled camera channel \"" + ch.channel_type + "\" requires a profile");
        }
        if (!channel_type_supported(ch.channel_type)) {
            throw std::runtime_error("unsupported realsense channel_type: " + ch.channel_type);
        }
        if (!seen_types.insert(ch.channel_type).second) {
            throw std::runtime_error("duplicate channel_type: " + ch.channel_type);
        }
        const std::string stream_key =
            ch.channel_type == "infrared"
                ? ch.channel_type + "-" + std::to_string(ch.stream_index.value_or(1U))
                : ch.channel_type;
        if (!seen_streams.insert(stream_key).second) {
            throw std::runtime_error("duplicate realsense stream selection for channel \"" + ch.channel_type + "\"");
        }
        const auto expected = expected_pixel_format_for_channel_type(ch.channel_type);
        if (ch.profile->pixel_format != expected) {
            throw std::runtime_error(
                "realsense channel \"" + ch.channel_type + "\" requires pixel_format \"" +
                std::string(rollio::pixel_format_to_string(expected)) + "\""
            );
        }
        out.push_back(ResolvedRealsenseCamera {ch.channel_type, *ch.profile, ch.stream_index});
    }

    if (out.empty()) {
        throw std::runtime_error("realsense driver requires at least one enabled camera channel");
    }
    return out;
}

#if ROLLIO_HAVE_REALSENSE

struct RsStreamSpec {
    rs2_stream stream;
    int index;
    rs2_format format;
};

auto serialize_device(const rs2::device& device) -> std::string {
    const auto serial = device.get_info(RS2_CAMERA_INFO_SERIAL_NUMBER);
    const auto name = device.get_info(RS2_CAMERA_INFO_NAME);

    std::ostringstream output;
    output << "{\"id\":\"" << json_escape(serial) << "\",\"name\":\"" << json_escape(name)
           << "\",\"driver\":\"realsense\",\"type\":\"camera\"}";
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

auto device_bus_string(const rs2::device& device) -> std::string {
    if (device.supports(RS2_CAMERA_INFO_PHYSICAL_PORT)) {
        return device.get_info(RS2_CAMERA_INFO_PHYSICAL_PORT);
    }
    return "unknown";
}

auto rs_stream_spec_for_channel(
    const std::string& channel_type,
    std::optional<uint32_t> stream_index
) -> RsStreamSpec {
    if (channel_type == "color") {
        return RsStreamSpec {RS2_STREAM_COLOR, 0, RS2_FORMAT_RGB8};
    }
    if (channel_type == "depth") {
        return RsStreamSpec {RS2_STREAM_DEPTH, 0, RS2_FORMAT_Z16};
    }
    if (channel_type == "infrared") {
        const auto idx = static_cast<int>(stream_index.value_or(1U));
        return RsStreamSpec {RS2_STREAM_INFRARED, idx, RS2_FORMAT_Y8};
    }
    throw std::runtime_error("unsupported realsense channel_type: " + channel_type);
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

auto print_validate_output(const ValidateCli& args) -> int {
    rs2::context context;
    const auto device = find_device_by_serial(context, args.id);
    const bool present = device.has_value();
    const bool types_ok = args.channel_types.empty() || validate_channel_types(args.channel_types);
    const bool valid = present && types_ok;

    const std::string name =
        present ? device->get_info(RS2_CAMERA_INFO_NAME) : std::string();
    const std::string bus = present ? device_bus_string(*device) : std::string();

    if (args.json) {
        std::cout << "{\"valid\":" << (valid ? "true" : "false") << ",\"id\":\"" << json_escape(args.id)
                  << "\",\"name\":\"" << json_escape(name) << "\",\"driver\":\"realsense\",\"bus\":\""
                  << json_escape(bus) << "\"}\n";
    } else if (valid) {
        std::cout << args.id << " is valid\n";
    } else {
        std::cout << args.id << " is invalid\n";
    }
    return 0;
}

// Only emit stream profiles that the realsense run command can actually
// fulfill. librealsense2 advertises many redundant variants per resolution
// (RGB8 + BGR8 + RGBA8 + YUYV for color, two infrared sensors, etc.), and
// `pipeline.start` is strict about format/index — for example D4xx series
// supports 1920x1080 *only* in YUYV, not RGB8. Without this filter the
// controller picks `1920x1080 @ 30 rgb24` as the default color profile and
// the run command then aborts with "Couldn't resolve requests".
auto profile_matches_run_command(rs2_stream stream, rs2_format format, int stream_index) -> bool {
    switch (stream) {
        case RS2_STREAM_COLOR:
            return format == RS2_FORMAT_RGB8;
        case RS2_STREAM_DEPTH:
            return format == RS2_FORMAT_Z16;
        case RS2_STREAM_INFRARED:
            // The run command defaults to infrared sensor 1 (the left IR on
            // the D4xx stereo module). Filter out the right IR (index 2) so
            // depth + infrared share the same sensor and resolution.
            return format == RS2_FORMAT_Y8 && stream_index == 1;
        default:
            return false;
    }
}

auto print_capabilities_output(const std::string& serial) -> int {
    rs2::context context;
    const auto device = find_device_by_serial(context, serial);
    if (!device.has_value()) {
        throw std::runtime_error("unknown realsense device: " + serial);
    }

    std::cout << "{\"id\":\"" << json_escape(serial) << "\",\"profiles\":[";
    auto first = true;
    for (const auto& sensor : device->query_sensors()) {
        for (const auto& profile : sensor.get_stream_profiles()) {
            const auto video_profile = profile.as<rs2::video_stream_profile>();
            if (!video_profile) {
                continue;
            }

            const auto stream_type = profile.stream_type();
            if (!profile_matches_run_command(
                    stream_type, video_profile.format(), profile.stream_index())) {
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

auto append_profile_json(std::ostringstream& out, uint32_t width, uint32_t height, uint32_t fps, rollio::PixelFormat pf)
    -> void {
    out << "{\"width\":" << width << ",\"height\":" << height << ",\"fps\":" << fps << ",\"pixel_format\":\""
        << pixel_format_json_name(pf) << "\"}";
}

auto print_query_human(const std::string& label, const std::string& serial,
                         const std::vector<std::string>& channel_types) -> void {
    std::cout << label << " (" << serial << ")\n";
    for (const auto& ch : channel_types) {
        std::cout << "  - " << ch << " [camera]\n";
    }
}

auto print_query_output(const QueryCli& args) -> int {
    rs2::context context;
    const auto device = find_device_by_serial(context, args.id);
    if (!device.has_value()) {
        throw std::runtime_error("unknown realsense device: " + args.id);
    }

    const auto label = device->get_info(RS2_CAMERA_INFO_NAME);
    const std::vector<std::string> channel_types = {"color", "depth", "infrared"};

    if (!args.json) {
        print_query_human(label, args.id, channel_types);
        return 0;
    }

    std::ostringstream profiles_color;
    profiles_color << "[";
    std::ostringstream profiles_depth;
    profiles_depth << "[";
    std::ostringstream profiles_ir;
    profiles_ir << "[";

    auto first_c = true;
    auto first_d = true;
    auto first_i = true;

    for (const auto& sensor : device->query_sensors()) {
        for (const auto& profile : sensor.get_stream_profiles()) {
            const auto video_profile = profile.as<rs2::video_stream_profile>();
            if (!video_profile) {
                continue;
            }
            const auto stream_type = profile.stream_type();
            if (!profile_matches_run_command(
                    stream_type, video_profile.format(), profile.stream_index())) {
                continue;
            }
            const auto w = static_cast<uint32_t>(video_profile.width());
            const auto h = static_cast<uint32_t>(video_profile.height());
            const auto fps = static_cast<uint32_t>(profile.fps());
            if (stream_type == RS2_STREAM_COLOR) {
                if (!first_c) {
                    profiles_color << ",";
                }
                first_c = false;
                append_profile_json(profiles_color, w, h, fps, rollio::PixelFormat::Rgb24);
            } else if (stream_type == RS2_STREAM_DEPTH) {
                if (!first_d) {
                    profiles_depth << ",";
                }
                first_d = false;
                append_profile_json(profiles_depth, w, h, fps, rollio::PixelFormat::Depth16);
            } else {
                if (!first_i) {
                    profiles_ir << ",";
                }
                first_i = false;
                append_profile_json(profiles_ir, w, h, fps, rollio::PixelFormat::Gray8);
            }
        }
    }
    profiles_color << "]";
    profiles_depth << "]";
    profiles_ir << "]";

    std::cout << "{\"driver\":\"realsense\",\"devices\":[{\"id\":\"" << json_escape(args.id)
              << "\",\"device_class\":\"realsense\",\"device_label\":\"" << json_escape(label)
              << "\",\"optional_info\":{},\"channels\":["
              << "{\"channel_type\":\"color\",\"kind\":\"camera\",\"available\":true,"
              << "\"modes\":[\"enabled\",\"disabled\"],\"profiles\":" << profiles_color.str()
              << ",\"supported_states\":[],\"supported_commands\":[],\"supports_fk\":false,\"supports_ik\":false,"
              << "\"dof\":null,\"default_control_frequency_hz\":null,"
              << "\"direct_joint_compatibility\":{\"can_lead\":[],\"can_follow\":[]},"
              << "\"defaults\":{},\"optional_info\":{}},"
              << "{\"channel_type\":\"depth\",\"kind\":\"camera\",\"available\":true,"
              << "\"modes\":[\"enabled\",\"disabled\"],\"profiles\":" << profiles_depth.str()
              << ",\"supported_states\":[],\"supported_commands\":[],\"supports_fk\":false,\"supports_ik\":false,"
              << "\"dof\":null,\"default_control_frequency_hz\":null,"
              << "\"direct_joint_compatibility\":{\"can_lead\":[],\"can_follow\":[]},"
              << "\"defaults\":{},\"optional_info\":{}},"
              << "{\"channel_type\":\"infrared\",\"kind\":\"camera\",\"available\":true,"
              << "\"modes\":[\"enabled\",\"disabled\"],\"profiles\":" << profiles_ir.str()
              << ",\"supported_states\":[],\"supported_commands\":[],\"supports_fk\":false,\"supports_ik\":false,"
              << "\"dof\":null,\"default_control_frequency_hz\":null,"
              << "\"direct_joint_compatibility\":{\"can_lead\":[],\"can_follow\":[]},"
              << "\"defaults\":{},\"optional_info\":{}}"
              << "]}]}\n";
    return 0;
}

struct EnabledCameraChannel {
    std::string channel_type;
    rollio::CameraChannelProfile profile;
    RsStreamSpec rs;
};

auto build_enabled_camera_channels(const rollio::BinaryDeviceConfig& config) -> std::vector<EnabledCameraChannel> {
    std::vector<EnabledCameraChannel> out;
    for (const auto& resolved : resolve_realsense_camera_channels(config)) {
        out.push_back(EnabledCameraChannel {
            resolved.channel_type,
            resolved.profile,
            rs_stream_spec_for_channel(resolved.channel_type, resolved.stream_index),
        });
    }
    return out;
}

class RealsenseFrameSink {
  public:
    RealsenseFrameSink(std::string channel_type, rollio::PixelFormat pixel_format, RsStreamSpec spec)
        : channel_type_(std::move(channel_type))
        , pixel_format_(pixel_format)
        , spec_(spec) {}

    virtual ~RealsenseFrameSink() = default;

    [[nodiscard]] auto channel_type() const -> const std::string& {
        return channel_type_;
    }

    virtual void try_publish(const rs2::frameset& frames) = 0;

  protected:
    auto pixel_format() const -> rollio::PixelFormat {
        return pixel_format_;
    }

    auto spec() const -> const RsStreamSpec& {
        return spec_;
    }

  private:
    std::string channel_type_;
    rollio::PixelFormat pixel_format_;
    RsStreamSpec spec_;
};

template<typename Publisher>
class RealsenseFrameSinkImpl final : public RealsenseFrameSink {
  public:
    RealsenseFrameSinkImpl(
        std::string channel_type,
        rollio::PixelFormat pixel_format,
        RsStreamSpec spec,
        Publisher publisher
    )
        : RealsenseFrameSink(std::move(channel_type), pixel_format, spec)
        , publisher_(std::move(publisher)) {}

    void try_publish(const rs2::frameset& frames) override {
        rs2::frame frame;
        if (spec().stream == RS2_STREAM_COLOR) {
            frame = frames.get_color_frame();
        } else if (spec().stream == RS2_STREAM_DEPTH) {
            frame = frames.get_depth_frame();
        } else {
            frame = frames.get_infrared_frame(spec().index);
        }
        if (!frame) {
            return;
        }

        const auto* frame_data = static_cast<const uint8_t*>(frame.get_data());
        const auto payload_size = static_cast<uint64_t>(frame.get_data_size());
        const auto video_frame = frame.as<rs2::video_frame>();

        auto sample = publisher_.loan_slice_uninit(payload_size).value();
        auto& header = sample.user_header_mut();
        header.timestamp_ns = static_cast<uint64_t>(frame.get_timestamp() * 1000000.0);
        header.width = static_cast<uint32_t>(video_frame.get_width());
        header.height = static_cast<uint32_t>(video_frame.get_height());
        header.pixel_format = pixel_format();
        header.frame_index = local_frame_index_;

        const auto latest_timestamp_ns = header.timestamp_ns;
        auto initialized_sample = sample.write_from_fn(
            [&](const uint64_t byte_idx) -> uint8_t { return frame_data[static_cast<std::size_t>(byte_idx)]; }
        );
        send(std::move(initialized_sample)).value();
        local_frame_index_ += 1U;

        if (SteadyClock::now() - last_status_ >= std::chrono::seconds(1)) {
            std::cerr << "rollio-camera-realsense: bus_root channel=" << channel_type()
                      << " frame_index=" << local_frame_index_ << " latest_timestamp_ns=" << latest_timestamp_ns
                      << " active=true\n";
            last_status_ = SteadyClock::now();
        }
    }

  private:
    Publisher publisher_;
    uint64_t local_frame_index_{0};
    SteadyClock::time_point last_status_{SteadyClock::now()};
};

auto run_realsense(const rollio::BinaryDeviceConfig& config, bool dry_run) -> int {
    using namespace iox2;

    const auto channels = build_enabled_camera_channels(config);

    if (dry_run) {
        std::cerr << "rollio-camera-realsense: dry-run ok device=" << config.id << " bus_root=" << config.bus_root
                  << " channels=" << channels.size() << '\n';
        for (const auto& ch : channels) {
            std::cerr << "  - " << ch.channel_type << " service=" << rollio::channel_frames_service_name(
                                                                                          config.bus_root, ch.channel_type
                                                                                      )
                      << " size=" << ch.profile.width << "x" << ch.profile.height << " fps=" << ch.profile.fps
                      << '\n';
        }
        return 0;
    }

    rs2::context context;
    if (!find_device_by_serial(context, config.id).has_value()) {
        throw std::runtime_error("unknown realsense device: " + config.id);
    }

    set_log_level_from_env_or(LogLevel::Info);
    auto node = NodeBuilder().create<ServiceType::Ipc>().value();

    std::vector<std::unique_ptr<RealsenseFrameSink>> sinks;
    sinks.reserve(channels.size());

    for (const auto& ch : channels) {
        const auto service_name_str = rollio::channel_frames_service_name(config.bus_root, ch.channel_type);
        const auto service_name = ServiceName::create(service_name_str.c_str()).value();
        auto frame_service = node.service_builder(service_name)
                                 .publish_subscribe<bb::Slice<uint8_t>>()
                                 .user_header<rollio::CameraFrameHeader>()
                                 .open_or_create()
                                 .value();
        const auto initial_payload_size = static_cast<uint64_t>(ch.profile.width) * ch.profile.height *
            pixel_format_byte_size(ch.profile.pixel_format);
        auto publisher = frame_service.publisher_builder()
                             .initial_max_slice_len(initial_payload_size)
                             .allocation_strategy(AllocationStrategy::PowerOfTwo)
                             .create()
                             .value();

        sinks.push_back(std::make_unique<RealsenseFrameSinkImpl<decltype(publisher)>>(
            ch.channel_type,
            ch.profile.pixel_format,
            ch.rs,
            std::move(publisher)
        ));
    }

    const auto control_service_name = ServiceName::create(rollio::CONTROL_EVENTS_SERVICE).value();
    auto control_service = node.service_builder(control_service_name)
                               .publish_subscribe<rollio::ControlEvent>()
                               .open_or_create()
                               .value();
    auto control_subscriber = control_service.subscriber_builder().create().value();

    rs2::config rs_config;
    rs_config.enable_device(config.id);
    for (const auto& ch : channels) {
        rs_config.enable_stream(
            ch.rs.stream,
            ch.rs.index,
            static_cast<int>(ch.profile.width),
            static_cast<int>(ch.profile.height),
            ch.rs.format,
            static_cast<int>(ch.profile.fps)
        );
    }

    rs2::pipeline pipeline;
    pipeline.start(rs_config);

    auto last_timeout_log = SteadyClock::now() - std::chrono::seconds(5);
    std::cerr << "rollio-camera-realsense: device=" << config.id << " bus_root=" << config.bus_root
              << " channels=" << channels.size() << '\n';

    while (true) {
        auto control_sample = control_subscriber.receive().value();
        while (control_sample.has_value()) {
            if (control_sample->payload().tag == rollio::ControlEventTag::Shutdown) {
                pipeline.stop();
                std::cerr << "rollio-camera-realsense: shutdown received for " << config.bus_root << '\n';
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
                          << " waiting for next frame after timeout\n";
                last_timeout_log = SteadyClock::now();
            }
            continue;
        }

        for (auto& sink : sinks) {
            sink->try_publish(frames);
        }
    }
}

#else

auto print_probe_output() -> int {
    std::cout << "[]\n";
    return 0;
}

auto print_validate_output(const ValidateCli& args) -> int {
    (void)args;
    throw std::runtime_error("realsense support is not compiled in");
}

auto print_capabilities_output(const std::string& serial) -> int {
    (void)serial;
    throw std::runtime_error("realsense support is not compiled in");
}

auto print_query_output(const QueryCli& args) -> int {
    (void)args;
    throw std::runtime_error("realsense support is not compiled in");
}

auto run_realsense(const rollio::BinaryDeviceConfig& config, bool dry_run) -> int {
    const auto resolved = resolve_realsense_camera_channels(config);
    if (!dry_run) {
        throw std::runtime_error("realsense support is not compiled in for device: " + config.id);
    }
    std::cerr << "rollio-camera-realsense: dry-run ok (stub build) device=" << config.id
              << " bus_root=" << config.bus_root << " channels=" << resolved.size() << '\n';
    for (const auto& ch : resolved) {
        std::cerr << "  - " << ch.channel_type << " service="
                  << rollio::channel_frames_service_name(config.bus_root, ch.channel_type) << '\n';
    }
    return 0;
}

#endif

}  // namespace

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
                throw std::runtime_error("validate requires arguments");
            }
            return print_validate_output(parse_validate_cli(argc - 2, argv + 2));
        }
        if (command == "capabilities") {
            if (argc < 3) {
                throw std::runtime_error("capabilities requires a serial number");
            }
            return print_capabilities_output(argv[2]);
        }
        if (command == "query") {
            if (argc < 3) {
                throw std::runtime_error("query requires arguments");
            }
            return print_query_output(parse_query_cli(argc - 2, argv + 2));
        }
        if (command == "run") {
            const auto dry_run = has_flag(argc - 1, argv + 1, "--dry-run");
            return run_realsense(load_run_binary_config(argc - 1, argv + 1), dry_run);
        }

        throw std::runtime_error("unknown subcommand: " + command);
    } catch (const std::exception& error) {
        std::cerr << "rollio-camera-realsense: " << error.what() << '\n';
        return 1;
    }
}
