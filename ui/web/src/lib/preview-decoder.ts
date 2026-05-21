/**
 * WebCodecs-based decoder registry for the encoded preview path.
 *
 * The visualizer broadcasts H.264 (codecId 0) / H.265 (1) / AV1 (2)
 * preview streams to the web UI as self-contained kind-0x03
 * (`encoded_packet`) messages. Keyframes carry inline SPS/PPS, so
 * `usePreviewSocket` auto-configures the decoder from the first
 * keyframe per camera (passing the AU payload as the `description`).
 *
 * This module owns the per-camera `VideoDecoder` lifetimes and
 * converts decoded `VideoFrame`s into a small `DecodedFrame` event
 * consumed by `usePreviewSocket`.
 *
 * The registry is injected via `UsePreviewSocketOptions.decoderRegistryFactory`
 * (mirroring the existing `objectUrlFactory` / `websocketFactory`
 * seams). Tests substitute a `FakeDecoderRegistry`; production uses the
 * `PreviewDecoderRegistry` defined here.
 */

import { videoDecoderAvailability } from "./browser-codecs";
import { incrementGauge, setGauge } from "./debug-metrics";

export interface DecodedFrame {
  name: string;
  videoFrame: VideoFrame;
  width: number;
  height: number;
  /** Codec PTS in µs (monotonic from recording start). Forwarded
   *  through the decoder unchanged via `VideoFrame.timestamp`. */
  timestampUs: number;
  /** Camera capture wall-clock µs since UNIX epoch — looked up from
   *  the per-PTS map populated during `decode()`. Use this for
   *  capture-to-display latency metrics. */
  sourceTimestampUs: number;
  receivedAtWallTimeMs: number;
}

export type DecoderRegistryFrameCallback = (frame: DecodedFrame) => void;

export interface DecoderRegistry {
  /**
   * Configure (or reconfigure) the decoder for `name`. Idempotent:
   * if a decoder is already configured for this camera, it is closed
   * first so the new `description` takes effect — this handles
   * stream restarts triggered by `set_preview_size`.
   */
  configure(
    name: string,
    codecId: number,
    description: Uint8Array,
    width: number,
    height: number,
    onFrame: DecoderRegistryFrameCallback,
  ): void;

  /** Submit one encoded access unit. Calls `onFrame` (registered via
   *  `configure`) when the decoder produces output. `sourceTimestampUs`
   *  is the camera's wall-clock capture time, surfaced back on
   *  `DecodedFrame` for latency metrics. */
  decode(
    name: string,
    payload: Uint8Array,
    ptsUs: number,
    sourceTimestampUs: number,
    isKeyframe: boolean,
  ): void;

  /** Tear down the decoder for one camera (e.g. on socket disconnect). */
  close(name: string): void;

  /** Tear down every decoder. Called on hook unmount and on socket flap. */
  closeAll(): void;
}

/** EncodedCodecId discriminants from `rollio-types/src/messages.rs`.
 *  Mirrored here so the wire codec id can be turned into a WebCodecs
 *  `codec` string without dragging in a Rust-generated bindings file. */
const CODEC_ID_H264 = 0;
const CODEC_ID_H265 = 1;
const CODEC_ID_AV1 = 2;

interface DecoderEntry {
  decoder: VideoDecoder;
  codecString: string;
  width: number;
  height: number;
  onFrame: DecoderRegistryFrameCallback;
  /** WebCodecs preserves `EncodedVideoChunk.timestamp` onto each
   *  output `VideoFrame.timestamp`, so we use it as a join key to
   *  look up the camera-side wall-clock timestamp recorded at
   *  `decode()` time. Entries are removed on lookup; entries that
   *  the decoder drops (B-frame reorder, dim change, etc.) age out
   *  via a size cap to bound memory. */
  pendingSourceTs: Map<number, number>;
}

/** Upper bound on the per-decoder ptsUs → sourceTimestampUs map.
 *  At 60 fps the decoder usually outputs within ~1 frame, so a few
 *  hundred entries is generous; this cap exists only to bound memory
 *  in case the decoder silently drops frames. */
const MAX_PENDING_SOURCE_TS = 256;

export class PreviewDecoderRegistry implements DecoderRegistry {
  private readonly entries = new Map<string, DecoderEntry>();

