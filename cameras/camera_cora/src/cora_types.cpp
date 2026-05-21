#include "cora_types.hpp"

#include <cstring>
#include <iomanip>
#include <iostream>
#include <sstream>

namespace rollio::coracam {

// ---------------------------------------------------------------------------
// Minimal CDR1 little-endian parser
//
// CDR1 (XCDR1, "PLAIN_CDR") alignment rules:
//   - int8 / uint8 / bool: no alignment required
//   - int16 / uint16:      align to 2 bytes
//   - int32 / uint32 / float: align to 4 bytes
//   - int64 / uint64 / double: align to 8 bytes
//   - string:              uint32 length (4-byte aligned), then length bytes
//                          including null terminator, then no trailing pad
//   - sequence<T>:         uint32 length (4-byte aligned), then length * sizeof(T)
//                          (each element padded to its natural alignment)
//
// The full payload from Fast-DDS includes a 4-byte representation-identifier
// encapsulation header before the CDR data:
//   Byte 0: 0x00 (reserved)
//   Byte 1: 0x00 = CDR_BE, 0x01 = CDR_LE
//   Byte 2-3: options (usually 0x00 0x00)
//
// We read the encapsulation byte to determine endianness, then parse the
// CDR bytes.  We only implement little-endian here; big-endian cora payloads
// are rejected with a warning.
// ---------------------------------------------------------------------------

namespace {

struct CdrReader {
    const uint8_t* buf;
    size_t len;
    size_t pos{0};
    bool ok{true};

    void align(size_t boundary) {
        const size_t rem = pos % boundary;
        if (rem != 0) {
            pos += boundary - rem;
        }
        if (pos > len) {
            ok = false;
        }
    }

    template <typename T>
    T read() {
        align(sizeof(T));
        if (!ok || pos + sizeof(T) > len) {
            ok = false;
            return T{};
        }
        T val{};
        std::memcpy(&val, buf + pos, sizeof(T));
        pos += sizeof(T);
        return val;
    }

    std::string read_string() {
        align(4);
        if (!ok)
            return {};
        const uint32_t length = read<uint32_t>();
        if (!ok || length == 0)
            return {};
        if (pos + length > len) {
            ok = false;
            return {};
        }
        // length includes null terminator
        std::string s(reinterpret_cast<const char*>(buf + pos), length > 0 ? length - 1 : 0);
        pos += length;
        return s;
    }

    // Read a sequence<uint8_t>.
    std::vector<uint8_t> read_byte_sequence() {
        align(4);
        if (!ok)
            return {};
        const uint32_t length = read<uint32_t>();
        if (!ok)
            return {};
        if (pos + length > len) {
            ok = false;
            return {};
        }
        std::vector<uint8_t> v(buf + pos, buf + pos + length);
        pos += length;
        return v;
    }
};

CoraStamp read_stamp(CdrReader& r) {
    CoraStamp s;
    s.sec = r.read<int32_t>();
    s.nanosec = r.read<uint32_t>();
    return s;
}

CoraHeader read_header(CdrReader& r) {
    CoraHeader h;
    h.stamp = read_stamp(r);
    h.frame_id = r.read_string();
    return h;
}

}  // namespace

// ---------------------------------------------------------------------------
// Public parse functions
// ---------------------------------------------------------------------------

bool parse_cora_raw_image(const uint8_t* bytes, size_t len, CoraRawImage& out) {
    if (len < 4) {
        return false;
    }

    // Check encapsulation header: byte 1 is 0x01 for CDR_LE.
    if (bytes[1] != 0x01) {
        std::cerr << "[coracam] cora_raw_image: expected CDR_LE (0x01), got 0x" << std::hex
                  << static_cast<int>(bytes[1]) << std::dec << " — big-endian not supported\n";
        return false;
    }

    CdrReader r{bytes + 4, len - 4, 0};

    out.header = read_header(r);
    out.height = r.read<uint32_t>();
    out.width = r.read<uint32_t>();
    out.encoding = r.read_string();
    out.is_bigendian = r.read<uint8_t>();
    // Align to 4 for step
    r.align(4);
    out.step = r.read<uint32_t>();
    out.data = r.read_byte_sequence();

    return r.ok;
}

bool parse_cora_h264_packet(const uint8_t* bytes, size_t len, CoraH264Packet& out) {
    if (len < 4) {
        return false;
    }

    if (bytes[1] != 0x01) {
        std::cerr << "[coracam] cora_h264_packet: expected CDR_LE (0x01), got 0x" << std::hex
                  << static_cast<int>(bytes[1]) << std::dec << " — big-endian not supported\n";
        return false;
    }

    CdrReader r{bytes + 4, len - 4, 0};

    out.header = read_header(r);
    out.width = r.read<uint32_t>();
    out.height = r.read<uint32_t>();
    out.is_keyframe = r.read<uint8_t>() != 0;
    out.data = r.read_byte_sequence();

    return r.ok;
}

bool parse_foxglove_compressed_video(const uint8_t* bytes, size_t len,
                                     FoxgloveCompressedVideo& out) {
    if (len < 4) {
        return false;
    }

    if (bytes[1] != 0x01) {
        std::cerr << "[coracam] foxglove_compressed_video: expected CDR_LE (0x01), got 0x"
                  << std::hex << static_cast<int>(bytes[1]) << std::dec
                  << " — big-endian not supported\n";
        return false;
    }

    CdrReader r{bytes + 4, len - 4, 0};

    // foxglove_msgs/msg/CompressedVideo CDR layout used by camera_node:
    //   timestamp.sec     (int32)
    //   timestamp.nanosec (uint32)
    //   frame_id          (string)
    //   data              (sequence<uint8>)
    //   format            (string)
    //
    // Keep data before format. If these two are swapped, the parser reads the
    // trailing "h264\0" CDR string bytes as packet data and Annex-B validation
    // fails with first bytes 68 32 36 34 00.
    out.timestamp = read_stamp(r);
    out.frame_id = r.read_string();
    out.data = r.read_byte_sequence();
    out.format = r.read_string();

    return r.ok;
}

void dump_cdr_hex(const char* label, const uint8_t* bytes, size_t len) {
    const size_t dump_len = (len < 64) ? len : 64;
    std::ostringstream oss;
    oss << "[coracam] " << label << " first " << dump_len << "/" << len << " bytes:";
    for (size_t i = 0; i < dump_len; ++i) {
        if (i % 16 == 0) {
            oss << "\n  ";
        }
        oss << std::hex << std::setw(2) << std::setfill('0') << static_cast<int>(bytes[i]) << ' ';
    }
    std::cerr << oss.str() << '\n';
}

}  // namespace rollio::coracam
