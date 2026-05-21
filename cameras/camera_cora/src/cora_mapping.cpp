#include "cora_mapping.hpp"

#include <cctype>
#include <charconv>
#include <fstream>
#include <stdexcept>
#include <string>
#include <string_view>

namespace rollio::coracam {

namespace {

auto trim(std::string s) -> std::string {
    auto is_ws = [](unsigned char c) { return c == ' ' || c == '\t' || c == '\r' || c == '\n'; };
    while (!s.empty() && is_ws(static_cast<unsigned char>(s.front()))) {
        s.erase(s.begin());
    }
    while (!s.empty() && is_ws(static_cast<unsigned char>(s.back()))) {
        s.pop_back();
    }
    return s;
}

auto strip_comment(std::string s) -> std::string {
    bool in_quotes = false;
    for (std::size_t i = 0; i < s.size(); ++i) {
        if (s[i] == '"') {
            in_quotes = !in_quotes;
        } else if (s[i] == '#' && !in_quotes) {
            s.erase(i);
            break;
        }
    }
    return trim(std::move(s));
}

auto strip_quotes(std::string s) -> std::string {
    s = trim(std::move(s));
    if (s.size() >= 2 && s.front() == '"' && s.back() == '"') {
        return s.substr(1, s.size() - 2);
    }
    return s;
}

auto parse_u32(const std::string& raw, const std::string& key) -> uint32_t {
    const auto s = trim(raw);
    uint32_t value = 0;
    const auto* b = s.data();
    const auto* e = s.data() + s.size();
    const auto [ptr, ec] = std::from_chars(b, e, value);
    if (ec != std::errc{} || ptr != e) {
        throw std::runtime_error("cora_mapping: invalid integer for key '" + key + "': " + s);
    }
    return value;
}

}  // namespace

auto parse_annex_b_validation(std::string_view value) -> AnnexBValidationMode {
    if (value == "scan")
        return AnnexBValidationMode::Scan;
    if (value == "metadata")
        return AnnexBValidationMode::Metadata;
    if (value == "auto")
        return AnnexBValidationMode::Auto;
    throw std::runtime_error("cora_mapping: invalid annex_b_validation '" + std::string(value) +
                             "' (expected scan|metadata|auto)");
}

auto annex_b_validation_to_string(AnnexBValidationMode mode) -> const char* {
    switch (mode) {
        case AnnexBValidationMode::Scan:
            return "scan";
        case AnnexBValidationMode::Metadata:
            return "metadata";
        case AnnexBValidationMode::Auto:
            return "auto";
    }
    return "scan";
}

auto parse_cora_mapping(std::string_view text) -> CoraMapping {
    CoraMapping out;

    enum class Section { Root, Topic };
    Section section = Section::Root;

    std::size_t cursor = 0;
    while (cursor <= text.size()) {
        const auto eol = text.find('\n', cursor);
        const auto end = eol == std::string_view::npos ? text.size() : eol;
        auto line = strip_comment(std::string(text.substr(cursor, end - cursor)));
        cursor = end == text.size() ? text.size() + 1 : end + 1;

        if (line.empty())
            continue;

        if (line.front() == '[') {
            if (line == "[[topics]]") {
                out.topics.emplace_back();
                section = Section::Topic;
                continue;
            }
            throw std::runtime_error("cora_mapping: unsupported table header: " + line);
        }

        const auto eq = line.find('=');
        if (eq == std::string::npos) {
            throw std::runtime_error("cora_mapping: invalid line (missing '='): " + line);
        }
        auto key = trim(line.substr(0, eq));
        auto raw = trim(line.substr(eq + 1));

        if (section == Section::Root) {
            if (key == "domain_id") {
                out.domain_id = parse_u32(raw, key);
            } else if (key == "participant_name") {
                out.participant_name = strip_quotes(raw);
            } else if (key == "max_packet_bytes") {
                out.max_packet_bytes = parse_u32(raw, key);
            } else if (key == "annex_b_validation") {
                out.annex_b_validation = parse_annex_b_validation(strip_quotes(raw));
            } else if (key == "metadata_validation_packets") {
                out.metadata_validation_packets = parse_u32(raw, key);
            } else {
                // Forward-compatible: ignore unknown keys.
            }
        } else {
            // section == Section::Topic
            if (out.topics.empty()) {
                throw std::runtime_error("cora_mapping: key '" + key +
                                         "' before [[topics]] header");
            }
            auto& t = out.topics.back();
            if (key == "channel_type") {
                t.channel_type = strip_quotes(raw);
            } else if (key == "topic") {
                t.topic = strip_quotes(raw);
            } else if (key == "type") {
                t.type = strip_quotes(raw);
            } else if (key == "max_packet_bytes") {
                t.max_packet_bytes = parse_u32(raw, key);
            } else if (key == "raw_expected_encoding") {
                t.raw_expected_encoding = strip_quotes(raw);
            } else if (key == "annex_b_validation") {
                t.annex_b_validation = parse_annex_b_validation(strip_quotes(raw));
            } else if (key == "metadata_validation_packets") {
                t.metadata_validation_packets = parse_u32(raw, key);
            } else {
                // Ignore unknown topic keys.
            }
        }
    }

    // Validate uniqueness of channel_type across [[topics]] entries.
    for (std::size_t i = 0; i < out.topics.size(); ++i) {
        if (out.topics[i].channel_type.empty()) {
            throw std::runtime_error("cora_mapping: [[topics]] entry " + std::to_string(i) +
                                     " missing channel_type");
        }
        for (std::size_t j = i + 1; j < out.topics.size(); ++j) {
            if (out.topics[i].channel_type == out.topics[j].channel_type) {
                throw std::runtime_error("cora_mapping: duplicate channel_type '" +
                                         out.topics[i].channel_type + "'");
            }
        }
    }
    return out;
}

auto load_cora_mapping_from_file(const std::string& path) -> CoraMapping {
    std::ifstream file(path);
    if (!file.is_open()) {
        throw std::runtime_error("cora_mapping: failed to open " + path);
    }
    std::string text((std::istreambuf_iterator<char>(file)), std::istreambuf_iterator<char>());
    return parse_cora_mapping(text);
}

}  // namespace rollio::coracam