  configure(
    name: string,
    codecId: number,
    description: Uint8Array,
    width: number,
    height: number,
    onFrame: DecoderRegistryFrameCallback,
  ): void {
    const availability = videoDecoderAvailability();
    if (!availability.available) {
      console.warn(
        `[preview-decoder] ${availability.detail} Cannot configure ${name}.`,
      );
      return;
    }

    const codecString = codecStringFor(codecId, description);
    if (!codecString) {
      console.warn(
        `[preview-decoder] unsupported codecId ${codecId} for ${name}`,
      );
      setGauge(`ui.preview_decoder_last_error.${name}`, `unsupported codecId ${codecId}`);
      return;
    }
    setGauge(`ui.preview_decoder_description_bytes.${name}`, description.byteLength);

    // Stream restart: close any existing decoder for this name so
    // queued frames from the prior session don't surface here.
    this.close(name);

    const decoder = new VideoDecoder({
      output: (videoFrame) => {
        const entry = this.entries.get(name);
        if (!entry) {
          videoFrame.close();
          return;
        }
        const pts = videoFrame.timestamp ?? 0;
        // Wall-clock at decode() time; falls back to current Date.now()
        // for output frames whose PTS doesn't correspond to a recorded
        // submit (shouldn't happen but keeps the metric finite).
        const sourceTs = entry.pendingSourceTs.get(pts) ?? Date.now() * 1000;
        entry.pendingSourceTs.delete(pts);
        entry.onFrame({
          name,
          videoFrame,
          width: entry.width,
          height: entry.height,
          timestampUs: pts,
          sourceTimestampUs: sourceTs,
          receivedAtWallTimeMs: Date.now(),
        });
      },
      error: (error) => {
        console.warn(`[preview-decoder] ${name} decoder error: ${error}`);
        incrementGauge(`ui.preview_decoder_errors_total.${name}`);
        setGauge(`ui.preview_decoder_last_error.${name}`, String(error));
      },
    });

    try {
      // Annex B mode: omit `description` so WebCodecs expects
      // start-code-prefixed NAL units in each `EncodedVideoChunk` and
      // reads SPS/PPS in-band. Encoders run without GLOBAL_HEADER so
      // every keyframe AU carries inline SPS/PPS — the parameter sets
      // the decoder needs to (re)initialize travel with the bitstream
      // itself, no out-of-band description required.
      decoder.configure({
        codec: codecString,
        codedWidth: width,
        codedHeight: height,
        optimizeForLatency: true,
      });
    } catch (error) {
      console.warn(
        `[preview-decoder] ${name} configure failed: ${error}`,
      );
      incrementGauge(`ui.preview_decoder_configure_failures_total.${name}`);
      setGauge(`ui.preview_decoder_last_error.${name}`, `configure: ${error}`);
      try {
        decoder.close();
      } catch {
        /* configure failure leaves the decoder unusable */
      }
      return;
    }

    setGauge(`ui.preview_decoder_codec_string.${name}`, codecString);
    setGauge(`ui.preview_decoder_state.${name}`, "configured");
    this.entries.set(name, {
      decoder,
      codecString,
      width,
      height,
      onFrame,
      pendingSourceTs: new Map(),
    });
  }

  decode(
    name: string,
    payload: Uint8Array,
    ptsUs: number,
    sourceTimestampUs: number,
    isKeyframe: boolean,
  ): void {
    const entry = this.entries.get(name);
    if (!entry) {
      incrementGauge(`ui.preview_decoder_drops_unconfigured.${name}`);
      return;
    }
    if (entry.decoder.state !== "configured") {
      incrementGauge(`ui.preview_decoder_drops_not_configured.${name}`);
      setGauge(`ui.preview_decoder_state.${name}`, entry.decoder.state);
      return;
    }
    entry.pendingSourceTs.set(ptsUs, sourceTimestampUs);
    if (entry.pendingSourceTs.size > MAX_PENDING_SOURCE_TS) {
      // Drop the oldest entries (insertion order = Map iteration order
      // in JS), keeping the most recent. Bounds memory if the decoder
      // ever stops producing output without notice.
      const drop = entry.pendingSourceTs.size - MAX_PENDING_SOURCE_TS;
      let i = 0;
      for (const key of entry.pendingSourceTs.keys()) {
        if (i++ >= drop) break;
        entry.pendingSourceTs.delete(key);
      }
    }
    try {
      const chunk = new EncodedVideoChunk({
        type: isKeyframe ? "key" : "delta",
        timestamp: ptsUs,
        // Copy the payload defensively: the caller may reuse the
        // backing ArrayBuffer for the next message, while the
        // EncodedVideoChunk constructor takes a snapshot at call
        // time.
        data: payload,
      });
      entry.decoder.decode(chunk);
    } catch (error) {
      console.warn(`[preview-decoder] ${name} decode failed: ${error}`);
      incrementGauge(`ui.preview_decoder_decode_failures_total.${name}`);
      setGauge(`ui.preview_decoder_last_error.${name}`, `decode: ${error}`);
    }
  }

