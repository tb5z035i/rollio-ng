#include "h264_annexb.hpp"

#include <cstdint>
#include <cstring>
#include <vector>

namespace rollio::coracam {

namespace {

// Append a 4-byte Annex-B start code.
void append_start(std::vector<uint8_t>& out) {
    out.push_back(0x00);
    out.push_back(0x00);
    out.push_back(0x00);
    out.push_back(0x01);
}

void append_slice_nal(std::vector<uint8_t>& out, uint8_t header, uint8_t slice_header,
                      uint64_t seed, uint8_t extra_len = 4) {
    append_start(out);
    out.push_back(header);
    out.push_back(slice_header);
    for (uint8_t i = 0; i < extra_len; ++i) {
        const uint8_t b = static_cast<uint8_t>((seed >> (i * 8U)) & 0xFFU);
        out.push_back(b != 0 ? b : 0x01U);
    }
}

auto find_next_start_code(const uint8_t* data, std::size_t size, std::size_t offset) noexcept
    -> std::size_t {
    if (offset >= size) {
        return size;
    }
    for (std::size_t i = offset; i + 3U <= size; ++i) {
        if (data[i] == 0x00 && data[i + 1U] == 0x00 && data[i + 2U] == 0x01) {
            return i;
        }
        if (i + 4U <= size && data[i] == 0x00 && data[i + 1U] == 0x00 && data[i + 2U] == 0x00 &&
            data[i + 3U] == 0x01) {
            return i;
        }
    }
    return size;
}

auto rbsp_from_ebsp(const uint8_t* data, std::size_t size) -> std::vector<uint8_t> {
    std::vector<uint8_t> rbsp;
    rbsp.reserve(size);
    uint32_t zero_run = 0;
    for (std::size_t i = 0; i < size; ++i) {
        const uint8_t byte = data[i];
        if (zero_run >= 2U && byte == 0x03) {
            zero_run = 0;
            continue;
        }
        rbsp.push_back(byte);
        if (byte == 0x00) {
            zero_run += 1U;
        } else {
            zero_run = 0;
        }
    }
    return rbsp;
}

class BitReader {
public:
    explicit BitReader(const std::vector<uint8_t>& bytes) : bytes_(bytes) {}

    auto read_bit(uint32_t& bit) noexcept -> bool {
        if (bit_pos_ >= bytes_.size() * 8U) {
            return false;
        }
        const auto byte_index = bit_pos_ / 8U;
        const auto bit_index = 7U - (bit_pos_ % 8U);
        bit = (bytes_[byte_index] >> bit_index) & 0x01U;
        ++bit_pos_;
        return true;
    }

