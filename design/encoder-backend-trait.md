# Encoder Backend Trait Refactor

## Context

The encoder crate today has a `CodecSession` trait, but everything beneath
it is monolithic. `LibavCodecSession::new` hard-codes setup for every
backend (CPU / NVIDIA / VAAPI / RVL); backend dispatch is a closed enum
match spread across `codec.rs`, `media.rs`, and `rollio-types/config.rs`.
The live NVIDIA path runs **MJPG decode + swscale on the CPU** before
handing NV12 to NVENC — about 15% of one core at 1920x1080@30 on an
i9-14900HX, even though NVENC itself is on the GPU. NVDEC names
(`mjpeg_cuvid` / `*_cuvid`) are listed in `media::select_decoder_name`
but only ever used for offline artifact verification. The live encode
path has no CUDA hw_frames context.

Adding a new vendor today (we expect Horizon X5) means touching seven
match arms across four files; there is no place for a third-party
backend to plug in without modifying central tables.

Goals:
1. Move backend logic behind a `dyn ColorEncoderBackend` trait so adding
   a vendor = new module + one registry entry (no enum match arms).
2. Split depth out into its own `dyn DepthEncoderBackend` trait so RVL
   and future depth codecs don't bleed into color backends.
3. Implement the NVIDIA full-HW path: NVDEC for compressed inputs,
   `hwupload_cuda` for raw, `scale_cuda` filter graph for format+resize,
   NVENC consuming CUDA frames directly. Frames never leave VRAM.
4. Implement the VAAPI full-HW path analogously.
5. Implement an H.264-Annex-B passthrough backend now. Cameras that
   emit H.264 in the future will route to it automatically.

---

## Design

### Trait surface — two registries

```rust
// encoder/src/backend/color/mod.rs
pub enum ColorCodec { H264, H265, Av1, Mjpg }     // runtime subset of EncoderCodec
pub trait ColorEncoderBackend: Send + Sync {
    fn id(&self) -> ColorBackendId;
    fn priority(&self) -> u32;
    fn available(&self) -> bool;
    fn supports(&self, codec: ColorCodec, input: PixelFormat) -> bool;
    fn open_session(
        &self,
        params: &ColorSessionParams<'_>,
        first_frame: &OwnedFrame,
    ) -> Result<Box<dyn CodecSession>>;
}
pub struct ColorBackendRegistry { backends: Vec<Arc<dyn ColorEncoderBackend>> }

// encoder/src/backend/depth/mod.rs
pub enum DepthCodec { Rvl }                       // grows with future depth codecs
pub trait DepthEncoderBackend: Send + Sync {
    fn id(&self) -> DepthBackendId;
    fn supports(&self, codec: DepthCodec) -> bool; // input is always Depth16
    fn open_session(
        &self,
        params: &DepthSessionParams<'_>,
        first_frame: &OwnedFrame,
    ) -> Result<Box<dyn CodecSession>>;
}
pub struct DepthBackendRegistry { backends: Vec<Arc<dyn DepthEncoderBackend>> }
```

Both registries are accessed via `OnceLock` global singletons. Both
produce the same downstream `Box<dyn CodecSession>` — preview/recording
runtimes are oblivious to which trait opened the session.

Top-level dispatch (in the runtime layer):

```rust
fn open_for_frame(params, first_frame) -> Result<Box<dyn CodecSession>> {
    if first_frame.header.pixel_format == PixelFormat::Depth16 {
        DepthBackendRegistry::global().open(...)
    } else {
        ColorBackendRegistry::global().open(...)
    }
}
```

`ColorEncoderBackend::Auto` resolves by priority: `Passthrough` (when
input codec matches output codec) → `Nvidia` → `Vaapi` → `Cpu`. The
chosen backend's `supports()` gates final selection so e.g. requesting
`backend = "nvidia"` on a host with no CUDA hardware returns a clear
"NVIDIA backend not available" error instead of silently falling back.

### Color backend implementations

