# rollio-encoder

**Per-stream video and depth encoding** for camera channels. Subscribes to iceoryx2 frame services, encodes with **FFmpeg/libav** (`ffmpeg-next`), and participates in the recording control plane via `rollio-bus` service names.

## CLI

- **`rollio-encoder probe`** — Lists available encode/decode capabilities (CPU, NVENC, VAAPI when present). Use **`--json`** for machine-readable output.
- **`rollio-encoder run`** — Runtime mode with **`--config`** or **`--config-inline`** (encoder section of the project config).

Supported color codecs include **H.264, H.265, AV1** (availability depends on the linked FFmpeg build). **16-bit depth** can be handled via the **RVL** lossless path (`rvl` crate), not generic “FFV1 for all depth” unless configured that way.

## Features

- **`static-ffmpeg`** (optional) — Bundles a static FFmpeg build for deployments without system `libav*`. Heavy build; see `Cargo.toml` for prerequisites. Default builds **dynamic-link** against system FFmpeg dev packages.

## See also

- [`rollio-bus`](../rollio-bus/README.md) — service names and buffers.
- Root [`README.md`](../README.md) — encoder validation commands and apt deps for `libavcodec-dev`, etc.
