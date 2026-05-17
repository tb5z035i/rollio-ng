# Rollio

Rollio is a local CLI framework for robotic teleoperation data collection. The
`rollio` controller starts and supervises device drivers, encoders, routing,
preview, UI, episode assembly, storage, and monitoring processes. Processes
communicate through iceoryx2 shared-memory IPC.

The primary runtime flow is:

1. Create or edit a project config.
2. Run `rollio collect --config <config.toml>`.
3. Use the terminal/web UI to start, stop, keep, and discard episodes.
4. Write collected episodes to the configured storage backend.

## Modules

### Rust Workspace

The root `Cargo.toml` workspace contains the main CLI, service binaries, device
drivers, storage/episode writers, and test tools:

| Path | Module |
| --- | --- |
| `controller/` | `rollio`, the orchestration CLI. Supports `setup` and `collect`. |
| `rollio-types/` | Shared config schema, message types, and runtime config builders. |
| `rollio-bus/` | Shared iceoryx2 topic and service naming helpers. |
| `visualizer/` | `rollio-visualizer`, preview stream bridge. |
| `web-gateway/` | `rollio-web-gateway`, HTTP/WebSocket gateway for the web UI. |
| `control-server/` | `rollio-control-server`, UI command/state bridge. |
| `teleop-router/` | `rollio-teleop-router`, leader/follower command routing. |
| `encoder/` | `rollio-encoder`, per-camera record/preview encoding. |
| `encoder-x5/` | `rollio-encoder-x5`, target-specific encoder package. |
| `episode-lerobot/` | `rollio-episode-lerobot`, LeRobot episode assembly. |
| `episode-mcap/` | `rollio-episode-mcap`, MCAP episode assembly. |
| `storage-local/` | `rollio-storage-local`, local filesystem storage worker. |
| `monitor/` | `rollio-monitor`, metrics and threshold monitoring. |
| `robots/pseudo/` | `rollio-device-pseudo`, no-hardware simulated camera/robot driver. |
| `robots/airbot_play_rust/` | `rollio-device-airbot-play`, AIRBOT Play driver. |
| `cameras/v4l2/` | `rollio-device-v4l2`, Linux V4L2 camera driver. |
| `test/test-publisher/` | `rollio-test-publisher`, synthetic IPC publisher. |
| `test/bus-tap/` | `rollio-bus-tap`, IPC inspection/debug tool. |

### Native, UI, And Packaging Modules

| Path | Module |
| --- | --- |
| `cameras/` | C++ camera drivers built with CMake. Currently ships `rollio-device-realsense`. |
| `cpp/common/` | Shared C++ interop headers. |
| `ui/terminal/` | React Ink terminal UI and native ASCII preview addon. |
| `ui/web/` | Web UI bundle served by the gateway. |
| `robots/nero/` | Python Nero driver package, built as a wheel during packaging. |
| `config/` | Example project configs. |
| `debian/` and `build.sh` | Debian package staging and packaging. |
| `third_party/` | Git submodules used by the build, including iceoryx2, ascii-video-renderer, librealsense, FFmpeg, and AIRBOT dependencies. |

## Build

Initialize submodules before building from a fresh checkout:

```bash
git submodule update --init --recursive
```

Install host build dependencies once:

```bash
make deps
```

Rust 1.88 or newer is required. Rust and Node.js are expected to come from your
normal toolchain manager (`rustup`, `nvm`, system packages, etc.); `make deps`
installs the apt-side native dependencies.

Build the full stack for the host architecture:

```bash
make build
make build BUILD_TYPE=release
```

Useful targeted builds:

```bash
make rust-build
make cpp-build
make ui-build
```

Build parameters:

| Variable | Values | Meaning |
| --- | --- | --- |
| `BUILD_TYPE` | `debug` or `release` | Selects Cargo profile, CMake build type, and output subdirectory. Defaults to `debug`. |
| `TARGET_ARCH` | `amd64` or `arm64` | Selects native or cross target. Defaults to the host architecture. |

## Cross-Build For Arm64

### Ubuntu 22.04 Arm64 Package

Use the Docker builder when the target package must run on Ubuntu 22.04 arm64.
This is the supported path for that target because the package is built against
Jammy userspace inside `Dockerfile.cross-jammy`.

Build the image:

```bash
docker build -f Dockerfile.cross-jammy -t rollio-cross-jammy .
```