| File (new) | Backend | Decode | Convert / scale | Encode |
|---|---|---|---|---|
| `backend/color/libav_cpu.rs` | `LibavCpuBackend` | libav SW MJPEG / H.264 / H.265 / AV1 decoders when needed | swscale (CPU SIMD) | `libx264` / `libx265` / `libsvtav1` / `mjpeg` |
| `backend/color/libav_nvidia.rs` | `LibavNvidiaBackend` | `mjpeg_cuvid` / `h264_cuvid` / `hevc_cuvid` / `av1_cuvid` for compressed; `hwupload_cuda` for raw | `scale_cuda` filter graph (format + dim, GPU only) | `h264_nvenc` / `hevc_nvenc` / `av1_nvenc` |
| `backend/color/libav_vaapi.rs` | `LibavVaapiBackend` | `*_vaapi` decoders where present; SW MJPEG + `hwupload_vaapi` (no VAAPI MJPEG decoder); `hwupload_vaapi` for raw | `scale_vaapi` filter graph | `h264_vaapi` / `hevc_vaapi` / `av1_vaapi` |
| `backend/color/passthrough.rs` | `PassthroughBackend` | none — input must already be Annex B | none — rescaling rejected | none — header rewrite (PTS / sequence / `source_timestamp_us`) + relay |

The three Libav backends share thin helpers (encoder-context setup,
preset/tune/GOP, GLOBAL_HEADER, color metadata/tb5z035i/workspace) which live in
`backend/color/libav_common.rs`. Each backend module owns only its
hw_device + decode pipeline + filter graph.

### Depth backend implementations

| File (new) | Backend | Notes |
|---|---|---|
| `backend/depth/rvl.rs` | `RvlBackend` | wraps existing `RvlCodecSession`; depth-only; CPU-only |

### CUDA pipeline (NVIDIA backend, the highest-value win)

One CUDA device per session, shared between decoder, filter graph, and
encoder so frames stay in VRAM:

```
                    ┌──────────────────────────────────────────────┐
                    │  CUDA hw_device_ctx (one per session)        │
                    └─┬────────────────┬───────────────────────────┘
                      │                │
   MJPG/H.264 bytes ──►  mjpeg_cuvid   │       ┌────────────────┐
                          h264_cuvid   │  CUDA │  scale_cuda    │  CUDA
                          → CUDA frame ├───────►  W:H:format=nv12├───────► h264_nvenc → Annex B
                      │                │       └────────────────┘
   Raw YUYV/RGB/Gray ──►  hwupload_cuda│
                          → CUDA frame │
                      │                │
                      └────────────────┘
```

- Decoder configured with `hw_device_ctx`. Output `AVFrame.format` is
  `AV_PIX_FMT_CUDA`.
- `scale_cuda` filter graph between decoder and encoder handles both
  format conversion (any input → NV12) and resize. Built once at session
  open; rebuilt on a preview-role `SetSize` (which destroys the session
  anyway, so this is naturally idempotent).
- Encoder configured with `hw_frames_ctx` from `scale_cuda`'s output.
  Direct CUDA frame consumption — no CPU memcpy.
- Raw inputs (YUYV / RGB24 / BGR24 / Gray8) use `hwupload_cuda` as the
  first filter; CPU only DMAs raw bytes to GPU. `scale_cuda` does the
  format conversion. Much cheaper than CPU swscale → NV12 → memcpy.

Expected effect: preview-encoder CPU drops from ~15% of one core to
single digits.

### H.264 passthrough constraints

- New `PixelFormat::H264AnnexB = 6` in `rollio-types/src/messages.rs`.
- New `EncoderBackend::Passthrough` in `rollio-types/src/config.rs`.
- `PassthroughBackend::supports` returns true only for
  `(ColorCodec::H264, PixelFormat::H264AnnexB)` today; extends to
  `H265AnnexB` later when needed.
- **No scaling.** Session open errors if `params.output_width / height`
  differs from `first_frame.header.width / height`.