    auto read_ue(uint32_t& value) noexcept -> bool {
        uint32_t leading_zero_bits = 0;
        uint32_t bit = 0;
        while (true) {
            if (!read_bit(bit)) {
                return false;
            }
            if (bit == 1U) {
                break;
            }
            ++leading_zero_bits;
            if (leading_zero_bits > 31U) {
                return false;
            }
        }

        uint32_t suffix = 0;
        for (uint32_t i = 0; i < leading_zero_bits; ++i) {
            if (!read_bit(bit)) {
                return false;
            }
            suffix = (suffix << 1U) | bit;
        }
        value = ((1U << leading_zero_bits) - 1U) + suffix;
        return true;
    }

private:
    const std::vector<uint8_t>& bytes_;
    std::size_t bit_pos_{0};
};

auto parse_slice_type(const uint8_t* ebsp, std::size_t size, uint32_t& slice_type) -> bool {
    if (size == 0U) {
        return false;
    }
    const auto rbsp = rbsp_from_ebsp(ebsp, size);
    BitReader reader(rbsp);
    uint32_t first_mb_in_slice = 0;
    if (!reader.read_ue(first_mb_in_slice)) {
        return false;
    }
    return reader.read_ue(slice_type);
}

auto normalized_slice_type(uint32_t slice_type) noexcept -> int {
    if (slice_type > 9U) {
        return -1;
    }
    return static_cast<int>(slice_type % 5U);
}

}  // namespace

auto make_mock_annexb_au(bool keyframe, uint64_t frame_index) -> std::vector<uint8_t> {
    std::vector<uint8_t> au;
    au.reserve(keyframe ? 64U : 16U);

    if (keyframe) {
        // Keyframe: SPS + PPS + IDR
        // SPS payload encodes a minimal high-level header (baseline profile 4.0).
        append_start(au);
        au.push_back(kNalHeaderSps);
        // SPS RBSP: profile_idc=66, constraint flags=0xC0, level_idc=40,
        // seq_parameter_set_id=0 (UVLC 1), plus a few placeholder bytes.
        const uint8_t sps_rbsp[] = {0x42, 0xC0, 0x28, 0x01, 0x0F};
        au.insert(au.end(), sps_rbsp, sps_rbsp + sizeof(sps_rbsp));

        // PPS RBSP: pic_parameter_set_id=0, seq_id=0, entropy=CAVLC.
        append_start(au);
        au.push_back(kNalHeaderPps);
        const uint8_t pps_rbsp[] = {0xEE, 0x01, 0x60};
        au.insert(au.end(), pps_rbsp, pps_rbsp + sizeof(pps_rbsp));

        // first_mb_in_slice=0, slice_type=2 (I): bit pattern "1 011".
        append_slice_nal(au, kNalHeaderIdr, 0xB0, frame_index, 8U);
    } else {
        // first_mb_in_slice=0, slice_type=0 (P): bit pattern "1 1".
        append_slice_nal(au, kNalHeaderSlice, 0xC0, frame_index, 6U);
    }

    return au;
}

auto has_annexb_start_code(const uint8_t* data, std::size_t size) noexcept -> bool {
    if (size < 3U) {
        return false;
    }
    // 3-byte start code
    if (data[0] == 0x00 && data[1] == 0x00 && data[2] == 0x01) {
        return true;
    }
    // 4-byte start code
    if (size >= 4U && data[0] == 0x00 && data[1] == 0x00 && data[2] == 0x00 && data[3] == 0x01) {
        return true;
    }
    return false;
}

auto find_nal_offsets(const uint8_t* data, std::size_t size) -> std::vector<std::size_t> {
    std::vector<std::size_t> out;
    if (!has_annexb_start_code(data, size)) {
        return out;
    }
    for (std::size_t i = 0; i + 4U <= size; ++i) {
        const bool is_start3 = (data[i] == 0x00 && data[i + 1] == 0x00 && data[i + 2] == 0x01);
        const bool is_start4 = (i + 5U <= size) && (data[i] == 0x00 && data[i + 1] == 0x00 &&
                                                    data[i + 2] == 0x00 && data[i + 3] == 0x01);
        if (is_start4) {
            out.push_back(i + 4U);
            i += 3U;
        } else if (is_start3) {
            out.push_back(i + 3U);
            i += 2U;
        }
    }
    return out;
}

// ---------------------------------------------------------------------------
// AU assembler
// ---------------------------------------------------------------------------

namespace {

// NAL type 9 (AUD).
constexpr uint8_t kNalTypeAud = 9;

}  // namespace

auto AnnexBAuAssembler::feed(const uint8_t* data, std::size_t size, uint64_t timestamp_us) -> bool {
    if (!has_annexb_start_code(data, size) || size < 4U) {
        return false;
    }

    // Determine NAL type of the *first* NAL unit in this sample. The sample
    // may contain multiple NAL units (already an AU) — in that case we
    // treat it as a complete AU and emit it directly.
    const auto offsets = find_nal_offsets(data, size);
    if (offsets.empty()) {
        return false;
    }
    const uint8_t first_nal_type = data[offsets.front()] & 0x1FU;

    // Multi-NAL sample → assume complete AU, ship pending then ship this.
    if (offsets.size() > 1) {
        if (!pending_.empty()) {
            ready_buf_ = std::move(pending_);
            ready_ts_us_ = pending_ts_us_;
            pending_.clear();
            ready_ = true;
        }
        // Either emit now or buffer to ready depending on whether one was
        // produced. We choose: if ready slot occupied, defer; otherwise emit.
        if (ready_) {
            // Buffer this complete AU as pending for next call.
            pending_.assign(data, data + size);
            pending_ts_us_ = timestamp_us;
        } else {
            ready_buf_.assign(data, data + size);
            ready_ts_us_ = timestamp_us;
            ready_ = true;
        }
        return ready_;
    }

    // Single NAL per sample. Aggregate.
    auto flush_pending = [&]() {
        if (!pending_.empty() && !ready_) {
            ready_buf_ = std::move(pending_);
            ready_ts_us_ = pending_ts_us_;
            pending_.clear();
            ready_ = true;
        }
    };

    // Boundary detection:
    //   - AUD always starts a new AU.
    //   - Different timestamp implies a new AU.
    //   - SPS or IDR after accumulated data implies a new AU *only* when
    //     timestamps are unavailable (zero).  When timestamps are present,
    //     same-timestamp SPS/IDR/slice/PPS all belong to the same AU
    //     (e.g. upstream sends SPS, PPS, IDR as separate samples all at the
    //     same timestamp_us).
    if (first_nal_type == kNalTypeAud) {
        flush_pending();
    } else if (!pending_.empty()) {
        const bool ts_known = (timestamp_us != 0 && pending_ts_us_ != 0);
        if (ts_known && timestamp_us != pending_ts_us_) {
            flush_pending();
        } else if (!ts_known && (first_nal_type == kNalTypeSps || first_nal_type == kNalTypeIdr)) {
            // No timestamp info — fall back to NAL-type boundary heuristic.
            flush_pending();
        }
    }

    if (pending_.empty()) {
        pending_.assign(data, data + size);
        pending_ts_us_ = timestamp_us;
    } else {
        pending_.insert(pending_.end(), data, data + size);
        // Keep earliest timestamp for the AU.
        if (pending_ts_us_ == 0) {
            pending_ts_us_ = timestamp_us;
        }
    }

    return ready_;
}

auto AnnexBAuAssembler::flush() -> bool {
    if (pending_.empty()) {
        return ready_;
    }
    if (ready_) {
        // Already a buffered ready AU; keep pending for next take.
        return true;
    }
    ready_buf_ = std::move(pending_);
    ready_ts_us_ = pending_ts_us_;
    pending_.clear();
    ready_ = true;
    return true;
}

auto scan_sps_pps(const uint8_t* data, std::size_t size, bool& has_sps, bool& has_pps,
                  bool& has_idr) noexcept -> bool {
    has_sps = false;
    has_pps = false;
    has_idr = false;

    if (!has_annexb_start_code(data, size)) {
        return false;
    }

    // Walk the buffer scanning for start codes and reading the NAL header byte.
    for (std::size_t i = 0; i + 4U <= size; ++i) {
        bool is_start3 = (data[i] == 0x00 && data[i + 1] == 0x00 && data[i + 2] == 0x01);
        bool is_start4 = (i + 5U <= size) && (data[i] == 0x00 && data[i + 1] == 0x00 &&
                                              data[i + 2] == 0x00 && data[i + 3] == 0x01);

        std::size_t nal_offset = 0;
        if (is_start4) {
            nal_offset = i + 4U;
            i += 3U;  // the for-loop adds 1
        } else if (is_start3) {
            nal_offset = i + 3U;
            i += 2U;
        } else {
            continue;
        }

        if (nal_offset >= size) {
            break;
        }

        const uint8_t nal_type = data[nal_offset] & 0x1FU;
        if (nal_type == kNalTypeSps) {
            has_sps = true;
        } else if (nal_type == kNalTypePps) {
            has_pps = true;
        } else if (nal_type == kNalTypeIdr) {
            has_idr = true;
        }
    }

    return true;
}

auto scan_h264_slice_types(const uint8_t* data, std::size_t size) -> H264SliceTypeStats {
    H264SliceTypeStats stats;
    if (!has_annexb_start_code(data, size)) {
        return stats;
    }

    const auto offsets = find_nal_offsets(data, size);
    for (const auto nal_offset : offsets) {
        if (nal_offset >= size) {
            continue;
        }
        const uint8_t nal_type = data[nal_offset] & 0x1FU;
        if (nal_type != kNalTypeNonIdr && nal_type != kNalTypeIdr) {
            continue;
        }

        ++stats.vcl_nalus;
        if (nal_type == kNalTypeIdr) {
            ++stats.idr_nalus;
        }

        const auto payload_start = nal_offset + 1U;
        const auto payload_end = find_next_start_code(data, size, payload_start);
        if (payload_start >= payload_end || payload_end > size) {
            ++stats.unknown_slices;
            continue;
        }

        uint32_t slice_type = 0;
        if (!parse_slice_type(data + payload_start, payload_end - payload_start, slice_type)) {
            ++stats.unknown_slices;
            continue;
        }

        switch (normalized_slice_type(slice_type)) {
            case 0:
                ++stats.p_slices;
                break;
            case 1:
                ++stats.b_slices;
                break;
            case 2:
                ++stats.i_slices;
                break;
            case 3:
                ++stats.sp_slices;
                break;
            case 4:
                ++stats.si_slices;
                break;
            default:
                ++stats.unknown_slices;
                break;
        }
    }
    return stats;
}

auto h264_picture_type_label(const H264SliceTypeStats& stats) noexcept -> char {
    uint32_t categories = 0;
    categories += stats.p_slices > 0U ? 1U : 0U;
    categories += stats.b_slices > 0U ? 1U : 0U;
    categories += stats.i_slices > 0U ? 1U : 0U;
    categories += stats.sp_slices > 0U ? 1U : 0U;
    categories += stats.si_slices > 0U ? 1U : 0U;

    if (categories > 1U) {
        return 'M';
    }
    if (stats.b_slices > 0U) {
        return 'B';
    }
    if (stats.p_slices > 0U) {
        return 'P';
    }
    if (stats.i_slices > 0U || stats.idr_nalus > 0U) {
        return 'I';
    }
    if (stats.sp_slices > 0U) {
        return 'S';
    }
    if (stats.si_slices > 0U) {
        return 'T';
    }
    return '?';
}

}  // namespace rollio::coracam
