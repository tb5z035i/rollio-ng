#!/usr/bin/env bash
set -euo pipefail

CXX=${CXX:-g++}
OUT=${OUT:-/tmp/bench_cdr_pod_bridge}

cat > /tmp/bench_cdr_pod_bridge.cpp <<'CPP'
#include <algorithm>
#include <chrono>
#include <cstdint>
#include <cstring>
#include <iomanip>
#include <iostream>
#include <stdexcept>
#include <string>
#include <vector>

#if defined(__GNUC__) || defined(__clang__)
#define NOINLINE __attribute__((noinline))
#else
#define NOINLINE
#endif

static volatile uint64_t g_sink = 0;

struct PodHeader {
    uint64_t timestamp_ns;
    uint32_t frame_id;
    uint32_t width;
    uint32_t height;
    uint32_t encoding;
    uint32_t data_len;
};

static_assert(sizeof(PodHeader) == 32, "Unexpected PodHeader layout");

static inline size_t align_up(size_t x, size_t a) {
    return (x + a - 1) & ~(a - 1);
}

template <class T>
static inline void write_primitive(std::vector<uint8_t>& buf, size_t& stream_pos, T v, size_t align) {
    stream_pos = align_up(stream_pos, align);
    size_t abs_pos = 4 + stream_pos;  // 4-byte CDR encapsulation header
    if (abs_pos + sizeof(T) > buf.size()) throw std::runtime_error("CDR buffer too small");
    std::memcpy(buf.data() + abs_pos, &v, sizeof(T));
    stream_pos += sizeof(T);
}

template <class T>
static inline T read_primitive(const std::vector<uint8_t>& buf, size_t& stream_pos, size_t align) {
    stream_pos = align_up(stream_pos, align);
    size_t abs_pos = 4 + stream_pos;
    if (abs_pos + sizeof(T) > buf.size()) throw std::runtime_error("CDR read overflow");
    T v;
    std::memcpy(&v, buf.data() + abs_pos, sizeof(T));
    stream_pos += sizeof(T);
    return v;
}

// Approximate XCDR1 / PLAIN_CDR for:
// struct ImageLike {
//   uint64 timestamp_ns;
//   uint32 frame_id;
//   uint32 width;
//   uint32 height;
//   uint32 encoding;
//   sequence<octet> data;
// };
NOINLINE size_t serialize_cdr_image(
    std::vector<uint8_t>& cdr,
    const uint8_t* payload,
    size_t payload_size,
    const PodHeader& h)
{
    if (payload_size > UINT32_MAX) throw std::runtime_error("payload too large");

    // Little-endian CDR encapsulation marker. Exact value is not important for this benchmark.
    cdr[0] = 0x00;
    cdr[1] = 0x01;
    cdr[2] = 0x00;
    cdr[3] = 0x00;

    size_t p = 0; // CDR stream position after encapsulation header

    write_primitive<uint64_t>(cdr, p, h.timestamp_ns, 8);
    write_primitive<uint32_t>(cdr, p, h.frame_id, 4);
    write_primitive<uint32_t>(cdr, p, h.width, 4);
    write_primitive<uint32_t>(cdr, p, h.height, 4);
    write_primitive<uint32_t>(cdr, p, h.encoding, 4);

    write_primitive<uint32_t>(cdr, p, static_cast<uint32_t>(payload_size), 4);
    size_t abs_payload = 4 + p;
    if (abs_payload + payload_size > cdr.size()) throw std::runtime_error("CDR buffer too small for payload");
    std::memcpy(cdr.data() + abs_payload, payload, payload_size);
    p += payload_size;

    return 4 + p;
}

struct CdrView {
    PodHeader h;
    const uint8_t* payload;
    size_t payload_size;
};

NOINLINE CdrView parse_cdr_image_view(const std::vector<uint8_t>& cdr) {
    size_t p = 0;

    CdrView v{};
    v.h.timestamp_ns = read_primitive<uint64_t>(cdr, p, 8);
    v.h.frame_id     = read_primitive<uint32_t>(cdr, p, 4);
    v.h.width        = read_primitive<uint32_t>(cdr, p, 4);
    v.h.height       = read_primitive<uint32_t>(cdr, p, 4);
    v.h.encoding     = read_primitive<uint32_t>(cdr, p, 4);
    v.h.data_len     = read_primitive<uint32_t>(cdr, p, 4);

    size_t abs_payload = 4 + p;
    if (abs_payload + v.h.data_len > cdr.size()) throw std::runtime_error("bad CDR payload length");

    v.payload = cdr.data() + abs_payload;
    v.payload_size = v.h.data_len;
    return v;
}

NOINLINE void pod_publish_copy(std::vector<uint8_t>& loan, const PodHeader& h, const uint8_t* payload) {
    std::memcpy(loan.data(), &h, sizeof(PodHeader));
    std::memcpy(loan.data() + sizeof(PodHeader), payload, h.data_len);
}

NOINLINE void raw_memcpy(std::vector<uint8_t>& dst, const std::vector<uint8_t>& src, size_t n) {
    std::memcpy(dst.data(), src.data(), n);
}

