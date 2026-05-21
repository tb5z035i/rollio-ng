# Plan: self-contained AU framing on iceoryx2 + request-keyframe / wait-for-IDR bootstrap

> **Status (paused mid-implementation):** Planning + Task B complete; Task A partially started, then reverted to keep the workspace compiling. Resume at the "Resume from here" section at the bottom.

## Context

Each `EncodedPacketKind::Packet` on the bus is documented to carry one Annex B access unit, but the libav-backed encoder backends set `AV_CODEC_FLAG_GLOBAL_HEADER` (`encoder/src/codec.rs:285`), which strips SPS/PPS into `AVCodecContext.extradata/tb5z035i/workspace`. Keyframe AUs on the wire are `[start][IDR]` only, not `[start][SPS][start][PPS][start][IDR]`. This breaks the "AU is self-contained" invariant, fails MCAP playback in Foxglove Studio (the `CompressedVideo` schema has no extrahome/userb85bbe4d/projects field), and forces a parallel `…/recording-config` topic + retained Config packet just to ship SPS/PPS to late subscribers.

A related latency problem surfaces on IPPP streams: a new subscriber (or `RecordingStart` on a passthrough camera) must wait up to one GOP for the next natural IDR before any frame is decodable.

Outcome: every `Packet` is one self-contained Annex B AU; keyframes carry inline SPS/PPS. There is no `Config` packet, no `…/recording-config` / `…/preview-config` topic, and no `CONFIG_INLINE` flag. Late subscribers (and every fresh recording session) publish `CameraControl::RequestKeyframe` to force an IDR within ~one frame, then gate writing/broadcasting on the first received keyframe to guarantee a clean session start.

## Decisions (user-confirmed)

1. **One topic per channel.** `…/packets` carries `Packet` and `EndOfStream`. The Config packet, the Config topic, and the `CONFIG_INLINE` flag are deleted.
2. **Keyframes inline SPS/PPS.** Mandatory. Doc strings updated to make this normative.
3. **`request_keyframe` end-to-end.** Encoder gets a `CodecSession::request_keyframe()` trait method; passthrough forwards via a new `CameraControl::RequestKeyframe` bus message; cameras producing native H.264 honor it. No pre-record buffer.
4. **Wait-for-IDR gating in every consumer.** Recording assemblers and the visualizer drop incoming Packets until the first `is_keyframe()` Packet arrives on a stream; only then open the file writer / start broadcasting. Defense-in-depth + correct semantics for late subscribers.

## Implementation

### A. Bus protocol

**`rollio-types/src/messages.rs`:**
- Delete `EncodedPacketKind::Config` variant (lines 447-460). Keep `Packet` and `EndOfStream`; renumber if needed.
- Delete `ENCODED_PACKET_FLAG_CONFIG_INLINE` (lines 482-487) — every keyframe inlines SPS/PPS by contract, no flag needed.
- Tighten `EncodedPacketKind::Packet` doc (453-456): "One encoded access unit. For H.264/H.265, MUST be Annex B framing. Keyframes (`ENCODED_PACKET_FLAG_KEYFRAME`) MUST carry SPS+PPS (H.264) / VPS+SPS+PPS (H.265) / sequence-header OBU (AV1) inline ahead of the VCL payload. Delta packets MUST NOT carry parameter sets."
- Add `CameraControl` message:
  ```rust
  #[derive(Debug, Clone, Copy, PartialEq, Eq, ZeroCopySend, Serialize, Deserialize)]
  #[type_name("CameraControl")]
  #[repr(C)]
  pub enum CameraControl { RequestKeyframe = 0 }
  ```
- Extend `PreviewControl` (~line 597) with `RequestKeyframe` variant.

**`rollio-bus/src/lib.rs`:** delete `recording_config_service_name` / `preview_config_service_name` helpers (~lines 136-152). Add `channel_camera_control_service_name(bus_root, channel_type) -> "{bus_root}/{channel_type}/control/camera"` next to the existing `control/mode` / `control/profile` helpers (~line 100). Camera-control topic carries `CameraControl`, best-effort (`history_size=0`).

