# Rollio

CLI framework for hardware discovery, setup, and data collection in robotic
teleoperation workflows.  Records camera streams and robot joint data into
structured LeRobot v2.1/v3.0 episode datasets.

See `design/` for architecture docs and sprint plans.

## Prerequisites

### Build dependencies

| Tool       | Minimum version | Purpose              |
|------------|-----------------|----------------------|
| Rust       | 1.85+           | Cargo workspace      |
| CMake      | 3.22+           | C++ modules          |
| g++ / clang| C++17 support   | C++ modules          |
| NASM       | recent          | `libjpeg-turbo` SIMD build used by `turbojpeg` |
| pkg-config | recent          | native `libav*` discovery for `rollio-encoder` |
| Node.js    | 18+             | UI (TypeScript/Ink)  |
| npm        | 9+              | UI dependency mgmt   |

### Optional (development)

| Tool          | Purpose                          |
|---------------|----------------------------------|
| clang-format  | C++ auto-formatting              |
| clang-tidy    | C++ static analysis              |
| ruff          | Python linting & formatting      |
| pre-commit    | Git hook runner                  |

## Getting started

```bash
# Clone with submodules
git clone --recursive <repo-url>
cd rollio-ng

# Debian/Ubuntu build tools
# `clang`/`libclang-dev`/`llvm-dev` are required for bindgen-based builds
# such as `iceoryx2` 0.8.1+ and the AIRBOT driver stack.
sudo apt-get update
sudo apt-get install -y \
  build-essential \
  cmake \
  nasm \
  pkg-config \
  clang \
  libclang-dev \
  llvm-dev \
  libavcodec-dev \
  libavformat-dev \
  libavutil-dev \
  libswscale-dev

# Rust
cargo build --workspace
cargo test --workspace
# Camera drivers (C++)
cmake -B cameras/build -S cameras -DCMAKE_CXX_COMPILER=g++
cmake --build cameras/build
ctest --test-dir cameras/build --output-on-failure

# UI
cd ui/terminal && npm install && npm run build && cd ../..

# AIRBOT wrapper + transport validation
cargo test -p rollio-device-airbot-play
cargo test --offline --manifest-path third_party/airbot-play-rust/Cargo.toml transport::iceoryx2::tests --lib
cargo run --manifest-path third_party/airbot-play-rust/Cargo.toml --bin airbot-play-iceoryx2 -- --interface can0

# AIRBOT hardware smoke (requires a configured CAN-connected arm)
cargo run -p rollio -- collect --config config/config.hardware.example.toml
```

If `cargo build --workspace` or `make` fails while compiling `turbojpeg-sys`
with `No CMAKE_ASM_NASM_COMPILER could be found`, install `nasm` and retry.

If an `iceoryx2` or `airbot-play-rust` build fails during bindgen with errors
like `fatal error: 'stddef.h' file not found`, install:

```bash
sudo apt-get update
sudo apt-get install -y clang libclang-dev llvm-dev
```

If `rollio-encoder` fails to compile with missing `libav*` pkg-config metadata, install the
development libraries instead of the full `ffmpeg` package:

```bash
sudo apt-get update
sudo apt-get install -y pkg-config libavcodec-dev libavformat-dev libavutil-dev libswscale-dev
```

## Pre-commit hooks (optional)

```bash
pip install pre-commit
pre-commit install
```

This enables automatic checks before each commit:
Rust formatting/linting, C++ formatting, Python linting/formatting,
and general file hygiene (trailing whitespace, TOML/YAML syntax, etc.).

## Sprint 2 Validation

```bash
# Full validation loop
make test

# Controller-managed pseudo-device smoke
make smoke-pseudo
```

`make smoke-pseudo` launches the Sprint 2 stack through the new `rollio collect`
entrypoint using `config/config.example.toml`. The expected checkpoint is that
the pseudo camera previews and robot status appear in the TUI, and the stack
shuts down cleanly when you press `Ctrl+C`.

## Sprint 4 Encoder Validation

`rollio-encoder` now supports:

- human-friendly `probe`, with structured output available from `probe --json`
- CPU-backed `h264`, `h265`, and `av1` video encoding/decoding through native `libav`
- `rvl` as the one-channel lossless `depth16` backend via `third_party/rvl-rust`
- bounded queue backpressure reporting on iceoryx2

Focused validation commands:

```bash
# Human-friendly capability summary
cargo run -p rollio-encoder -- probe

# Machine-readable capability report
cargo run -p rollio-encoder -- probe --json

# Encoder tests, including report-only throughput/resource output
cargo test -p rollio-encoder -- --nocapture

# Optional hardware-specific round-trip tests
cargo test -p rollio-encoder nvidia_video_codecs_round_trip_when_available -- --ignored --nocapture
cargo test -p rollio-encoder vaapi_video_codecs_round_trip_when_available -- --ignored --nocapture
```

The encoder test suite reports benchmark-style metrics such as elapsed time,
compression ratio, and resident memory. GPU/video-engine metrics remain
best-effort and host-dependent; the CPU path is always validated, while the
NVIDIA and VAAPI round-trip tests are capability-gated and ignored by default.

## Project layout

```
rollio-bus/           Shared iceoryx2 topic/service naming helpers (Rust lib)
rollio-types/         Shared iceoryx2 message types + controller config surface
controller/           CLI entry point and process orchestrator (Rust)
visualizer/           iceoryx2 <-> WebSocket bridge (Rust)
teleop-router/        Leader-follower command forwarding (Rust)
encoder/              Video encoding per camera stream (Rust)
episode-assembler/    Assembles episodes from video + state data (Rust)
storage/              Local and remote storage backends (Rust)
monitor/              Health/performance metrics evaluator (Rust)
test/test-publisher/    Synthetic iceoryx2 data publisher (Rust)
cameras/              In-repo camera drivers + camera-driver extension docs
robots/               In-repo robot drivers + robot-driver extension docs
cpp/                  Shared C++ interop headers and legacy wrapper entrypoint
ui/terminal/          Terminal UI built with React/Ink (TypeScript)
config/               Example configuration files
design/               Architecture docs and sprint plans
third_party/          Submodules: iceoryx2, ascii-video-renderer
```
