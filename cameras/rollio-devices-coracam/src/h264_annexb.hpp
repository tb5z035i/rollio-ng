#ifndef ROLLIO_DEVICES_CORACAM_H264_ANNEXB_HPP
#define ROLLIO_DEVICES_CORACAM_H264_ANNEXB_HPP

#include <cstddef>
#include <cstdint>
#include <vector>

namespace rollio::coracam {

// H264 Annex-B utilities used by the coracam channel workers.
//
// NAL unit types (nal_unit_type bits 4:0 of the first byte after start code):
//   1 = Non-IDR coded slice
//   5 = IDR coded slice (keyframe)
//   7 = SPS
//   8 = PPS
//
// These are the only types the coracam/passthrough layer cares about for
// keyframe detection, config extraction, and AU boundary decisions.

constexpr uint8_t kNalTypeSps = 7;
constexpr uint8_t kNalTypePps = 8;
constexpr uint8_t kNalTypeIdr = 5;
constexpr uint8_t kNalTypeNonIdr = 1;

// NAL header byte helpers. Note: nal_unit_type is the low 5 bits.
inline constexpr uint8_t nal_header(uint8_t ref_idc, uint8_t nal_type) noexcept {
    return static_cast<uint8_t>((ref_idc & 0x3U) << 5U) | (nal_type & 0x1FU);
}

// Minimal SPS payload prefix (enough for a real decoder to parse the
// profile/level, but we fill the rest with zeros). This is not a valid
// decodable stream; it is sufficient for the encoder passthrough backend
// to extract a codec config and forward it as-is.
inline constexpr uint8_t kNalHeaderSps = nal_header(3, kNalTypeSps);       // 0x67
inline constexpr uint8_t kNalHeaderPps = nal_header(3, kNalTypePps);       // 0x68
inline constexpr uint8_t kNalHeaderIdr = nal_header(3, kNalTypeIdr);       // 0x65
inline constexpr uint8_t kNalHeaderSlice = nal_header(2, kNalTypeNonIdr);  // 0x41

// Return a minimal Annex-B access unit.
//
// keyframe: [0x00,0x00,0x00,0x01, SPS_NAL...] [start] [PPS_NAL...] [start] [IDR_NAL...]
// delta:    [0x00,0x00,0x00,0x01, slice_NAL...]
//
// Payload bytes beyond the NAL header are a short non-zero RBSP so a
// NAL scanner does not confuse emulation prevention bytes with a start code.
auto make_mock_annexb_au(bool keyframe, uint64_t frame_index) -> std::vector<uint8_t>;

// Return true if the slice [data, size) contains an Annex-B start code
// (3- or 4-byte). Used for hard validation of incoming samples.
auto has_annexb_start_code(const uint8_t* data, std::size_t size) noexcept -> bool;

// Scan for SPS and PPS NAL units in an Annex-B access unit. Sets
// *has_sps / *has_pps accordingly. Returns false if the buffer does
// not start with a valid start code.
auto scan_sps_pps(const uint8_t* data, std::size_t size, bool& has_sps, bool& has_pps,
                  bool& has_idr) noexcept -> bool;

// Best-effort H.264 VCL slice-type summary for one Annex-B access unit.
//
// This parses only the first two Exp-Golomb fields of each VCL slice
// header (`first_mb_in_slice`, `slice_type`). It is intentionally not a
// decoder; it is enough to distinguish common GOP shapes such as all-I,
// IPPPP, or streams containing B slices.
struct H264SliceTypeStats {
    uint32_t vcl_nalus{0};
    uint32_t idr_nalus{0};
    uint32_t p_slices{0};
    uint32_t b_slices{0};
    uint32_t i_slices{0};
    uint32_t sp_slices{0};
    uint32_t si_slices{0};
    uint32_t unknown_slices{0};
};

auto scan_h264_slice_types(const uint8_t* data, std::size_t size) -> H264SliceTypeStats;

// Return a compact picture label derived from the VCL slice summary:
//   I/P/B/S/T for I/P/B/SP/SI, M for mixed slice types, ? for unknown.
auto h264_picture_type_label(const H264SliceTypeStats& stats) noexcept -> char;

// Find the offset to the *NAL header byte* for every NAL unit in the buffer
// (i.e. the byte right after each start code). Returns an empty vector if
// no start code is found. Used by the AU assembler for boundary decisions
// and by unit tests for golden bytes verification.
auto find_nal_offsets(const uint8_t* data, std::size_t size) -> std::vector<std::size_t>;

// ---------------------------------------------------------------------------
// AU assembler — used when the upstream Cora publisher emits one NAL per
// DDS sample instead of a complete access unit per sample.
//
// Strategy: accumulate NAL units until the next sample either
//   (a) carries an AUD (NAL type 9), or
//   (b) has a different DTS/PTS than the running aggregate, or
//   (c) carries a slice (1 or 5) whose first_mb_in_slice == 0 indicating
//       a new picture boundary.
//
// The first complete AU is returned; the assembler retains the partial
// AU under construction until the next call. Callers must check is_ready()
// before consuming the buffer.
// ---------------------------------------------------------------------------

class AnnexBAuAssembler {
public:
    AnnexBAuAssembler() = default;