### B. `CodecSession::request_keyframe()` API

**`encoder/src/codec.rs:71-76`** — add to trait, default no-op:
```rust
fn request_keyframe(&mut self) -> crate::error::Result<()> { Ok(()) }
```
One-shot: cleared after the next emitted keyframe (or for passthrough, after upstream publish succeeds).

Per-backend:
- **Libav** (`LibavCodecSession`): add `force_keyframe: bool`. In `encode` (lines ~501-580), after `source.set_pts(...)` (both direct and scaled paths, ~529 / ~560), if `force_keyframe` is set, call `source.set_kind(ffmpeg::picture::Type::I)` and `unsafe { (*source.as_mut_ptr()).key_frame = 1; }`, then clear.
- **X5** (`encoder-x5/src/backend.rs`): add `force_keyframe: bool` to `HorizonX5Session` (~line 358). **FFI extension required**: add `int force_idr` parameter to `x5_encoder_encode` (signature at backend.rs:59-71 + C shim in `encoder-x5/src/ffi.c`), wired to Horizon BSP `mc_video_frame_t.bIdr`. Pass `self.force_keyframe as i32` at the encode call (~487-501); clear after.
- **Passthrough** (`encoder/src/backend/color/passthrough.rs`): `request_keyframe` calls `sink.request_upstream_keyframe()?`, clears local flag on success.

### C. Encoder backend cleanup (Config removal + frame format)

**Libav `encoder/src/codec.rs`:**
- Delete line 285 `encoder.set_flags(GLOBAL_HEADER)` and the comment block at 281-284. Without GLOBAL_HEADER, libx264 / NVENC / VAAPI emit keyframe AUs as `[start][SPS][start][PPS][start][SEI][start][IDR]` (already validated by the existing lerobot test fixture, `episode-lerobot/src/muxer/ffmpeg_video.rs:177-216`).
- Delete extrahome/userb85bbe4d/projects capture (lines 345-355), the `extrahome/user6fe5a7fc/projects` field, the `config_sent` field, and `ensure_config_sent` (385-411). Delete the `self.ensure_config_sent(sink)?;` call in `encode` (line 503).
- `drain_packets` (460-497) keeps its existing `header.set_keyframe(packet.is_key())` line; everything else stays.

**X5 `encoder-x5/src/backend.rs:514-548`:**
- Delete the `extract_h264_parameter_sets` + `write_config` block (523-535) and the `config_sent` field. The VPU already produces correct AUs; no Config to emit.
- Keep `make_header` and the keyframe flag (line 541-543).

**Passthrough `encoder/src/backend/color/passthrough.rs`:**
- Delete `config_sent` field, `extract_sps_pps`, and the first-keyframe `write_config` path (lines 135-157 + callers ~177-205). The session is pure byte-relay; no special first-keyframe handling.
- Keep `is_keyframe` detection so `KEYFRAME` flag is set correctly on each Packet.

### D. Shared Annex B helper

New module **`encoder/src/annexb.rs`** re-exported from `encoder/src/lib.rs`:
```rust
pub fn split_annex_b_nalus(bytes: &[u8]) -> impl Iterator<Item = &[u8]>;
pub fn extract_h264_parameter_sets(annex_b: &[u8]) -> Option<Vec<u8>>;  // SPS(7) + PPS(8) with 4-byte start codes
pub fn extract_h265_parameter_sets(annex_b: &[u8]) -> Option<Vec<u8>>;  // VPS(32) + SPS(33) + PPS(34)
pub fn extract_av1_sequence_header(temporal_unit: &[u8]) -> Option<Vec<u8>>;  // OBU type 1
pub fn is_h264_keyframe(annex_b: &[u8]) -> bool;  // contains NAL type 5 / 7 / 8
```
Consumed by the lerobot muxer and the visualizer when they need SPS/PPS to seed an MP4 `avcC` box or a WebCodecs description. Replaces the duplicate parsers previously living in passthrough.rs, encoder-x5/backend.rs, and the lerobot test fixture.

### E. `EncodedPacketSink` trait simplification