template <class F>
double bench_seconds(const std::string& name, size_t bytes_per_iter, int iters, F&& f) {
    // Warmup
    for (int i = 0; i < 10; ++i) f(i);

    auto t0 = std::chrono::steady_clock::now();
    for (int i = 0; i < iters; ++i) {
        f(i);
    }
    auto t1 = std::chrono::steady_clock::now();

    double sec = std::chrono::duration<double>(t1 - t0).count();
    double gb = static_cast<double>(bytes_per_iter) * iters / 1e9;
    double gbps = gb / sec;
    double us = sec * 1e6 / iters;

    std::cout << std::left << std::setw(36) << name
              << "  " << std::right << std::setw(9) << std::fixed << std::setprecision(2) << gbps << " GB/s"
              << "  " << std::setw(9) << std::fixed << std::setprecision(2) << us << " us/iter"
              << "  bytes/iter=" << bytes_per_iter
              << "\n";

    return sec;
}

int main(int argc, char** argv) {
    size_t payload_size = 1920ull * 1080ull * 3ull; // RGB8 1080p
    int iters = 1000;

    for (int i = 1; i < argc; ++i) {
        std::string a = argv[i];
        if (a == "--size" && i + 1 < argc) {
            payload_size = std::stoull(argv[++i]);
        } else if (a == "--iters" && i + 1 < argc) {
            iters = std::stoi(argv[++i]);
        } else if (a == "--help") {
            std::cout << "Usage: " << argv[0] << " [--size BYTES] [--iters N]\n";
            return 0;
        } else {
            throw std::runtime_error("unknown argument: " + a);
        }
    }

    PodHeader h{};
    h.timestamp_ns = 123456789;
    h.frame_id = 42;
    h.width = 1920;
    h.height = 1080;
    h.encoding = 1;
    h.data_len = static_cast<uint32_t>(payload_size);

    std::vector<uint8_t> payload(payload_size);
    std::vector<uint8_t> payload_copy(payload_size);
    std::vector<uint8_t> loan(sizeof(PodHeader) + payload_size);
    std::vector<uint8_t> temp(payload_size);
    std::vector<uint8_t> cdr(4 + 128 + payload_size);

    for (size_t i = 0; i < payload.size(); ++i) {
        payload[i] = static_cast<uint8_t>((i * 131u + 7u) & 0xffu);
    }

    size_t cdr_size = serialize_cdr_image(cdr, payload.data(), payload_size, h);
    cdr.resize(cdr_size);

    std::cout << "Payload size: " << payload_size << " bytes"
              << "  CDR size: " << cdr_size << " bytes"
              << "  iterations: " << iters << "\n\n";

    bench_seconds("raw memcpy payload only",
                  payload_size,
                  iters,
                  [&](int i) {
                      raw_memcpy(payload_copy, payload, payload_size);
                      g_sink += payload_copy[static_cast<size_t>(i) % payload_size];
                  });

    bench_seconds("POD header + payload copy",
                  sizeof(PodHeader) + payload_size,
                  iters,
                  [&](int i) {
                      PodHeader hh = h;
                      hh.frame_id += static_cast<uint32_t>(i);
                      pod_publish_copy(loan, hh, payload.data());
                      g_sink += loan[sizeof(PodHeader) + (static_cast<size_t>(i) % payload_size)];
                  });

    bench_seconds("CDR parse view + copy to POD",
                  cdr_size + sizeof(PodHeader) + payload_size,
                  iters,
                  [&](int i) {
                      CdrView v = parse_cdr_image_view(cdr);
                      pod_publish_copy(loan, v.h, v.payload);
                      g_sink += loan[sizeof(PodHeader) + (static_cast<size_t>(i) % payload_size)];
                  });

    bench_seconds("CDR deserialize temp + copy to POD",
                  cdr_size + payload_size + sizeof(PodHeader) + payload_size,
                  iters,
                  [&](int i) {
                      CdrView v = parse_cdr_image_view(cdr);
                      std::memcpy(temp.data(), v.payload, v.payload_size); // simulate DDS sequence materialization
                      pod_publish_copy(loan, v.h, temp.data());
                      g_sink += loan[sizeof(PodHeader) + (static_cast<size_t>(i) % payload_size)];
                  });

    std::vector<uint8_t> cdr_out(4 + 128 + payload_size);
    bench_seconds("CDR serialize from POD/payload",
                  sizeof(PodHeader) + payload_size + payload_size,
                  iters,
                  [&](int i) {
                      PodHeader hh = h;
                      hh.frame_id += static_cast<uint32_t>(i);
                      size_t n = serialize_cdr_image(cdr_out, payload.data(), payload_size, hh);
                      g_sink += cdr_out[n - 1];
                  });

    std::cout << "\nsink=" << g_sink << "\n";
    return 0;
}
CPP

${CXX} -O3 -march=native -std=c++17 -Wall -Wextra /tmp/bench_cdr_pod_bridge.cpp -o "${OUT}"

echo "Built: ${OUT}"
echo
"${OUT}" "$@"