    // Feed one NAL-only sample plus its timestamp (microseconds).
    // Returns true when an AU has just become ready; call take() to
    // consume the bytes. Returns false when the sample was buffered or
    // rejected (no start code).
    auto feed(const uint8_t* data, std::size_t size, uint64_t timestamp_us) -> bool;

    // Force the current aggregate out as an AU (used on shutdown / IDR
    // handoff). Returns false if the buffer is empty.
    auto flush() -> bool;

    [[nodiscard]] auto is_ready() const noexcept -> bool {
        return ready_;
    }
    [[nodiscard]] auto ready_au() const noexcept -> const std::vector<uint8_t>& {
        return ready_buf_;
    }
    [[nodiscard]] auto ready_timestamp_us() const noexcept -> uint64_t {
        return ready_ts_us_;
    }

    // Consume the current ready AU. After this call is_ready() returns false.
    auto take(std::vector<uint8_t>& out) -> uint64_t {
        out = std::move(ready_buf_);
        ready_buf_.clear();
        ready_ = false;
        return ready_ts_us_;
    }

private:
    std::vector<uint8_t> pending_;
    uint64_t pending_ts_us_{0};

    std::vector<uint8_t> ready_buf_;
    uint64_t ready_ts_us_{0};
    bool ready_{false};
};

// ---------------------------------------------------------------------------
// EagerCoraSdkNalAssembler — temporary bridge for Cora SDK H264 publisher.
//
// TODO(coracam-temp): Remove this class (and the wire-up in
// channel_worker.cpp:worker_loop_cora_h264) once the Cora SDK ships an
// AU-granular H.264 publisher. Today the SDK emits one NAL per DDS sample,
// and the downstream iceoryx bus expects whole Annex-B access units.
//
// Known NAL set at the upstream: SPS(7), PPS(8), IDR(5), non-IDR slice(1).
// Strategy: a small state machine over these four types. Emit immediately
// when an AU is complete, do not wait for the next sample:
//   - keyframe AU = [SPS][PPS][IDR]  → emit when IDR arrives after SPS+PPS
//   - delta AU    = [non-IDR slice]  → emit immediately
// Parameter-set orphans (PPS without SPS, IDR without SPS+PPS) are dropped
// and counted. The assembler keeps at most one ready AU; the caller must
// take() before the next feed() so a second AU does not arrive while one
// is still buffered.
// ---------------------------------------------------------------------------

class EagerCoraSdkNalAssembler {
public:
    struct Counters {
        uint64_t orphan_pps{0};            // PPS arrived without preceding SPS
        uint64_t orphan_idr{0};            // IDR arrived without SPS+PPS pair
        uint64_t slice_breaks_param_set{0};  // non-IDR slice arrived mid-param-set
        uint64_t param_set_resets{0};      // SPS observed while already mid-param-set
        uint64_t unknown_nal{0};           // NAL type outside the known 4
    };

    EagerCoraSdkNalAssembler() = default;

    // Feed one NAL-only sample (must start with an Annex-B start code) plus
    // its timestamp (microseconds). Returns true when an AU has just become
    // ready; call take() to consume the bytes.
    auto feed(const uint8_t* data, std::size_t size, uint64_t timestamp_us) -> bool;

    // Force the in-flight buffer out as an AU. Used at shutdown only.
    // Returns true if take() now has an AU to deliver.
    auto flush() -> bool;

    [[nodiscard]] auto is_ready() const noexcept -> bool {
        return ready_;
    }
    [[nodiscard]] auto pending_bytes() const noexcept -> std::size_t {
        return pending_.size();
    }
    [[nodiscard]] auto counters() const noexcept -> const Counters& {
        return counters_;
    }

    auto take(std::vector<uint8_t>& out) -> uint64_t {
        out = std::move(ready_buf_);
        ready_buf_.clear();
        ready_ = false;
        return ready_ts_us_;
    }

private:
    enum class State : uint8_t { Idle, GotSps, GotSpsPps };

    void emit_pending();
    void emit_single(const uint8_t* data, std::size_t size, uint64_t timestamp_us);

    State state_{State::Idle};
    std::vector<uint8_t> pending_;
    uint64_t pending_ts_us_{0};
    std::size_t pps_offset_{0};  // start of PPS NAL inside pending_ (incl. start code)

    std::vector<uint8_t> ready_buf_;
    uint64_t ready_ts_us_{0};
    bool ready_{false};

    Counters counters_{};
};

}  // namespace rollio::coracam

#endif  // ROLLIO_DEVICES_CORACAM_H264_ANNEXB_HPP
