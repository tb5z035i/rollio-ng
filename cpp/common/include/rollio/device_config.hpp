#ifndef ROLLIO_DEVICE_CONFIG_HPP
#define ROLLIO_DEVICE_CONFIG_HPP

#include <charconv>
#include <cstdint>
#include <fstream>
#include <optional>
#include <stdexcept>
#include <string>
#include <string_view>
#include <unordered_map>

#include "rollio/types.h"

namespace rollio {

struct CameraDeviceConfig {
    std::string name;
    std::string type;
    std::string driver;
    std::string id;
    uint32_t width{0};
    uint32_t height{0};
    uint32_t fps{0};
    PixelFormat pixel_format{PixelFormat::Rgb24};
    std::optional<std::string> stream;
    std::optional<uint32_t> channel;
    std::optional<std::string> transport;
};

namespace detail {

inline auto trim(std::string value) -> std::string {
    auto is_space = [](const unsigned char ch) -> bool {
        return ch == ' ' || ch == '\t' || ch == '\r' || ch == '\n';
    };

    while (!value.empty() && is_space(static_cast<unsigned char>(value.front()))) {
        value.erase(value.begin());
    }
    while (!value.empty() && is_space(static_cast<unsigned char>(value.back()))) {
        value.pop_back();
    }

    return value;
}

inline auto strip_quotes(std::string value) -> std::string {
    value = trim(std::move(value));
    if (value.size() >= 2 && value.front() == '"' && value.back() == '"') {
        return value.substr(1, value.size() - 2);
    }
    return value;
}

inline auto strip_comment(std::string value) -> std::string {
    auto in_quotes = false;
    for (std::size_t idx = 0; idx < value.size(); ++idx) {
        if (value[idx] == '"') {
            in_quotes = !in_quotes;
        } else if (value[idx] == '#' && !in_quotes) {
            value.erase(idx);
            break;
        }
    }
    return trim(std::move(value));
}

inline auto parse_u32(const std::unordered_map<std::string, std::string>& values,
                      const std::string& key) -> uint32_t {
    const auto it = values.find(key);
    if (it == values.end()) {
        throw std::runtime_error("missing required key: " + key);
    }

    uint32_t value = 0;
    const auto raw = trim(it->second);
    const auto* begin = raw.data();
    const auto* end = raw.data() + raw.size();
    const auto [ptr, error] = std::from_chars(begin, end, value);
    if (error != std::errc{} || ptr != end) {
        throw std::runtime_error("failed to parse integer key: " + key);
    }
    return value;
}

inline auto parse_optional_u32(const std::unordered_map<std::string, std::string>& values,
                               const std::string& key) -> std::optional<uint32_t> {
    const auto it = values.find(key);
    if (it == values.end()) {
        return std::nullopt;
    }
    return parse_u32(values, key);
}

inline auto parse_required_string(const std::unordered_map<std::string, std::string>& values,
                                  const std::string& key) -> std::string {
    const auto it = values.find(key);
    if (it == values.end()) {
        throw std::runtime_error("missing required key: " + key);
    }

    const auto value = strip_quotes(it->second);
    if (value.empty()) {
        throw std::runtime_error("key must not be empty: " + key);
    }
    return value;
}

inline auto parse_optional_string(const std::unordered_map<std::string, std::string>& values,
                                  const std::string& key) -> std::optional<std::string> {
    const auto it = values.find(key);
    if (it == values.end()) {
        return std::nullopt;
    }
    return strip_quotes(it->second);
}

inline auto parse_simple_toml(std::string_view text)
    -> std::unordered_map<std::string, std::string> {
    std::unordered_map<std::string, std::string> values;
    std::size_t cursor = 0;

    while (cursor <= text.size()) {
        const auto next_newline = text.find('\n', cursor);
        const auto line_end = next_newline == std::string_view::npos ? text.size() : next_newline;
        auto line = strip_comment(std::string(text.substr(cursor, line_end - cursor)));
        cursor = line_end == text.size() ? text.size() + 1 : line_end + 1;

        if (line.empty()) {
            continue;
        }
        if (line.front() == '[') {
            throw std::runtime_error("standalone device config must not contain TOML tables");
        }

        const auto separator = line.find('=');
        if (separator == std::string::npos) {
            throw std::runtime_error("invalid TOML line: " + line);
        }

        auto key = trim(line.substr(0, separator));
        auto value = trim(line.substr(separator + 1));
        if (key.empty()) {
            throw std::runtime_error("invalid TOML key");
        }
        values[key] = value;
    }

    return values;
}

}  // namespace detail

inline auto parse_camera_device_config(std::string_view text) -> CameraDeviceConfig {
    const auto values = detail::parse_simple_toml(text);

    CameraDeviceConfig config;
    config.name = detail::parse_required_string(values, "name");
    config.type = detail::parse_required_string(values, "type");
    config.driver = detail::parse_required_string(values, "driver");
    config.id = detail::parse_required_string(values, "id");
    config.width = detail::parse_u32(values, "width");
    config.height = detail::parse_u32(values, "height");
    config.fps = detail::parse_u32(values, "fps");
    config.stream = detail::parse_optional_string(values, "stream");
    config.channel = detail::parse_optional_u32(values, "channel");
    config.transport = detail::parse_optional_string(values, "transport");

    const auto pixel_format_name = detail::parse_required_string(values, "pixel_format");
    const auto pixel_format = pixel_format_from_string(pixel_format_name);
    if (!pixel_format.has_value()) {
        throw std::runtime_error("unsupported pixel_format: " + pixel_format_name);
    }
    config.pixel_format = *pixel_format;

    return config;
}

inline auto load_camera_device_config_from_file(const std::string& path) -> CameraDeviceConfig {
    std::ifstream file(path);
    if (!file.is_open()) {
        throw std::runtime_error("failed to open config file: " + path);
    }

    std::string text((std::istreambuf_iterator<char>(file)), std::istreambuf_iterator<char>());
    return parse_camera_device_config(text);
}

}  // namespace rollio

#endif  // ROLLIO_DEVICE_CONFIG_HPP
