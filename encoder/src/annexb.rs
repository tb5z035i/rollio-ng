//! Annex B (H.264 / H.265) and OBU (AV1) byte-level helpers shared by
//! the encoder backends and the downstream assemblers / muxers.
//!
//! Lives in the encoder crate because that's where the two
//! producer-side callers (passthrough, X5) and the libav drain logic
//! live; downstream crates depend on `rollio-encoder` for these
//! parsers rather than duplicating them.

/// Iterate the NALU bodies of an Annex B byte slice, with start codes
/// (3- or 4-byte) stripped. Bytes that don't sit between two start
/// codes are skipped. Returns an empty iterator on input that doesn't
/// contain any start code.
pub fn split_annex_b_nalus(bytes: &[u8]) -> impl Iterator<Item = &[u8]> {
    let mut starts: Vec<(usize, usize)> = Vec::new();
    let mut i = 0;
    while i + 2 < bytes.len() {
        if bytes[i] == 0x00 && bytes[i + 1] == 0x00 {
            if bytes[i + 2] == 0x01 {
                starts.push((i, 3));
                i += 3;
                continue;
            }
            if i + 3 < bytes.len() && bytes[i + 2] == 0x00 && bytes[i + 3] == 0x01 {
                starts.push((i, 4));
                i += 4;
                continue;
            }
        }
        i += 1;
    }
    let mut out: Vec<&[u8]> = Vec::with_capacity(starts.len());
    for (idx, &(offset, prefix)) in starts.iter().enumerate() {
        let body_start = offset + prefix;
        let body_end = if idx + 1 < starts.len() {
            starts[idx + 1].0
        } else {
            bytes.len()
        };
        if body_start <= body_end {
            out.push(&bytes[body_start..body_end]);
        }
    }
    out.into_iter()
}

/// H.264: locate SPS (NAL type 7) and PPS (NAL type 8) NAL units in an
/// Annex B AU and rebuild them into a single 4-byte-start-code-prefixed
/// buffer (`[00 00 00 01][SPS][00 00 00 01][PPS]`) suitable for an MP4
/// muxer's `extradata/tb5z035i/workspace` field or a WebCodecs `description`. Returns
/// `None` if either is missing.
pub fn extract_h264_parameter_sets(annex_b: &[u8]) -> Option<Vec<u8>> {
    let mut sps: Option<&[u8]> = None;
    let mut pps: Option<&[u8]> = None;
    for nalu in split_annex_b_nalus(annex_b) {
        if nalu.is_empty() {
            continue;
        }
        match nalu[0] & 0x1F {
            7 => {
                sps.get_or_insert(nalu);
            }
            8 => {
                pps.get_or_insert(nalu);
            }
            _ => {}
        }
    }
    let sps = sps?;
    let pps = pps?;
    let mut out = Vec::with_capacity(4 + sps.len() + 4 + pps.len());
    out.extend_from_slice(&[0x00, 0x00, 0x00, 0x01]);
    out.extend_from_slice(sps);
    out.extend_from_slice(&[0x00, 0x00, 0x00, 0x01]);
    out.extend_from_slice(pps);
    Some(out)
}

/// H.265: locate VPS (NAL type 32), SPS (33), and PPS (34) NAL units.
/// HEVC NAL headers are 2 bytes; type is `(byte[0] >> 1) & 0x3F`.
/// Returns the concatenation with 4-byte start codes, or `None` if any
/// of the three is missing.
pub fn extract_h265_parameter_sets(annex_b: &[u8]) -> Option<Vec<u8>> {
    let mut vps: Option<&[u8]> = None;
    let mut sps: Option<&[u8]> = None;
    let mut pps: Option<&[u8]> = None;
    for nalu in split_annex_b_nalus(annex_b) {
        if nalu.is_empty() {
            continue;
        }
        let nal_type = (nalu[0] >> 1) & 0x3F;
        match nal_type {
            32 => {
                vps.get_or_insert(nalu);
            }
            33 => {
                sps.get_or_insert(nalu);
            }
            34 => {
                pps.get_or_insert(nalu);
            }
            _ => {}
        }
    }
    let vps = vps?;
    let sps = sps?;
    let pps = pps?;
    let mut out = Vec::with_capacity(12 + vps.len() + sps.len() + pps.len());
    for nalu in [vps, sps, pps] {
        out.extend_from_slice(&[0x00, 0x00, 0x00, 0x01]);
        out.extend_from_slice(nalu);
    }
    Some(out)
}

