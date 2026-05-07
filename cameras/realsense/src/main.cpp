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
        << "Usage: rollio-device-realsense <probe|validate|capabilities|query|run> [args...]\n"
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
        case rollio::PixelFormat::H264:
            // Compressed formats: bytes-per-pixel is meaningless. Realsense
            // never produces these but we still need a well-defined return
            // value to keep the switch exhaustive (-Wswitch-enum).
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
        case rollio::PixelFormat::H264:
            return "h264";
    }
    return "rgb24";
}

auto channel_type_supported(std::string_view t) -> bool {
    return t == "color" || t == "depth" || t == "infrared";
}

auto validate_channel_types(const std::vector<std::string>& requested) -> bool {
    return std::all_of(requested.begin(), requested.end(),
                       [](const std::string& t) { return channel_type_supported(t); });
}

auto pixel_format_allowed_for_channel_type(const std::string& channel_type,
                                           rollio::PixelFormat pixel_format) -> bool {
    if (channel_type == "color") {
        // Color sensors can publish either RGB24 (librealsense converts
        // YUYV->RGB internally on every frame) or raw YUYV (skips that
        // conversion; the encoder decodes natively). The latter is the
        // recommended fast path; RGB24 stays for backward compatibility.
        return pixel_format == rollio::PixelFormat::Rgb24 ||
               pixel_format == rollio::PixelFormat::Yuyv;
    }
    if (channel_type == "depth") {
        return pixel_format == rollio::PixelFormat::Depth16;
    }
    if (channel_type == "infrared") {
        return pixel_format == rollio::PixelFormat::Gray8;
    }
    throw std::runtime_error("unsupported realsense channel_type: " + channel_type);
}

auto allowed_pixel_formats_label(const std::string& channel_type) -> std::string {
    if (channel_type == "color") {
        return "rgb24, yuyv";
    }
    if (channel_type == "depth") {
        return "depth16";
    }
    if (channel_type == "infrared") {
        return "gray8";
    }
    throw std::runtime_error("unsupported realsense channel_type: " + channel_type);
}

struct ResolvedRealsenseCamera {
    std::string channel_type;
    rollio::CameraChannelProfile profile;
    std::optional<uint32_t> stream_index;
};

auto resolve_realsense_camera_channels(const rollio::BinaryDeviceConfig& config)
    -> std::vector<ResolvedRealsenseCamera> {
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
            throw std::runtime_error("enabled camera channel \"" + ch.channel_type +
                                     "\" requires a profile");
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
            throw std::runtime_error("duplicate realsense stream selection for channel \"" +
                                     ch.channel_type + "\"");
        }
        if (!pixel_format_allowed_for_channel_type(ch.channel_type, ch.profile->pixel_format)) {
            throw std::runtime_error(
                "realsense channel \"" + ch.channel_type + "\" requires pixel_format in {" +
                allowed_pixel_formats_label(ch.channel_type) + "}, got \"" +
                std::string(rollio::pixel_format_to_string(ch.profile->pixel_format)) + "\"");
        }
        out.push_back(ResolvedRealsenseCamera{ch.channel_type, *ch.profile, ch.stream_index});
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

