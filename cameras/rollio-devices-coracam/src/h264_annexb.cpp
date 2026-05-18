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

// Append a minimal NAL unit: header byte + a few non-emulation-prevention
// payload bytes so the unit is non-empty and NAL scanners can parse it.
void append_nal(std::vector<uint8_t>& out, uint8_t header, uint64_t seed, uint8_t extra_len = 4) {
    append_start(out);
    out.push_back(header);
    // Fill payload bytes derived from seed so each AU is unique and tests
    // can verify the payload changes between frames.
    for (uint8_t i = 0; i < extra_len; ++i) {
        const uint8_t b = static_cast<uint8_t>((seed >> (i * 8U)) & 0xFFU);
        // Avoid 0x00 runs that could look like emulation-prevention bytes.
        out.push_back(b != 0 ? b : 0x01U);
    }
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

        // IDR slice header placeholder. Real decoders would reject this,
        // but the coracam passthrough only inspects NAL type bytes.
        append_nal(au, kNalHeaderIdr, frame_index, 8U);
    } else {
        // Delta frame: single non-IDR slice.
        append_nal(au, kNalHeaderSlice, frame_index, 6U);
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

}  // namespace rollio::coracam