/// AV1: locate the sequence-header OBU (`obu_type == 1`) inside a low-
/// overhead temporal unit and return its bytes (without any framing
/// wrapper). The output is suitable for `AVCodecParameters.extradata/tb5z035i/workspace`
/// on AV1 streams. Returns `None` if no sequence header is present.
///
/// Parses the leading OBU header byte by byte; uses LEB128 to read
/// each OBU's `obu_size` when `obu_has_size_field == 1`.
pub fn extract_av1_sequence_header(temporal_unit: &[u8]) -> Option<Vec<u8>> {
    let mut i = 0;
    while i < temporal_unit.len() {
        let header = temporal_unit[i];
        i += 1;
        let obu_type = (header >> 3) & 0x0F;
        let has_extension = (header >> 2) & 0x01 != 0;
        let has_size = (header >> 1) & 0x01 != 0;
        if has_extension {
            if i >= temporal_unit.len() {
                return None;
            }
            i += 1;
        }
        let payload_len = if has_size {
            let (len, consumed) = read_leb128(&temporal_unit[i..])?;
            i += consumed;
            len
        } else {
            (temporal_unit.len() - i) as u64
        };
        let payload_end = i.checked_add(payload_len as usize)?;
        if payload_end > temporal_unit.len() {
            return None;
        }
        if obu_type == 1 {
            return Some(temporal_unit[i..payload_end].to_vec());
        }
        i = payload_end;
    }
    None
}

fn read_leb128(bytes: &[u8]) -> Option<(u64, usize)> {
    let mut value: u64 = 0;
    let mut shift = 0;
    for (idx, &byte) in bytes.iter().enumerate().take(8) {
        value |= ((byte & 0x7F) as u64) << shift;
        if byte & 0x80 == 0 {
            return Some((value, idx + 1));
        }
        shift += 7;
    }
    None
}