**`encoder/src/codec.rs:53-57`** — trait collapses to:
```rust
pub trait EncodedPacketSink {
    fn write_packet(&mut self, header: EncodedPacketHeader, payload: &[u8]) -> Result<()>;
    fn write_eos(&mut self, header: EncodedPacketHeader) -> Result<()>;
    fn request_upstream_keyframe(&mut self) -> Result<()> { Ok(()) }
}
```
`write_config` is gone. `request_upstream_keyframe` defaults to no-op; the two IPC sinks (`encoder/src/sink.rs`) build a `CameraControl` publisher at construction time and implement the method by publishing `CameraControl::RequestKeyframe`. Test `MockSink` keeps the default no-op.

### F. Recording + preview runtime wiring

**`encoder/src/recording_runtime.rs`** — in `handle_frame` (~324-350), immediately after the session opens (~344), call `active_session.as_mut().unwrap().request_keyframe()?;`. Libav fresh sessions emit IDR first by construction (no-op), but the call is essential for passthrough so the upstream camera flushes a keyframe.

**`encoder/src/preview_runtime.rs`** — same insertion point after session re-open on `SetSize`. Also handle the new `PreviewControl::RequestKeyframe` variant in the existing drain loop (~131-152): forward to `state.session.as_mut().map(|s| s.request_keyframe())`.

### G. Camera control plumbing

**`robots/pseudo/src/bin/device.rs`** — the only current H.264-native camera. In `run_camera_channel` (~568-641):
- Open a `CameraControl` subscriber on the channel's `control/camera` topic alongside `shutdown_subscriber` (~595).
- Drain the subscriber each tick; on `RequestKeyframe` set a `force_idr: bool` on the camera runtime.
- Pass the flag through to the internal H264Encoder call at `publish_camera_frame` (~528, ultimately into the libx264 wrapper at ~159-194). Use the same `pict_type = I` + `key_frame = 1` recipe as `LibavCodecSession`. Clear after the encode call.

**`cameras/v4l2/src/main.rs`** — no H.264 path today (rejected at line 181-189). **Defer**: when V4L2 H.264 support lands, subscribe to camera-control in `run_camera` (~651), issue `VIDIOC_S_CTRL` with `V4L2_CID_MPEG_VIDEO_FORCE_KEY_FRAME` (cid `0x009909c9`) value `1` on `RequestKeyframe`.

**`cameras/realsense/src/main.cpp`** — no H.264 path. No-op for now.