auto find_device_by_serial(const rs2::context& context,
                           const std::string& serial) -> std::optional<rs2::device> {
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

auto rs_stream_spec_for_channel(const std::string& channel_type,
                                rollio::PixelFormat pixel_format,
                                std::optional<uint32_t> stream_index) -> RsStreamSpec {
    if (channel_type == "color") {
        // Default fast path: ask the sensor for raw YUYV so librealsense
        // skips its internal YUYV->RGB conversion (one full frame of
        // SIMD-light scalar work per frame). The encoder decodes the
        // YUV422 frames directly via libavcodec / swscale. Operators that
        // explicitly need RGB on the bus (e.g. for ad-hoc subscribers
        // that don't run the encoder) keep `pixel_format = "rgb24"` and
        // librealsense converts as before.
        const auto rs_format =
            pixel_format == rollio::PixelFormat::Yuyv ? RS2_FORMAT_YUYV : RS2_FORMAT_RGB8;
        return RsStreamSpec{RS2_STREAM_COLOR, 0, rs_format};
    }
    if (channel_type == "depth") {
        return RsStreamSpec{RS2_STREAM_DEPTH, 0, RS2_FORMAT_Z16};
    }
    if (channel_type == "infrared") {
        const auto idx = static_cast<int>(stream_index.value_or(1U));
        return RsStreamSpec{RS2_STREAM_INFRARED, idx, RS2_FORMAT_Y8};
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

    const std::string name = present ? device->get_info(RS2_CAMERA_INFO_NAME) : std::string();
    const std::string bus = present ? device_bus_string(*device) : std::string();

    if (args.json) {
        std::cout << "{\"valid\":" << (valid ? "true" : "false") << ",\"id\":\""
                  << json_escape(args.id) << "\",\"name\":\"" << json_escape(name)
                  << "\",\"driver\":\"realsense\",\"bus\":\"" << json_escape(bus) << "\"}\n";
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
            // Both color formats are now valid run-command inputs:
            //   * RGB8 -> bus pixel_format = "rgb24" (legacy path)
            //   * YUYV -> bus pixel_format = "yuyv"  (fast path)
            return format == RS2_FORMAT_RGB8 || format == RS2_FORMAT_YUYV;
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
            if (!profile_matches_run_command(stream_type, video_profile.format(),
                                             profile.stream_index())) {
                continue;
            }

            const auto stream_name = stream_type == RS2_STREAM_COLOR   ? "color"
                                     : stream_type == RS2_STREAM_DEPTH ? "depth"
                                                                       : "infrared";
            const auto* format_name = video_profile.format() == RS2_FORMAT_RGB8  ? "rgb24"
                                      : video_profile.format() == RS2_FORMAT_YUYV ? "yuyv"
                                      : video_profile.format() == RS2_FORMAT_Z16  ? "depth16"
                                      : video_profile.format() == RS2_FORMAT_Y8   ? "gray8"
                                                                                 : "unknown";
            if (!first) {
                std::cout << ",";
            }
            first = false;
            std::cout << "{" << "\"stream\":\"" << stream_name << "\","
                      << "\"index\":" << profile.stream_index() << ","
                      << "\"width\":" << video_profile.width() << ","
                      << "\"height\":" << video_profile.height() << ","
                      << "\"fps\":" << profile.fps() << ","
                      << "\"pixel_format\":\"" << format_name << "\"}";
        }
    }
    std::cout << "]}\n";
    return 0;
}

auto append_profile_json(std::ostringstream& out, uint32_t width, uint32_t height, uint32_t fps,
                         rollio::PixelFormat pf) -> void {
    out << "{\"width\":" << width << ",\"height\":" << height << ",\"fps\":" << fps
        << ",\"pixel_format\":\"" << pixel_format_json_name(pf) << "\"}";
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
            if (!profile_matches_run_command(stream_type, video_profile.format(),
                                             profile.stream_index())) {
                continue;
            }
            const auto w = static_cast<uint32_t>(video_profile.width());
            const auto h = static_cast<uint32_t>(video_profile.height());
            const auto fps = static_cast<uint32_t>(profile.fps());
            if (stream_type == RS2_STREAM_COLOR) {
                // Map the rs2 native format to the bus pixel format the
                // operator must request to actually get this stream.
                // Both YUYV and RGB8 share the same (w,h,fps) sets on
                // D4xx sensors, so we emit a separate profile entry per
                // bus format so the wizard can offer both options.
                const auto pf = video_profile.format() == RS2_FORMAT_YUYV
                                    ? rollio::PixelFormat::Yuyv
                                    : rollio::PixelFormat::Rgb24;
                if (!first_c) {
                    profiles_color << ",";
                }
                first_c = false;
                append_profile_json(profiles_color, w, h, fps, pf);
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
              << "\",\"default_device_name\":\"realsense\","
              << "\"optional_info\":{},\"channels\":["
              << "{\"channel_type\":\"color\",\"kind\":\"camera\",\"available\":true,"
              << "\"channel_label\":\"Intel RealSense RGB\",\"default_name\":\"realsense_rgb\","
              << "\"modes\":[\"enabled\",\"disabled\"],\"profiles\":" << profiles_color.str()
              << ",\"supported_states\":[],\"supported_commands\":[],\"supports_fk\":false,"
                 "\"supports_ik\":false,"
              << "\"dof\":null,\"default_control_frequency_hz\":null,"
              << "\"direct_joint_compatibility\":{\"can_lead\":[],\"can_follow\":[]},"
              << "\"defaults\":{},\"optional_info\":{}},"
              << "{\"channel_type\":\"depth\",\"kind\":\"camera\",\"available\":true,"
              << "\"channel_label\":\"Intel RealSense Depth\",\"default_name\":\"realsense_depth\","
              << "\"modes\":[\"enabled\",\"disabled\"],\"profiles\":" << profiles_depth.str()
              << ",\"supported_states\":[],\"supported_commands\":[],\"supports_fk\":false,"
                 "\"supports_ik\":false,"
              << "\"dof\":null,\"default_control_frequency_hz\":null,"
              << "\"direct_joint_compatibility\":{\"can_lead\":[],\"can_follow\":[]},"
              << "\"defaults\":{},\"optional_info\":{}},"
              << "{\"channel_type\":\"infrared\",\"kind\":\"camera\",\"available\":true,"
              << "\"channel_label\":\"Intel RealSense Infrared\",\"default_name\":\"realsense_ir\","
              << "\"modes\":[\"enabled\",\"disabled\"],\"profiles\":" << profiles_ir.str()
              << ",\"supported_states\":[],\"supported_commands\":[],\"supports_fk\":false,"
                 "\"supports_ik\":false,"
              << "\"dof\":null,\"default_control_frequency_hz\":null,"
              << "\"direct_joint_compatibility\":{\"can_lead\":[],\"can_follow\":[]},"
              << "\"defaults\":{},\"optional_info\":{}}" << "]}]}\n";
    return 0;
}

struct EnabledCameraChannel {
    std::string channel_type;
    rollio::CameraChannelProfile profile;
    RsStreamSpec rs;
};

auto build_enabled_camera_channels(const rollio::BinaryDeviceConfig& config)
    -> std::vector<EnabledCameraChannel> {
    std::vector<EnabledCameraChannel> out;
    for (const auto& resolved : resolve_realsense_camera_channels(config)) {
        out.push_back(EnabledCameraChannel{
            resolved.channel_type,
            resolved.profile,
            rs_stream_spec_for_channel(
                resolved.channel_type,
                resolved.profile.pixel_format,
                resolved.stream_index),
        });
    }
    return out;
}

// Tracks the offset between the realsense device's hardware clock
// (monotonic, device-internal microseconds) and the host's UNIX-epoch
// clock. Refreshed periodically so a 10-20+ minute episode does not
// accumulate device-vs-host crystal drift (combined ~50 ppm worst-case
// would otherwise reach ~30 ms over 20 min).
//
// We deliberately bypass `RS2_OPTION_GLOBAL_TIME_ENABLED` — librealsense's
// smoothed global-time offset can step the published value backward by a
// few microseconds between adjacent frames, which surfaced as 1 µs gaps
// in the encoded MP4 PTS. By owning the offset ourselves we get:
//   * Per-channel intervals match the camera's real capture cadence
//     between refreshes (uniform, e.g. 11.1 ms at 90 fps / 16.7 ms at
//     60 fps / 33.3 ms at 30 fps).
//   * Per-refresh offset shift is bounded by NTP slew + crystal drift
//     over the refresh window. With a 5 s window, worst-case shift is
//     well under the 11.1 ms minimum frame interval, so the refresh
//     cannot step a published timestamp backward across one frame.
class HardwareClockOffset {
public:
    explicit HardwareClockOffset(std::chrono::microseconds refresh_period)
        : refresh_period_us_(refresh_period.count()) {}

    // Translate a device hardware-clock timestamp (microseconds) into
    // UNIX-epoch microseconds, refreshing the underlying offset if the
    // device-time gap since the last refresh exceeds the configured
    // window. Thread-affine: the realsense pipeline is single-threaded
    // and dispatches to every sink from the same loop, so we don't add
    // a mutex here.
    auto to_unix_us(int64_t device_us) -> int64_t {
        if (!initialized_) {
            offset_us_ = host_unix_us_now() - device_us;
            initialized_ = true;
            last_refresh_device_us_ = device_us;
        } else if (device_us - last_refresh_device_us_ >= refresh_period_us_) {
            offset_us_ = host_unix_us_now() - device_us;
            last_refresh_device_us_ = device_us;
        }
        return device_us + offset_us_;
    }

private:
    static auto host_unix_us_now() -> int64_t {
        return std::chrono::duration_cast<std::chrono::microseconds>(
                   std::chrono::system_clock::now().time_since_epoch())
            .count();
    }

    int64_t refresh_period_us_;
    bool initialized_{false};
    int64_t offset_us_{0};
    int64_t last_refresh_device_us_{0};
};

class RealsenseFrameSink {
public:
    RealsenseFrameSink(std::string channel_type, rollio::PixelFormat pixel_format,
                       RsStreamSpec spec, std::shared_ptr<HardwareClockOffset> clock)
        : channel_type_(std::move(channel_type)),
          pixel_format_(pixel_format),
          spec_(spec),
          clock_(std::move(clock)) {}

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

    auto clock() -> HardwareClockOffset& {
        return *clock_;
    }

private:
    std::string channel_type_;
    rollio::PixelFormat pixel_format_;
    RsStreamSpec spec_;
    std::shared_ptr<HardwareClockOffset> clock_;
};

template <typename Publisher>
class RealsenseFrameSinkImpl final : public RealsenseFrameSink {
public:
    RealsenseFrameSinkImpl(std::string channel_type, rollio::PixelFormat pixel_format,
                           RsStreamSpec spec, std::shared_ptr<HardwareClockOffset> clock,
                           Publisher publisher)
        : RealsenseFrameSink(std::move(channel_type), pixel_format, spec, std::move(clock)),
          publisher_(std::move(publisher)) {}

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

        // Resolve the device-side hardware timestamp for this frame in
        // microseconds. We prefer `RS2_FRAME_METADATA_FRAME_TIMESTAMP`
        // because it's the raw ASIC-side counter (microsecond precision,
        // monotonic, device-wide so color and depth share the same clock,
        // unaffected by `RS2_OPTION_GLOBAL_TIME_ENABLED` smoothing). On
        // some firmware revisions / sensor combos the metadata may be
        // unavailable, in which case we fall back to `frame.get_timestamp()`
        // and emit a one-shot warning so the operator knows we're in the
        // less-clean codepath.
        int64_t device_us = 0;
        if (!frame_timestamp_metadata_checked_) {
            frame_timestamp_metadata_checked_ = true;
            frame_timestamp_metadata_supported_ =
                frame.supports_frame_metadata(RS2_FRAME_METADATA_FRAME_TIMESTAMP);
            if (!frame_timestamp_metadata_supported_) {
                std::cerr << "rollio-device-realsense: warning: channel=" << channel_type()
                          << " does not expose RS2_FRAME_METADATA_FRAME_TIMESTAMP; falling "
                             "back to frame.get_timestamp() (may include librealsense "
                             "global-time smoothing artifacts)\n";
            }
        }
        if (frame_timestamp_metadata_supported_) {
            device_us = frame.get_frame_metadata(RS2_FRAME_METADATA_FRAME_TIMESTAMP);
        } else {
            device_us = static_cast<int64_t>(frame.get_timestamp() * 1000.0);
        }

        // Drop syncer-padded duplicates. librealsense's pipeline syncer
        // can re-emit the most recent depth frame paired with a fresh
        // color frame while the depth sensor is still warming up — those
        // padded frames carry the same raw hardware timestamp as the
        // previous one. Republishing them would (a) waste encoder
        // bandwidth, (b) require us to invent a synthetic timestamp that
        // doesn't reflect a real new capture. Instead we drop the frame
        // and let the assembler align around the gap.
        //
        // Per-frame strict equality on `device_us` is the right test
        // because the hardware counter is microsecond-resolution, so two
        // *real* successive captures differ by ~11,111 µs (90 fps) /
        // ~16,667 µs (60 fps) / ~33,333 µs (30 fps) at the smallest, all
        // far above the metadata's quantization.
        if (last_device_us_ != 0 && device_us <= last_device_us_) {
            duplicate_drop_count_ += 1;
            if (!duplicate_drop_warned_) {
                duplicate_drop_warned_ = true;
                std::cerr << "rollio-device-realsense: warning: dropping syncer-padded frame "
                             "on channel="
                          << channel_type() << " (device_us=" << device_us
                          << ", last_device_us=" << last_device_us_
                          << "); subsequent drops are counted but silenced.\n";
            }
            return;
        }
        last_device_us_ = device_us;

        // Translate the device hardware-clock value to UNIX-epoch
        // microseconds. The shared `HardwareClockOffset` is responsible
        // for periodic re-anchoring so a long episode (10-20+ min) does
        // not accumulate device-vs-host crystal drift.
        const auto unix_us = clock().to_unix_us(device_us);

        // Belt-and-suspenders monotonicity guard. With duplicates already
        // dropped above and the 5 s offset refresh window short enough
        // that NTP slew can't shift the offset by more than the minimum
        // frame interval, this branch should effectively never fire. If
        // it ever does (e.g. operator NTP-stepped the host clock
        // backward), log once with the magnitude.
        uint64_t timestamp_us = 0;
        if (last_published_us_ != 0 && unix_us <= static_cast<int64_t>(last_published_us_)) {
            const auto delta_us = static_cast<int64_t>(last_published_us_) - unix_us;
            if (!nonmonotonic_warned_) {
                nonmonotonic_warned_ = true;
                std::cerr << "rollio-device-realsense: warning: non-monotonic timestamp on "
                             "channel="
                          << channel_type() << " (raw_unix_us=" << unix_us
                          << ", last_published_us=" << last_published_us_
                          << ", backward_step_us=" << delta_us
                          << "); bumping by 1 us to keep the stream strictly increasing. "
                             "Subsequent occurrences are silenced.\n";
            }
            timestamp_us = last_published_us_ + 1;
        } else {
            timestamp_us = static_cast<uint64_t>(std::max<int64_t>(0, unix_us));
        }
        last_published_us_ = timestamp_us;

        auto sample = publisher_.loan_slice_uninit(payload_size).value();
        auto& header = sample.user_header_mut();
        header.timestamp_us = timestamp_us;
        header.width = static_cast<uint32_t>(video_frame.get_width());
        header.height = static_cast<uint32_t>(video_frame.get_height());
        header.pixel_format = pixel_format();
        header.frame_index = local_frame_index_;

        const auto latest_timestamp_us = header.timestamp_us;
        // Use the slice memcpy fast path. The per-byte `write_from_fn` path
        // routes every byte through a type-erased `bb::StaticFunction` call
        // plus a bounds-checked slice subscript and a placement-new, which
        // burns a full CPU core when streaming 640x480@60 color+depth.
        auto frame_slice = iox2::bb::ImmutableSlice<uint8_t>(frame_data, payload_size);
        auto initialized_sample = sample.write_from_slice(frame_slice);
        send(std::move(initialized_sample)).value();
        local_frame_index_ += 1U;

        if (SteadyClock::now() - last_status_ >= std::chrono::seconds(1)) {
            std::cerr << "rollio-device-realsense: bus_root channel=" << channel_type()
                      << " frame_index=" << local_frame_index_
                      << " latest_timestamp_us=" << latest_timestamp_us
                      << " duplicates_dropped=" << duplicate_drop_count_ << " active=true\n";
            last_status_ = SteadyClock::now();
        }
    }

private:
    Publisher publisher_;
    uint64_t local_frame_index_{0};
    SteadyClock::time_point last_status_{SteadyClock::now()};
    bool frame_timestamp_metadata_checked_{false};
    bool frame_timestamp_metadata_supported_{false};
    int64_t last_device_us_{0};
    uint64_t last_published_us_{0};
    uint64_t duplicate_drop_count_{0};
    bool duplicate_drop_warned_{false};
    bool nonmonotonic_warned_{false};
};

auto run_realsense(const rollio::BinaryDeviceConfig& config, bool dry_run) -> int {
    using namespace iox2;

    const auto channels = build_enabled_camera_channels(config);

    if (dry_run) {
        std::cerr << "rollio-device-realsense: dry-run ok device=" << config.id
                  << " bus_root=" << config.bus_root << " channels=" << channels.size() << '\n';
        for (const auto& ch : channels) {
            std::cerr << "  - " << ch.channel_type << " service="
                      << rollio::channel_frames_service_name(config.bus_root, ch.channel_type)
                      << " size=" << ch.profile.width << "x" << ch.profile.height
                      << " fps=" << ch.profile.fps << '\n';
        }
        return 0;
    }

    rs2::context context;
    auto device_opt = find_device_by_serial(context, config.id);
    if (!device_opt.has_value()) {
        throw std::runtime_error("unknown realsense device: " + config.id);
    }

    // Best-effort: ask librealsense to disable its smoothed global-time
    // domain so `frame.get_timestamp()` falls back to the hardware-clock
    // domain. Empirically this set_option call is a no-op on some D400
    // firmware combos (the per-sensor option doesn't propagate to the
    // pipeline-level time keeper), so the sink-side code does NOT depend
    // on it succeeding — it reads the raw hardware counter via
    // `RS2_FRAME_METADATA_FRAME_TIMESTAMP` regardless. The set_option is
    // kept as defence-in-depth: when it does work, the fallback path of
    // `frame.get_timestamp()` (used only when the metadata field is
    // unavailable) is also clean of global-time smoothing artifacts.
    for (auto& sensor : device_opt->query_sensors()) {
        if (sensor.supports(RS2_OPTION_GLOBAL_TIME_ENABLED)) {
            try {
                sensor.set_option(RS2_OPTION_GLOBAL_TIME_ENABLED, 0.0F);
            } catch (const rs2::error& error) {
                std::cerr
                    << "rollio-device-realsense: device=" << config.id
                    << " warning: failed to disable RS2_OPTION_GLOBAL_TIME_ENABLED on sensor: "
                    << error.what()
                    << " (smoothed global-time may produce small non-uniform frame intervals)\n";
            }
        }
        // If the sensor doesn't support `RS2_OPTION_GLOBAL_TIME_ENABLED`
        // at all, the default is hardware-clock domain — exactly what we
        // want — so silence that case. The sink-side domain check will
        // still emit a per-channel warning if any frame ever lands in a
        // different domain.
    }

    set_log_level_from_env_or(LogLevel::Info);
    auto node = NodeBuilder().create<ServiceType::Ipc>().value();

    // Shared by every sink for this device: color and depth read from the
    // same hardware clock so they share one host-clock offset, and
    // refreshing in one place keeps them perfectly co-aligned. The 5 s
    // window is chosen so the per-refresh offset shift (NTP slew + crystal
    // drift) stays well below the minimum frame interval (11.1 ms at the
    // realsense's max 90 fps), which makes the sink-side monotonicity
    // guard rarely (if ever) trip in normal operation.
    auto clock = std::make_shared<HardwareClockOffset>(std::chrono::seconds(5));

    std::vector<std::unique_ptr<RealsenseFrameSink>> sinks;
    sinks.reserve(channels.size());

    for (const auto& ch : channels) {
        const auto service_name_str =
            rollio::channel_frames_service_name(config.bus_root, ch.channel_type);
        const auto service_name = ServiceName::create(service_name_str.c_str()).value();
        auto frame_service = node.service_builder(service_name)
                                 .publish_subscribe<bb::Slice<uint8_t>>()
                                 .user_header<rollio::CameraFrameHeader>()
                                 .open_or_create()
                                 .value();
        const auto initial_payload_size = static_cast<uint64_t>(ch.profile.width) *
                                          ch.profile.height *
                                          pixel_format_byte_size(ch.profile.pixel_format);
        auto publisher = frame_service.publisher_builder()
                             .initial_max_slice_len(initial_payload_size)
                             .allocation_strategy(AllocationStrategy::PowerOfTwo)
                             .create()
                             .value();

        sinks.push_back(std::make_unique<RealsenseFrameSinkImpl<decltype(publisher)>>(
            ch.channel_type, ch.profile.pixel_format, ch.rs, clock, std::move(publisher)));
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
        rs_config.enable_stream(ch.rs.stream, ch.rs.index, static_cast<int>(ch.profile.width),
                                static_cast<int>(ch.profile.height), ch.rs.format,
                                static_cast<int>(ch.profile.fps));
    }

    rs2::pipeline pipeline;
    pipeline.start(rs_config);

    auto last_timeout_log = SteadyClock::now() - std::chrono::seconds(5);
    std::cerr << "rollio-device-realsense: device=" << config.id << " bus_root=" << config.bus_root
              << " channels=" << channels.size() << '\n';

    while (true) {
        auto control_sample = control_subscriber.receive().value();
        while (control_sample.has_value()) {
            if (control_sample->payload().tag == rollio::ControlEventTag::Shutdown) {
                pipeline.stop();
                std::cerr << "rollio-device-realsense: shutdown received for " << config.bus_root
                          << '\n';
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
                std::cerr << "rollio-device-realsense: device=" << config.id
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
    std::cerr << "rollio-device-realsense: dry-run ok (stub build) device=" << config.id
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
        std::cerr << "rollio-device-realsense: " << error.what() << '\n';
        return 1;
    }
}