- The preview runtime's `is_valid_preview_dim` gains a "passthrough-
  active" branch: when the active session was opened on a Passthrough
  backend, the only valid `SetSize` request is one whose dims match the
  source. Anything else is rejected with the existing log line.
- The visualizer publishes `scaling_locked: bool` on the stream_info
  JSON whenever the active backend is `Passthrough`. The UI suppresses
  `set_preview_size` calls while `scaling_locked = true`.
- Sequence number, PTS, and `source_timestamp_us` on
  `EncodedPacketHeader` are rewritten by the passthrough session so its
  output is indistinguishable from a re-encoded stream downstream. NAL
  bytes forward verbatim.
- Codec mismatch (camera publishes H.264, config requests H.265) routes
  to `LibavNvidiaBackend` for transcode (`h264_cuvid` decode + `hevc_nvenc`
  encode). Passthrough does not auto-apply when codecs differ.

### Config / wire format additions

```rust
// rollio-types/src/messages.rs
pub enum PixelFormat {
    Rgb24 = 0, Bgr24 = 1, Yuyv = 2, Mjpeg = 3,
    Depth16 = 4, Gray8 = 5,
    H264AnnexB = 6,   // new
}

// rollio-types/src/config.rs
pub enum EncoderBackend {
    Auto, Cpu, Nvidia, Vaapi,
    Passthrough,      // new
}
```

The unified `EncoderCodec` enum and `EncodedCodecId` wire enum stay
unchanged. Color/Depth typed enums are runtime-only refinements with a
one-line `try_from(EncoderCodec)`. Existing TOML continues to parse.

The `Rvl ↔ Cpu` validation rule in `config.rs` is **deleted**: with the
depth trait split, RVL no longer flows through the color
`EncoderBackend` enum.

---

## Per-channel encoder instances

Confirmed unchanged. The controller already spawns one `rollio-encoder`
process per `(camera_channel, role)` pair via
`ProjectConfig::encoder_runtime_configs_v2()` and
`build_encoder_spec` in `controller/src/runtime_plan.rs`. A RealSense
camera with color + depth + IR channels in collect mode yields six
encoder processes (three recording + three preview). The trait split
doesn't change this; each per-channel process queries the appropriate
registry based on its first-frame `pixel_format`.

---

## Files to modify

**New**:
- `encoder/src/backend/mod.rs` — re-exports + shared `EncoderBackendId` types
- `encoder/src/backend/color/mod.rs` — `ColorEncoderBackend` trait + `ColorBackendRegistry`
- `encoder/src/backend/color/libav_common.rs` — shared libav encoder-context setup
- `encoder/src/backend/color/libav_cpu.rs`
- `encoder/src/backend/color/libav_nvidia.rs`
- `encoder/src/backend/color/libav_vaapi.rs`
- `encoder/src/backend/color/passthrough.rs`
- `encoder/src/backend/depth/mod.rs` — `DepthEncoderBackend` trait + `DepthBackendRegistry`
- `encoder/src/backend/depth/rvl.rs`
- `encoder/src/backend/filter_graph.rs` — `scale_cuda` / `scale_vaapi` filter graph helpers via `ffmpeg-next`'s filter API
- `encoder/src/backend/hw_device.rs` — `cuda_device()` / `vaapi_device()` lazy lookups (built on existing `media::create_hw_device`)