**Fallback.** Cameras without a control subscriber: publishes go nowhere (iceoryx2 doesn't error). The consumer's wait-for-IDR filter (section H) eventually unblocks on the next natural IDR. Cameras whose hardware can't insert mid-stream IDR: log once at session open, accept best-effort, never error.

### H. Wait-for-IDR filter in consumers

Every consumer of `…/packets` keeps per-stream state `seen_keyframe: bool` (initially false), reset on `EndOfStream`. On each received `Packet`:
- If `!seen_keyframe && !packet.is_keyframe()`: drop silently (best-effort metric: count dropped pre-keyframe packets for observability).
- If `packet.is_keyframe()`: set `seen_keyframe = true`, then process. On this first keyframe, extract SPS/PPS via the shared annexb helper if the consumer needs an extrahome/userb85bbe4d/projects blob (mcap doesn't; lerobot does for `AVCodecParameters.extrahome/userb85bbe4d/projects`).
- If `seen_keyframe`: process normally.

**mcap (`episode-mcap/src/runtime.rs`)** — the existing `RecordingStreamBuffer` (`episode-mcap/src/packets.rs`) currently has a `config` field that gets populated from a Config subscriber. Delete:
- The config subscriber in `create_camera_subscribers` (~669) and the corresponding drain in `drain_camera_packets` (~1077-1098).
- `RecordingStreamBuffer.config: Option<EncodedStreamConfig>` and `EncodedStreamConfig` itself (in packets.rs).
- `observe_config` (packets.rs:73-92).

Add to `RecordingStreamBuffer`: `seen_keyframe: bool`. In the packet-drain path (~537-571), wrap the `writer.write_message(...)` call in `if buffer.seen_keyframe || packet.header.is_keyframe()`. Set `seen_keyframe = true` on the first keyframe. The Foxglove `CompressedVideo` schema's home/user6fe5a7fc/projects field receives the AU verbatim — inline SPS/PPS in keyframes means downstream Foxglove Studio decodes without needing extradata/tb5z035i/workspace.

**lerobot (`episode-lerobot/src/muxer/ffmpeg_video.rs`)** — rewrite `write_stream`:
- Iterate `stream.packets`; advance past any leading non-keyframe packets (count + warn).
- On the first keyframe, call `extract_h264_parameter_sets` (H.264) / `extract_h265_parameter_sets` (H.265) / `extract_av1_sequence_header` (AV1) on the AU payload via the shared helper. The result populates `AVCodecParameters.extradata/tb5z035i/workspace` (current lines 64-75). For MJPEG: empty extrahome/userb85bbe4d/projects, no extraction.
- `output.write_header()` once extradata/tb5z035i/workspace is seeded.
- Then write the first keyframe Packet as the first AVPacket, then continue with all subsequent packets. libavformat's MP4 muxer strips/rewrites the duplicate inline SPS/PPS via the implicit `h264_mp4toannexb` BSF when building `avcC`.
- Codec / dims / time_base come from the first Packet's `EncodedPacketHeader` instead of from a deleted `EncodedStreamConfig`. The header carries `codec`, `width`, `height`, `time_base_num/den`, `pixel_format`, `episode_index` — all the metadata/tb5z035i/workspace that used to live in `EncodedStreamConfig`.
- Delete the `EncodedStreamConfig` struct in `episode-lerobot/src/packets.rs` and its config-subscriber drain.
- The existing muxer test (`h264_annex_b_packets_mux_to_decodable_mp4`, ~122-256) updates: drop the `stream.config = Some(EncodedStreamConfig { extradata/tb5z035i/workspace, … })` setup (~245-253), build the stream from packets only, assert the muxer extracts extrahome/userb85bbe4d/projects internally.

**visualizer (`visualizer/src/main.rs`)** — in `ipc_poll_loop` (~212-344):
- Delete the config subscriber + `cached_configs` HashMap + the SPS/PPS prepend workaround (lines 218-294, 320-333). The workaround was needed because libav-stripped AUs lacked inline parameter sets; with the new contract it's double-writing and must go.
- Add per-stream `seen_keyframe: HashMap<channel_id, bool>` (or a field on the existing per-channel state struct, whichever already exists). On packet receive, drop if `!seen_keyframe && !is_keyframe()`. Broadcast all packets to all current WS clients once `seen_keyframe` is true.
- On `EndOfStream`: clear `seen_keyframe` for that channel.

### I. Visualizer / WebCodecs side

**`ui/web/src/lib/preview-decoder.ts:99-190`** — `configure` is already in Annex B mode (no `description` arg, lines 159-169); inline SPS/PPS in keyframes works as-is. With Config gone, `configure` is now only called once per channel at WS-open (to set the codec string from the channel's known codec ID); it doesn't reconfigure on Config-replay. Minor cleanup: drop the now-impossible "description changed" branch if any survives.

**`visualizer/src/websocket.rs`** + `visualizer/src/main.rs` control wiring:
- Replace the `Fn(u32, u32)` `preview_control_sender` callback (websocket.rs:23, main.rs:123-134) with an enum-typed sender:
  ```rust
  pub enum PreviewControlAction { SetSize { width: u32, height: u32 }, RequestKeyframe }
  pub type PreviewControlSender = Arc<dyn Fn(PreviewControlAction) + Send + Sync>;
  ```
- On WS client connect (websocket.rs ~111-165), publish `PreviewControlAction::RequestKeyframe`. The IPC thread (main.rs ~232-244) maps both variants to the appropriate `PreviewControl::*` publish per channel.
- `RequestKeyframe` ignores the resizable-policy gate.
- Drop the cached-Config replay (~99-108 in websocket.rs).

### J. Tests + verification

**Unit:**
- `encoder/src/annexb.rs` new tests: 3-byte vs 4-byte start code handling, missing SPS, mixed prefixes, `is_h264_keyframe` true on `[IDR]` / `[SPS][PPS][IDR]`, false on `[P]`.  **Already implemented.**
- Extend `codec.rs:942-1010` (libav session ordering test) to assert that the first Packet has `is_keyframe()` and its payload contains NAL types 7 + 8 + 5 (via the shared helper). No more Config packet in the expected sequence.
- Extend `passthrough.rs:482-533` similarly — no Config, first Packet is keyframe with inline SPS/PPS.
- New mock-sink test in `codec.rs`: open `LibavCodecSession` with `gop_size=120`, call `request_keyframe()` on frame 30, assert frame 30's Packet has `is_keyframe()` set (without the call, IDR would be every 4s @ 30fps).
- Rewrite `episode-lerobot/src/muxer/ffmpeg_video.rs:122-256` test: build a stream with no config, only packets; assert muxed MP4 decodes.
- Add a lerobot test for "stream starts with a delta packet": muxer drops the delta, starts at the next keyframe, MP4 still decodes from frame 1.

**Encoder smoke binary.** `encoder/examples/smoke_inline_params.rs` (per-backend, gated on host availability): encode 30 frames through each backend, ffprobe-assert NAL types 7+8+5 on every keyframe Packet's payload.

**End-to-end:**
- `cargo run -p rollio-controller -- record` against `config/pseudo-h264-yuyv-smoke.toml`; `ffplay` the MP4; open the MCAP in Foxglove Studio. Both decode from frame 1.
- X5 device smoke: cross-build, scp to `sunrise@192.168.25.1` (BSP paths in `~/.claude/memory/reference_x5_bsp.md`), run the X5 smoke config, ffprobe-verify NAL types 7+8+5 on the first AU.

**Force-keyframe / wait-for-IDR E2E:**
- Libav with `keyint=120` (4s GOP @ 30fps). Start a recording. Assert the very first Packet observed on `…/packets` has `is_keyframe()` and contains NAL type 5. Reverting just the `request_keyframe()` call after `open_session` should break this — that's the regression test for the IPPP latency fix.
- Passthrough: tiny harness subscribes to `…/control/camera`, asserts `CameraControl::RequestKeyframe` is observed within ~10 ms of `RecordingStart` and the pseudo camera's next published AU is a keyframe. Separately, force a recording-start mid-GOP by suppressing the request: assert mcap/lerobot drops leading delta packets and the produced file starts at the first natural keyframe (defensive correctness).
- Visualizer: open `ui/web` against a running pseudo-h264 channel, refresh the page; the WebCodecs preview should render within ~one frame (no GOP wait).

## Critical files

- `rollio-types/src/messages.rs` — delete Config variant + CONFIG_INLINE flag; add `CameraControl`, `PreviewControl::RequestKeyframe`; tighten Packet doc.
- `rollio-bus/src/lib.rs` — delete config-topic helpers; add `channel_camera_control_service_name`.
- `encoder/src/codec.rs` — drop GLOBAL_HEADER + Config logic; add `request_keyframe` to trait + libav impl; trim sink trait to `write_packet` + `write_eos` + `request_upstream_keyframe`.
- `encoder/src/annexb.rs` (already exists) — shared Annex B / NAL helpers.
- `encoder/src/sink.rs` — IPC sinks build camera-control publisher; drop `write_config`.
- `encoder/src/backend/color/passthrough.rs` — strip Config logic; `request_keyframe` forwards via sink.
- `encoder-x5/src/backend.rs` + `encoder-x5/src/ffi.c` — `bIdr` FFI parameter; drop Config emission.
- `encoder/src/recording_runtime.rs` — call `request_keyframe()` after `open_session()`.
- `encoder/src/preview_runtime.rs` — same; handle `PreviewControl::RequestKeyframe`.
- `robots/pseudo/src/bin/device.rs` — `CameraControl` subscriber + force-IDR plumbing into the H264Encoder.
- `episode-mcap/src/runtime.rs` + `episode-mcap/src/packets.rs` — delete Config subscriber + `EncodedStreamConfig`; add wait-for-IDR filter.
- `episode-lerobot/src/muxer/ffmpeg_video.rs` + `episode-lerobot/src/packets.rs` — same; extract SPS/PPS from first keyframe AU; codec metadata/tb5z035i/workspace from Packet header.
- `visualizer/src/main.rs` + `visualizer/src/websocket.rs` — delete config subscriber, cached_configs, prepend workaround; add wait-for-IDR filter; publish `RequestKeyframe` on WS connect.
- `ui/web/src/lib/preview-decoder.ts` — minor cleanup; Annex B mode unchanged.

---

## Resume from here

### What's committed-to-workspace (compiles cleanly)

- **`encoder/src/annexb.rs`** — new shared helper module, 14 unit tests passing (`cargo test -p rollio-encoder --lib annexb::`). Provides `split_annex_b_nalus`, `extract_h264_parameter_sets`, `extract_h265_parameter_sets`, `extract_av1_sequence_header`, `is_h264_keyframe`. Replaces the duplicate parsers in passthrough.rs (lines 135-157, 319-350), encoder-x5/backend.rs (`extract_h264_parameter_sets`), and the lerobot test fixture (`episode-lerobot/src/muxer/ffmpeg_video.rs:323-384`).
- **`encoder/src/lib.rs`** — `pub mod annexb;` added at the top.
- **`rollio-types/src/messages.rs`** — `PreviewControl` extended with `RequestKeyframe` variant (~line 598); new `CameraControl` enum with `RequestKeyframe` variant (~line 622). `EncodedPacketKind::Config` and `ENCODED_PACKET_FLAG_CONFIG_INLINE` are **still present** (deletion deferred until downstream callers are migrated).
- **`encoder/src/preview_runtime.rs`** — the `let PreviewControl::SetSize { … } = *sample.payload();` at line 136 was converted to a `match` that ignores `RequestKeyframe` for now (real handling lands with section F). Single minimal compile-fix; no behavior change.

### What's NOT done

Everything else in sections A (deletions), C, E, F, G, H, I, J. The implementation roadmap below mirrors the original task list 1-1.

### Recommended next-session task order

1. **A (deletions) + C + E** in one pass — delete `EncodedPacketKind::Config`, `ENCODED_PACKET_FLAG_CONFIG_INLINE`, `has_inline_config()` / `set_inline_config()`, trim `EncodedPacketSink` to `write_packet/write_eos/request_upstream_keyframe`, drop `write_config` calls and the entire Config-emission path in libav + X5 + passthrough. This is the breaking cascade; doing it together avoids two compile waves. Also delete the `recording_config_service_name` and `preview_config_service_name` helpers in `rollio-bus/src/lib.rs`.
2. **D + sink IPC publisher** — add `CodecSession::request_keyframe()` (libav: pict_type=I + key_frame=1 on next AVFrame; X5: extend FFI with `bIdr`; passthrough: call `sink.request_upstream_keyframe()`). Build `CameraControl` publisher inside `IpcRecordingSink` / `IpcPreviewPacketSink`.
3. **F** — wire `request_keyframe()` into recording_runtime + preview_runtime after `open_session()`. Handle `PreviewControl::RequestKeyframe` in preview_runtime drain.
4. **G** — pseudo camera subscriber + force-IDR plumbing.
5. **H (mcap, lerobot, visualizer)** — wait-for-IDR filter + Config subscriber removal + lerobot muxer rewrite to extract SPS/PPS from first keyframe via `annexb::extract_h264_parameter_sets`.
6. **I** — visualizer Rust + ui/web TS cleanup.
7. **J** — tests, smoke binary, E2E including X5 device.

### Edit-tool gotcha

Multi-line `Edit` calls against `rollio-types/src/messages.rs` were unreliable for old_strings containing certain combinations of `/` + the literal `extrahome/userb85bbe4d/projects` substring — the matcher silently fails. Workaround: use shorter single-line ASCII-only old_strings (no slash + path-like keyword combinations on the same line), or fall back to `Write` for a full-file rewrite. Single-line edits with simple anchors always worked.

### Verification command sweep

After completing all sections:
```bash
cargo build --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
# Then E2E smoke (see section J).
```