/// Return true if an Annex B H.264 AU contains an IDR slice (NAL type
/// 5), an SPS (7), or a PPS (8). Used by passthrough to set the
/// keyframe flag without owning a full bitstream parser.
pub fn is_h264_keyframe(annex_b: &[u8]) -> bool {
    for nalu in split_annex_b_nalus(annex_b) {
        if nalu.is_empty() {
            continue;
        }
        let nal_type = nalu[0] & 0x1F;
        if matches!(nal_type, 5 | 7 | 8) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    const START4: &[u8] = &[0x00, 0x00, 0x00, 0x01];
    const START3: &[u8] = &[0x00, 0x00, 0x01];

    fn nal(nal_type: u8, payload: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(1 + payload.len());
        out.push(nal_type & 0x1F);
        out.extend_from_slice(payload);
        out
    }

    fn au(units: &[(&[u8], &[u8])]) -> Vec<u8> {
        let mut out = Vec::new();
        for (start, body) in units {
            out.extend_from_slice(start);
            out.extend_from_slice(body);
        }
        out
    }

    #[test]
    fn split_handles_4byte_start_codes() {
        let sps = nal(7, b"sps-data/tb5z035i/workspace");
        let pps = nal(8, b"pps-data/tb5z035i/workspace");
        let idr = nal(5, b"idr-slice");
        let bytes = au(&[(START4, &sps), (START4, &pps), (START4, &idr)]);
        let nalus: Vec<&[u8]> = split_annex_b_nalus(&bytes).collect();
        assert_eq!(nalus.len(), 3);
        assert_eq!(nalus[0], &sps[..]);
        assert_eq!(nalus[1], &pps[..]);
        assert_eq!(nalus[2], &idr[..]);
    }

    #[test]
    fn split_handles_3byte_start_codes() {
        let sps = nal(7, b"sps");
        let pps = nal(8, b"pps");
        let idr = nal(5, b"idr");
        let bytes = au(&[(START3, &sps), (START3, &pps), (START3, &idr)]);
        let nalus: Vec<&[u8]> = split_annex_b_nalus(&bytes).collect();
        assert_eq!(nalus.len(), 3);
        assert_eq!(nalus[0], &sps[..]);
        assert_eq!(nalus[2], &idr[..]);
    }

    #[test]
    fn split_handles_mixed_prefix_lengths() {
        let sps = nal(7, b"sps");
        let idr = nal(5, b"idr");
        let bytes = au(&[(START4, &sps), (START3, &idr)]);
        let nalus: Vec<&[u8]> = split_annex_b_nalus(&bytes).collect();
        assert_eq!(nalus.len(), 2);
        assert_eq!(nalus[0], &sps[..]);
        assert_eq!(nalus[1], &idr[..]);
    }

    #[test]
    fn split_returns_empty_on_garbage() {
        let nalus: Vec<&[u8]> = split_annex_b_nalus(&[0x01, 0x02, 0x03, 0x04]).collect();
        assert!(nalus.is_empty());
    }

    #[test]
    fn extract_h264_with_sps_and_pps() {
        let sps = nal(7, b"sps-bytes");
        let pps = nal(8, b"pps-bytes");
        let idr = nal(5, b"idr-bytes");
        let bytes = au(&[(START3, &sps), (START3, &pps), (START3, &idr)]);
        let result = extract_h264_parameter_sets(&bytes).expect("sps+pps present");
        let mut expected = Vec::new();
        expected.extend_from_slice(START4);
        expected.extend_from_slice(&sps);
        expected.extend_from_slice(START4);
        expected.extend_from_slice(&pps);
        assert_eq!(result, expected);
    }

    #[test]
    fn extract_h264_returns_none_when_pps_missing() {
        let sps = nal(7, b"sps");
        let idr = nal(5, b"idr");
        let bytes = au(&[(START4, &sps), (START4, &idr)]);
        assert!(extract_h264_parameter_sets(&bytes).is_none());
    }

    #[test]
    fn extract_h264_returns_none_when_sps_missing() {
        let pps = nal(8, b"pps");
        let idr = nal(5, b"idr");
        let bytes = au(&[(START4, &pps), (START4, &idr)]);
        assert!(extract_h264_parameter_sets(&bytes).is_none());
    }

    #[test]
    fn extract_h265_with_vps_sps_pps() {
        let vps = vec![(32 << 1) | 0, 0x00, 0xaa];
        let sps = vec![(33 << 1) | 0, 0x00, 0xbb];
        let pps = vec![(34 << 1) | 0, 0x00, 0xcc];
        let bytes = au(&[(START4, &vps), (START4, &sps), (START4, &pps)]);
        let result = extract_h265_parameter_sets(&bytes).expect("vps+sps+pps present");
        let mut expected = Vec::new();
        for nalu in [&vps, &sps, &pps] {
            expected.extend_from_slice(START4);
            expected.extend_from_slice(nalu);
        }
        assert_eq!(result, expected);
    }

    #[test]
    fn extract_h265_returns_none_when_vps_missing() {
        let sps = vec![(33 << 1) | 0, 0x00, 0xbb];
        let pps = vec![(34 << 1) | 0, 0x00, 0xcc];
        let bytes = au(&[(START4, &sps), (START4, &pps)]);
        assert!(extract_h265_parameter_sets(&bytes).is_none());
    }

    #[test]
    fn is_keyframe_true_on_idr() {
        let idr = nal(5, b"idr");
        let bytes = au(&[(START4, &idr)]);
        assert!(is_h264_keyframe(&bytes));
    }

    #[test]
    fn is_keyframe_true_on_sps_pps_idr() {
        let sps = nal(7, b"sps");
        let pps = nal(8, b"pps");
        let idr = nal(5, b"idr");
        let bytes = au(&[(START4, &sps), (START4, &pps), (START4, &idr)]);
        assert!(is_h264_keyframe(&bytes));
    }

    #[test]
    fn is_keyframe_false_on_p_slice_only() {
        let p = nal(1, b"p-slice");
        let bytes = au(&[(START4, &p)]);
        assert!(!is_h264_keyframe(&bytes));
    }

    #[test]
    fn av1_sequence_header_with_size_field() {
        // OBU header for sequence header (obu_type=1) with has_size=1,
        // no extension, reserved=0:
        // bits 7..0 = 0 0001 0 1 0 → 0b00001010 = 0x0A
        let mut tu = Vec::new();
        tu.push(0x0A);
        // size: 3 bytes (single-byte LEB128)
        tu.push(0x03);
        tu.extend_from_slice(&[0xaa, 0xbb, 0xcc]);
        let result = extract_av1_sequence_header(&tu).expect("seq header present");
        assert_eq!(result, vec![0xaa, 0xbb, 0xcc]);
    }

    #[test]
    fn av1_sequence_header_skips_other_obus() {
        // First OBU: temporal delimiter (obu_type=2), has_size=1
        // 0 0010 0 1 0 = 0b00010010 = 0x12
        let mut tu = Vec::new();
        tu.push(0x12);
        tu.push(0x00);
        // Second OBU: sequence header (obu_type=1), has_size=1, size=2
        tu.push(0x0A);
        tu.push(0x02);
        tu.extend_from_slice(&[0xde, 0xad]);
        let result = extract_av1_sequence_header(&tu).expect("seq header present");
        assert_eq!(result, vec![0xde, 0xad]);
    }
}