**Modified**:
- `encoder/src/codec.rs` — `open_session()` becomes a thin dispatcher that picks the right registry based on `first_frame.header.pixel_format` and the codec being requested. `LibavCodecSession` body splits: shared encoder-context setup moves into `backend/color/libav_common.rs`; per-backend hw_device / decode-pipeline / filter-graph code moves into the backend modules.
- `encoder/src/media.rs` — keeps low-level helpers (`pixel_format_for_libav`, color-range setup, `compute_pts_us`); drops `select_encoder_name` / `select_decoder_name` tables (moved into backend modules' codec-name resolution).
- `encoder/src/preview_runtime.rs` — `open_session(...)` call unchanged in shape but the dispatcher routes through the registries. SetSize validation gains the passthrough-dim-lock rule.
- `encoder/src/recording_runtime.rs` — same.
- `encoder/src/probe.rs` — iterate `ColorBackendRegistry::global().backends()` and `DepthBackendRegistry::global().backends()` for the capability report.
- `encoder/src/preview.rs` — `decode_or_copy_frame_to_av` becomes a private helper of `LibavCpuBackend`.
- `rollio-types/src/messages.rs` — add `PixelFormat::H264AnnexB`; update `bytes_per_pixel()` (variable-length, returns 0 like Mjpeg).
- `rollio-types/src/config.rs` — add `EncoderBackend::Passthrough`; delete the `Rvl ↔ Cpu` validation; the passthrough-codec-match rule is a runtime check, not config-time.
- `visualizer/src/stream_info.rs` — emit `scaling_locked: bool`.
- `visualizer/src/protocol.rs` / `main.rs` — populate `scaling_locked` from per-camera observed backend (visualizer learns this from the encoder's `EncodedConfig` packets, which already carry `codec` and could carry a backend-id field if needed; alternative is to derive it from input-vs-output codec match on the encoder side and stamp into a header byte).
- `ui/web/src/lib/protocol.ts` — parse `scaling_locked` on `StreamInfoMessage`.
- `ui/web/src/components/CameraGrid.tsx` — skip the `onPreviewSizeChange` callback when `streamInfo.scaling_locked === true`.

---

## Existing helpers to reuse

- `media::create_hw_device` (used today for VAAPI) — generalizes to CUDA with a small refactor.
- `media::create_hw_frames_context` / `media::upload_hw_frame` — reusable across both HW backends.
- `media::resolve_chroma_subsampling`, `resolve_bit_depth`, `scaled_pixel_format`, `encoder_pixel_format` — keep as common policy helpers consumed by all Libav-backed sessions.
- `media::set_swscale_color_range_to_mpeg` — `LibavCpuBackend` only.
- `preview::decode_or_copy_frame_to_av` — moves into `LibavCpuBackend`.
- `codec::RvlCodecSession` — wrapped by `RvlBackend`, no logic change.
- `sink::IpcRecordingSink` / `IpcPreviewPacketSink` / `IpcPreviewJpegSink` — untouched.
- `codec::CodecSession` trait + `EncodedPacketSink` trait — untouched. The new backend traits sit *above* them.

---

## Phase 3 implementation notes (deferred)

A partial attempt at Phase 3 was reverted before commit. Capturing
the dead ends so the next pass doesn't relearn them:

- **`AVFilterGraph::hw_device_ctx` is gone** in this libav version
  (the bindgen output for ffmpeg-sys-next 8.x doesn't expose it).
  The doc above suggested setting it directly on the graph; that's
  outdated. The device has to be attached *per filter context*
  instead — `AVFilterContext::hw_device_ctx` does exist (verified in
  `bindings.rs` at the offset 144 marker).
- The clean libav idiom: parse the graph spec, then either
  - (a) Call `av_buffersrc_parameters_set(buffer_src, &params)` with
    `params.hw_frames_ctx` cloned from the cuvid decoder's
    `hw_frames_ctx`. The device propagates downstream through link
    negotiation, so `scale_cuda` picks it up automatically.
  - (b) For the `hwupload` filter (CPU-input → CUDA), walk
    `graph.filters[]`, find the hwupload context, set its
    `hw_device_ctx` before `avfilter_graph_config()`. The graph's
    parse-then-init step has *already* initialised the filter by the
    time we reach it, so the official ffmpeg examples actually use
    `avfilter_graph_alloc_filter` + `avfilter_init_str` *manually*
    for filters that need a per-context device. That's the more
    verbose but correct path.
- ffmpeg-next's `Graph::parse` calls `avfilter_graph_parse_ptr`,
  which initialises filter contexts as part of parsing. To set
  `hw_device_ctx` on a filter context, we either need to skip parse
  and build the graph manually with `avfilter_graph_alloc_filter` +
  link wiring, or set it via the buffer source parameter dance for
  the cuvid case.
- Building the encoder side then needs: clone the buffersink's
  `hw_frames_ctx` (extract from `(*sink.inputs).hw_frames_ctx`
  *after* `validate`), assign to `(*encoder_ctx).hw_frames_ctx`,
  open NVENC. NVENC reads input format from the hw_frames_ctx's
  `sw_format` (NV12).

Suggested next-session scope: implement MJPG-only path first (cuvid
decoder → buffersrc params dance → scale_cuda → buffersink →
NVENC). That gets the user's main CPU win without the hwupload
complexity. Add raw-input support (`hwupload` for YUYV/RGB/Gray8) in
a follow-up commit using the manual `alloc_filter` + `init_str`
pattern.

Dependency note: enabling `ffmpeg-next/filter` requires the host
system to have `libavfilter-dev` (verified install path on this
machine).

## Phased landing

Each phase is a separate commit. Behaviorally additive — each phase
should leave `make test` green and `rollio collect` producing the same
preview/recording behavior as before, with the noted improvements.

1. **Trait + CPU path.** Define `ColorEncoderBackend` / `DepthEncoderBackend`
   traits + registries. Move existing CPU behavior to `LibavCpuBackend`.
   Move RVL to `RvlBackend`. Route `codec::open_session` through the
   registries. Existing tests pass. No external behavior change. Drop
   the `Rvl ↔ Cpu` validation.
2. **Passthrough + new PixelFormat.** Add `PixelFormat::H264AnnexB`,
   `EncoderBackend::Passthrough`, and `PassthroughBackend`. Synthesized
   H.264 test fixture verifies bytes-out-equals-bytes-in (modulo header
   rewrite). Resize requests rejected with clear error.
3. **NVIDIA full-HW path.** Implement `LibavNvidiaBackend` with CUDA
   hw_device, `*_cuvid` decoders, `scale_cuda` filter graph, NVENC
   consuming CUDA frames. Measure CPU drop. (Largest single improvement.)
4. **VAAPI full-HW path.** Implement `LibavVaapiBackend` analogously.
   Test on Intel iGPU host where available; CI may compile-only.
5. **UI dim-lock surface.** `scaling_locked` on stream_info; UI suppresses
   `set_preview_size` when set. Visualizer log goes quiet under
   passthrough sources.

---

## Verification

- **Phase 1**: `cargo test -p rollio-encoder` passes unchanged. `make build && eval "$(make set-env)" && rollio collect ...` produces identical preview/recording behavior. No change in `nvidia-smi` or `ps -o pcpu`.
- **Phase 2**: New unit test in `backend/color/passthrough.rs` feeds a synthesized H.264 IDR + delta sequence; asserts identical NAL bytes out (header fields rewritten). Resize rejection path covered by another test. `nvenc_repro` example extended (or new sibling example `passthrough_repro`) to exercise the path end-to-end.
- **Phase 3**: `nvidia-smi --query-compute-apps` lists the preview encoder PID. Encoder log shows **no `[swscaler @ ...]` lines** for color paths (CPU swscale absent). `ps -o pcpu` on the preview-encoder process drops to <5% on a 14900HX at 1920x1080@30 MJPG. `Display ms` in the InfoPanel still produces reasonable values (decode latency hasn't grown).
- **Phase 4**: Same as phase 3 on a VAAPI host.
- **Phase 5**: With a passthrough source synthesized via test publisher, the visualizer log shows zero `set_preview_size` commands from the UI.

---

## Open follow-ups (out of scope here)

- Migrating `EncoderRuntimeConfigV2` to per-channel typed configs (color vs depth) is a cleaner future step than today's dual `color_codec` + `depth_codec` fields on one struct. Not required for this refactor.
- Horizon X5 SoC backend: lands as `backend/color/horizon.rs` after this refactor, no changes elsewhere besides the registry default-set and one new `EncoderBackend` enum variant.
- Adopting `iceoryx2` notification primitives instead of timed `node.wait` would further reduce the preview-encoder loop overhead; orthogonal to backend split.
