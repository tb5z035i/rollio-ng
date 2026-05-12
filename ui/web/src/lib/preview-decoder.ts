/**
 * WebCodecs-based decoder registry for the encoded preview path.
 *
 * The visualizer broadcasts H.264 (codecId 0) / H.265 (1) / AV1 (2)
 * preview streams to the web UI as binary kind-0x02 (`encoded_config`)
 * and kind-0x03 (`encoded_packet`) messages. This module owns the
 * per-camera `VideoDecoder` lifetimes and converts decoded
 * `VideoFrame`s into a small `DecodedFrame` event consumed by
 * `usePreviewSocket`.
 *
 * The registry is injected via `UsePreviewSocketOptions.decoderRegistryFactory`
 * (mirroring the existing `objectUrlFactory` / `websocketFactory`
 * seams). Tests substitute a `FakeDecoderRegistry`; production uses the
 * `PreviewDecoderRegistry` defined here.
 */

export interface DecodedFrame {
  name: string;
  videoFrame: VideoFrame;
  width: number;
  height: number;
  timestampUs: number;
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
   *  `configure`) when the decoder produces output. */
  decode(
    name: string,
    payload: Uint8Array,
    ptsUs: number,
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
}

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
    if (typeof VideoDecoder === "undefined") {
      console.warn(
        `[preview-decoder] WebCodecs VideoDecoder unavailable; cannot configure ${name}`,
      );
      return;
    }

    const codecString = codecStringFor(codecId, description);
    if (!codecString) {
      console.warn(
        `[preview-decoder] unsupported codecId ${codecId} for ${name}`,
      );
      return;
    }

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
        entry.onFrame({
          name,
          videoFrame,
          width: entry.width,
          height: entry.height,
          timestampUs: videoFrame.timestamp ?? 0,
          receivedAtWallTimeMs: Date.now(),
        });
      },
      error: (error) => {
        console.warn(`[preview-decoder] ${name} decoder error: ${error}`);
      },
    });

    try {
      // Pass the AVCC `description` from the visualizer verbatim. The
      // Rust side's `annex_b_to_avcc` already produced the AVCC
      // configuration record bytes the spec calls for here.
      decoder.configure({
        codec: codecString,
        codedWidth: width,
        codedHeight: height,
        description,
        optimizeForLatency: true,
      });
    } catch (error) {
      console.warn(
        `[preview-decoder] ${name} configure failed: ${error}`,
      );
      try {
        decoder.close();
      } catch {
        /* configure failure leaves the decoder unusable */
      }
      return;
    }

    this.entries.set(name, {
      decoder,
      codecString,
      width,
      height,
      onFrame,
    });
  }

  decode(
    name: string,
    payload: Uint8Array,
    ptsUs: number,
    isKeyframe: boolean,
  ): void {
    const entry = this.entries.get(name);
    if (!entry) {
      return;
    }
    if (entry.decoder.state !== "configured") {
      return;
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
    }
  }

  close(name: string): void {
    const entry = this.entries.get(name);
    if (!entry) {
      return;
    }
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
 * For H.264 we pull the profile/compat/level bytes out of the AVCC
 * configuration record (the bytes are at offsets 1..4 — see
 * `visualizer/src/protocol.rs::annex_b_to_avcc`). The resulting
 * `avc1.PPCCLL` string is what `VideoDecoder.configure` expects.
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
      if (description.byteLength < 4) {
        return "avc1.42E01F"; // baseline / 3.1 fallback
      }
      const profile = description[1].toString(16).padStart(2, "0");
      const compat = description[2].toString(16).padStart(2, "0");
      const level = description[3].toString(16).padStart(2, "0");
      return `avc1.${profile}${compat}${level}`.toUpperCase();
    }
    case CODEC_ID_H265:
      return "hev1.1.6.L93.B0";
    case CODEC_ID_AV1:
      return "av01.0.04M.08";
    default:
      return null;
  }
}