Register arm64 QEMU/binfmt on the Docker host once. This is required by
target-architecture build probes that execute arm64 binaries during the build.

```bash
docker run --privileged --rm tonistiigi/binfmt --install arm64
```

Build and package from the checked-out repo:

```bash
docker run --rm -it --user "$(id -u):$(id -g)" \
  -v "$PWD":/workspace \
  -w /workspace \
  rollio-cross-jammy
```

The image defaults to `BUILD_TYPE=release` and `TARGET_ARCH=arm64`. It runs
`make build` followed by `make package`, producing
`dist/rollio_<version>_arm64.deb` and the Nero wheel.

### Host Cross-Build

For host-managed arm64 cross-builds, install arm64 build dependencies once on an
amd64 Ubuntu host:

```bash
make deps TARGET_ARCH=arm64
rustup target add aarch64-unknown-linux-gnu
```

Build and package for Linux arm64:

```bash
make build BUILD_TYPE=release TARGET_ARCH=arm64
make package BUILD_TYPE=release TARGET_ARCH=arm64
```

`make test TARGET_ARCH=arm64` is compile-only for Rust/C++ and skips UI/Python
runtime tests when the host cannot execute arm64 binaries.

## Package Products

`make package` is a pure packaging step. Run `make build` first with the same
`BUILD_TYPE` and `TARGET_ARCH`.

```bash
make build BUILD_TYPE=release
make package BUILD_TYPE=release
```

The Nero wheel packaging step requires either `uv` or Python's `build` module.

Package outputs are written under `dist/`:

| Product | Contents |
| --- | --- |
| `rollio_<version>_<arch>.deb` | Runtime package with Rollio binaries, C++ camera drivers, UI bundles, and Debian metadata. |
| `rollio_device_nero-<version>-py3-none-any.whl` | Python wheel for the Nero hardware driver. |

Build outputs used before packaging include:

| Output | Contents |
| --- | --- |
| `target/<profile>/` | Host Rust binaries for debug or release builds. |
| `target/aarch64-unknown-linux-gnu/<profile>/` | Cross-built arm64 Rust binaries. |
| `cameras/build-<arch>-<build_type>/` | C++ camera driver build tree. |
| `ui/terminal/dist/` | Built terminal UI. |
| `ui/web/dist/` | Built web UI. |

## Run

For in-tree development, export the build environment after `make build`:

```bash
eval "$(make set-env)"
rollio setup --sim-pseudo 4 --output config.toml
rollio collect --config config.toml
```

For a release in-tree run, use matching build parameters:

```bash
make build BUILD_TYPE=release
eval "$(make set-env BUILD_TYPE=release)"
rollio collect --config config/config.example.toml
```

After installing the Debian package, the binaries are on `PATH`:

```bash
sudo apt install ./dist/rollio_*_amd64.deb
rollio collect --config /path/to/config.toml
```

On an arm64 target, install the arm64 package produced by the cross-build:

```bash
sudo apt install ./dist/rollio_*_arm64.deb
rollio collect --config /path/to/config.toml
```

## Config Example

[`config/config.example.toml`](config/config.example.toml) is the canonical
annotated config example. It is loaded by tests and is intended to run on any
host because it uses the in-repo `pseudo` driver instead of physical hardware.

Use it for a smoke run:

```bash
make build
eval "$(make set-env)"
rollio collect --config config/config.example.toml
```

The example shows the supported top-level config areas:

| Section | Purpose |
| --- | --- |
| `project_name`, `mode` | Project identity and collection mode. |
| `[episode]` | Dataset format, nominal FPS, and chunking. |
| `[controller]` | Child process shutdown and polling behavior. |
| `[visualizer]` | Preview bridge port. |
| `[[devices]]` and `[[devices.channels]]` | Camera and robot devices, channel profiles, record encoders, and preview encoders. |
| `[[pairings]]` | Leader/follower teleoperation mapping. |
| `[ui]` | Operator keybindings and optional web host/port overrides. |
| `[assembler]` | Episode staging and end-of-stream timeout. |
| `[storage]` | Local or HTTP storage destination. |
| `[monitor]` | Metrics polling and threshold warnings. |

Generate a new config interactively with:

```bash
rollio setup --output config.toml
```

For hardware-free setup testing, inject simulated pseudo devices:

```bash
rollio setup --sim-pseudo 4 --output config.toml
```
