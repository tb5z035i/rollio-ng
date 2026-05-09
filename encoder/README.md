# rollio-encoder

**Per configured camera stream:** pull frames from the device driver, optionally **decode/convert** pixel formats, **encode** to disk artifacts during recording, and expose a **stable RGB preview** tap for the UI — all while participating in the global **`control/events`** / **`encoder/video-ready`** / **`encoder/backpressure`** choreography.

---

## Concepts & behaviors

### Why encoders are separate processes

- **Isolation:** a stuck FFmpeg worker should not take down the camera mmap loop.
- **Fan-out:** one camera device may feed **color + depth + IR** → **multiple encoder processes**, each with its own `EncoderRuntimeConfigV2` / `process_id`.
- **Always-on preview:** `rollio` spins encoders during **setup** too so the wizard sees live video even before you record an episode.

### Recording vs idle (conceptual)

When no episode is being written, the process may do **little or no codec work** but still services **preview** and listens for **`control/events`**. On **`RecordingStart`**, it begins muxing according to project format; on stop/keep/discard/shutdown it flushes and coordinates **`VideoReady`** with the assembler.

### Subcommands

#### `probe`

Introspects **this build’s** FFmpeg feature set (CPU encoders, NVENC, VAAPI when compiled in). Use **`--json`** in CI to assert capabilities on a machine image.

#### `run`

- **Requires** encoder slice: **`--config`** or **`--config-inline`** (`EncoderRuntimeConfigV2`).
- Subscribes **`{bus_root}/{channel_type}/frames`** from the device publisher.
- Emits encoded outputs + sidecar metadata per project rules (see source / config types).

**Codec availability** depends on how FFmpeg was linked:

- **Default:** dynamic link against distro **`libavcodec`** / friends → install `-dev` packages (`make deps` / root README).
- **`static-ffmpeg` feature:** bundles codecs (GPL + long first build; CUDA/NVENC toolchain notes in [`Cargo.toml`](Cargo.toml)).

**Depth:** may use **RVL** lossless packing via the `rvl` crate when configured — not every depth path is “just another HEVC stream.”

---

## iceoryx2

**Subscribe:** `{bus_root}/{channel_type}/frames`; `control/events`.

**Publish:** `encoder/video-ready`; `encoder/backpressure`; `{bus_root}/{channel_type}/preview`.

---

## Lifecycle

**Spawned by:** `rollio` per encoder config ([`build_encoder_spec`](../controller/src/runtime_plan.rs)).

**Children:** internal worker thread only.

---

## Built product & dependencies

- **Binary:** `rollio-encoder`.
- **Default:** dynamic FFmpeg dev libraries on the host.
- **Optional:** `static-ffmpeg` Cargo feature.

## See also

- [`rollio-bus`](../rollio-bus/README.md), [`rollio-visualizer`](../visualizer/README.md).