  close(name: string): void {
    const entry = this.entries.get(name);
    if (!entry) {
      return;
    }
    setGauge(`ui.preview_decoder_state.${name}`, "closed");
    this.entries.delete(name);
    try {
      entry.decoder.close();
    } catch {
      /* a decoder that has already errored throws on close; safe to ignore */
    }
  }

  closeAll(): void {
    for (const name of Array.from(this.entries.keys())) {
      this.close(name);
    }
  }
}

/**
 * Build a WebCodecs `codec` string for the given Rollio codec id.
 *
 * For H.264, the visualizer hands us the encoder's Annex B extrahome/usere8ece17c/projects
 * verbatim — a sequence of start-code-prefixed NAL units containing
 * at minimum one SPS and one PPS. We locate the SPS (NAL type 7) and
 * read the profile_idc / constraint_set_flags / level_idc bytes that
 * directly follow the NAL header, then format them as `avc1.PPCCLL`.
 *
 * H.265 / AV1 callers need richer codec strings derived from the
 * stream config (HVCC for HEVC, OBU sequence-header parsing for AV1).
 * Today the visualizer only encodes H.264 previews, so we ship a
 * conservative HEV1 string for H.265 and a baseline AV1 string and
 * leave a richer derivation for follow-up.
 */
export function codecStringFor(
  codecId: number,
  description: Uint8Array,
): string | null {
  switch (codecId) {
    case CODEC_ID_H264: {
      const sps = findAnnexBNalu(description, 7);
      if (!sps || sps.byteLength < 4) {
        return "avc1.42E01F"; // baseline / 3.1 fallback
      }
      // SPS byte 0 is the NAL header; bytes 1/2/3 are profile_idc,
      // constraint_set_flags (a.k.a. profile_compatibility), and
      // level_idc respectively. These are the three hex pairs in
      // WebCodecs' canonical `avc1.PPCCLL` codec string. The `avc1`
      // prefix MUST stay lowercase — Chrome's codec-name parser is
      // case-sensitive and rejects `AVC1.…` with
      // `NotSupportedError: Unknown or ambiguous codec name.`
      const profile = sps[1].toString(16).padStart(2, "0").toUpperCase();
      const compat = sps[2].toString(16).padStart(2, "0").toUpperCase();
      const level = sps[3].toString(16).padStart(2, "0").toUpperCase();
      return `avc1.${profile}${compat}${level}`;
    }
    case CODEC_ID_H265:
      return "hev1.1.6.L93.B0";
    case CODEC_ID_AV1:
      return "av01.0.04M.08";
    default:
      return null;
  }
}

/**
 * Walk a byte slice in Annex B framing and return the first NAL unit
 * body (start codes stripped) whose `nal_unit_type` matches `wanted`.
 * Handles both 3-byte (`00 00 01`) and 4-byte (`00 00 00 01`) start
 * codes; returns null if no matching NAL is found.
 */
function findAnnexBNalu(bytes: Uint8Array, wanted: number): Uint8Array | null {
  const starts: Array<{ offset: number; prefix: number }> = [];
  let i = 0;
  while (i + 2 < bytes.byteLength) {
    if (bytes[i] === 0x00 && bytes[i + 1] === 0x00) {
      if (bytes[i + 2] === 0x01) {
        starts.push({ offset: i + 3, prefix: 3 });
        i += 3;
        continue;
      }
      if (i + 3 < bytes.byteLength && bytes[i + 2] === 0x00 && bytes[i + 3] === 0x01) {
        starts.push({ offset: i + 4, prefix: 4 });
        i += 4;
        continue;
      }
    }
    i += 1;
  }
  for (let k = 0; k < starts.length; k++) {
    const begin = starts[k].offset;
    const end = k + 1 < starts.length
      ? starts[k + 1].offset - starts[k + 1].prefix
      : bytes.byteLength;
    if (end - begin <= 0) continue;
    const nalType = bytes[begin] & 0x1f;
    if (nalType === wanted) {
      return bytes.subarray(begin, end);
    }
  }
  return null;
}
