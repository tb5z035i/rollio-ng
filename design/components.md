# Component Architecture

## Overview

Rollio uses a **multi-process architecture** where each functional component
runs as an independent executable. Processes communicate via **iceoryx2**
(zero-copy shared-memory IPC) for both high-bandwidth data (camera frames,
robot states) and control events (episode lifecycle, shutdown signals).

The end user interacts with a **single command in a single terminal** (e.g.
`rollio collect -c config.toml`). The Controller process is the entry point;
it spawns all other modules as child processes, with the **UI process
inheriting the Controller's terminal** (stdin/stdout/stderr) so the TUI
appears directly in the user's terminal. All other child processes have their
stdio redirected to log files — they never write to the user's terminal.

The UI communicates with the rest of the system via **WebSocket** through a
bridge process (the Visualizer). This is the only non-iceoryx2 communication
path.

Each module is an **independent subproject** with its own build system,
producing a standalone executable. Modules do not link against or depend on
each other at build time — they communicate only through iceoryx2 topics and
events at runtime.

**Argument passing convention**: probe subcommands (`probe`, `validate`,
`capabilities`) use simple CLI arguments. Runtime subcommands (`run` and
equivalents — any mode that interacts with iceoryx2) accept their full
configuration via **either** a `--config <path>` flag pointing to a config
file, **or** a `--config-inline <string>` flag accepting serialized TOML
inline. This allows the Controller to extract the relevant section of the
master TOML config and pass it directly when spawning a child process —
avoiding fragile long argument lists for complex device parameters.

---

## Data Flow

### Preview Mode (setup preview / collect idle)

```
Camera ──[iceoryx2: raw frames]──► Visualizer ──[WebSocket]──► UI

Robot  ──[iceoryx2: joint states]──► Visualizer ──[WebSocket]──► UI

Leader Robot ──[iceoryx2: state]──► Teleop Router ──[iceoryx2: cmd]──► Follower Robot
```

All devices are live. Cameras stream, robots are in their configured mode
(free-drive or command-following), teleop pairs are active. Nothing is
recorded.

### Recording Mode (active episode)

```
Camera ──[iceoryx2: raw frames]──┬──► Encoder (one per stream) ──► video file on disk
                                 └──► Visualizer ──[WebSocket]──► UI

All Robots ──[iceoryx2: joint states]──┬──► Episode Assembler (buffers observations)
(leaders + followers)                  └──► Visualizer ──[WebSocket]──► UI

Leader Robot ──[iceoryx2: state]──► Teleop Router ──[iceoryx2: cmd]──┬──► Follower Robot
                                                                     └──► Episode Assembler (buffers actions)
```

iceoryx2 pub-sub fan-out delivers frames to both Encoder and Visualizer
simultaneously (zero-copy). The Episode Assembler subscribes to robot state
topics and buffers timestamped data in memory for the duration of the episode.

### Episode Finalization (after user chooses "keep")

```
Controller ──[iceoryx2: episode_stop]──► Encoder(s): flush & close video file(s)
                                        ──► Episode Assembler: freeze buffered data

Encoder(s) ──[iceoryx2: video_ready + file path]──► Episode Assembler

Episode Assembler:
  1. Writes tabular data (robot states, actions) → Parquet
  2. Writes metadata (info.json or equivalent)
  3. Organizes directory layout per chosen format (LeRobot 2.1 / 3.0 / mcap)
  4. Notifies Storage with completed episode path

Storage ──► writes to backend (local / HTTP)
        ──[iceoryx2: episode_stored]──► Controller
```

Steps 1–4 happen in the background. The user can start the next episode
immediately after choosing "keep".

### Episode Discard

```
Controller ──[iceoryx2: episode_discard]──► Encoder(s): discard partial video
                                          ──► Episode Assembler: discard buffered data
```

### Backpressure Behavior

Encoder and Storage each maintain internal queues. When a queue fills:

- **Queue rejects newer data** (oldest-in-queue has priority).
- **If between episodes**: the Controller blocks the user from starting the
  next episode until the queue drains. The UI shows a warning.
- **If during recording**: the Controller warns the user and **discards the
  current episode immediately**. With properly sized queues this should be
  rare.

---

## Components

### 1. Image Sensors (C++)

Cameras (USB webcams, RealSense, and future sensor types). Each supported
sensor type is a separate driver implementation behind a common interface.

**Executable modes** (subcommands of a single binary per sensor type):

- `probe`: discover all connected instances of this sensor type, output a list
  of identifiers. Maximum latency: 200ms.
- `validate <id>`: confirm a specific device is present and reachable.
- `capabilities <id>`: report supported (width, height, fps) combinations,
  pixel formats (MJPEG, YUYV, etc.), available streams (color, depth,
  infrared), and device metadata (serial number, firmware version).
- `run --config <path>` or `run --config-inline <toml>`:
  open the device with the given parameters, capture frames, and publish raw
  frames to an iceoryx2 topic. Listen for iceoryx2 shutdown events to exit
  gracefully. Configuration specifies the device ID, stream, resolution,
  FPS, pixel format, and iceoryx2 topic name.

A single physical sensor (e.g. RealSense D435i) can produce multiple streams
(color + depth + infrared). Each stream is published to a **separate iceoryx2
topic**. The Controller spawns one `run` invocation per active stream; if
multiple streams come from the same device, multiple invocations of the same
binary run concurrently (the driver must support concurrent access, or
multiplex internally).

This module is IO-intensive. Async I/O should be used to minimize CPU usage
while maintaining low latency.

**Initial sensor types**: V4L2 USB camera, Intel RealSense, Pseudo camera
(see §Mock Devices).

### 2. Robots (Python / C++)

Robot arms and end-effectors. Language depends on vendor SDK availability — use
C++ when the vendor provides a C/C++ SDK, Python when the vendor provides a
Python SDK.

**Executable modes** (subcommands of a single binary per robot type):

- `probe`: discover all connected instances, output identifiers. Maximum
  latency: 200ms.
- `validate <id>`: confirm device is present and reachable.
- `capabilities <id>`: report device serial number, number of joints,
  supported control modes, connected sub-devices (e.g. end-effector type),
  and other type-specific metadata.
- `run --config <path>` or `run --config-inline <toml>`:
  open the device, enter the specified control mode, and:
  Configuration specifies the device ID, control mode, iceoryx2 topic names,
  and any driver-specific parameters.
  1. **Publish state** (joint positions, velocities, efforts, and optionally
     end-effector pose via FK) to iceoryx2 topics continuously, with minimal
     latency.
  2. **Subscribe to commands** on an iceoryx2 topic (for command-following
     mode): accept joint-space commands, or Cartesian commands if the driver
     supports IK internally.
  3. **Listen for mode-switch events** on iceoryx2 to toggle between
     free-drive and command-following at runtime.
  4. **Listen for shutdown events** to exit gracefully.

**Control modes** (initial design, planning mode deferred):

- **Free-drive**: gravity-compensated, human can physically move the arm. The
  driver still publishes state. No commands are accepted.
- **Command-following**: the arm tracks commands received on its iceoryx2
  command topic at high frequency (≥ 10Hz for joint commands, higher if the
  hardware supports it). The internal control loop (which may run at 200Hz–
  1kHz depending on the robot) is handled entirely within the driver process
  and is not exposed to other modules.

**FK/IK**: if a robot type has a kinematic model (URDF, DH parameters), the
driver computes FK internally and publishes end-effector pose alongside joint
states. IK for Cartesian command-following is also internal to the driver. The
Teleop Router (§3) does not perform any kinematic computation.

**Shared physical interfaces**: multiple logical robots may share a physical
bus (e.g. AIRBOT Play arm + G2 gripper on the same CAN interface). Each runs
as a **separate process**. Bus arbitration is handled at the OS level
(SocketCAN for CAN buses). The modules do not depend on or coordinate with
each other.

**Initial robot types**: AIRBOT Play, AIRBOT G2 (gripper), AIRBOT E2
(demonstrator), Pseudo robot (see §Mock Devices).

### 3. Teleop Router (Rust)

A lightweight process that implements leader→follower command forwarding. One
Teleop Router process per teleop pair.

**Responsibilities**:

- Subscribe to the leader robot's state topic on iceoryx2.
- Apply a mapping to produce commands for the follower.
- Publish commands to the follower robot's command topic on iceoryx2.

**Mapping strategies**:

- **Direct joint mapping**: remap leader joint indices to follower joint
  indices with optional scaling. This is the default when leader and follower
  are the same robot type.
- **Cartesian forwarding**: subscribe to the leader's end-effector pose (which
  the leader's driver computes via FK) and publish it as a Cartesian command
  to the follower (whose driver applies IK internally). Requires both leader
  and follower to support FK/IK respectively.

The Teleop Router itself contains **no kinematic knowledge**. It is a pure
message transformer — subscribe, remap/forward, publish.

**Configuration** (passed at launch by the Controller): leader topic, follower
topic, mapping strategy, joint index map, scaling factors.

### 4. Encoder (Rust)

Video encoding for camera streams. **One Encoder process per camera stream**
(e.g. 4 cameras = 4 Encoder processes). This provides natural parallelism,
fault isolation (one encoder crash affects only one stream), and clean mapping
to hardware encoder sessions (NVENC, VAAPI).

**Executable modes**:

- `probe`: report available codecs and hardware acceleration support (NVENC,
  VAAPI, software). Output as structured JSON.
- `run --config <path>` or `run --config-inline <toml>`:
  subscribe to raw frames on iceoryx2, encode, and write to a video file.
  Configuration specifies the iceoryx2 topic, codec, output path, and
  queue parameters.
  The Encoder runs continuously but only writes when it receives a
  `recording_start` event. On `recording_stop`, it flushes the pipeline and
  closes the file, then publishes a `video_ready` event with the output path.

**Supported codecs**:

- **H.265 / H.264 / AV1**: for RGB color frames. Hardware acceleration
  preferred (NVENC, VAAPI), software fallback available.
- **FFV1**: lossless codec for depth frames (16-bit single-channel).
- **MJPEG**: for RGB frames, lower priority fallback.

**Internal queue**: configurable size. When the queue is full, it rejects
incoming frames and publishes a `backpressure` event to the Controller via
iceoryx2.

### 5. Episode Assembler (Rust)

Assembles complete episodes from encoded video files and buffered robot state
data into a structured dataset format. Supports **multiple format backends**.

**Responsibilities**:

1. Subscribe to **all robot state topics** on iceoryx2 during recording
   (both leaders and followers) and buffer timestamped data in memory.
   These become the `observation.state.*` columns in the output.
2. Subscribe to **follower command topics** on iceoryx2 during recording.
   These are the same topics the Teleop Router publishes to and the follower
   robot driver reads from. The captured commands become the `action` column
   in the output.
3. On `recording_stop`, freeze both buffers.
4. Wait for `video_ready` events from all Encoder processes for the episode.
5. Resample robot state and action data to the nominal FPS (aligning
   timestamps).
6. Write tabular data (Parquet).
7. Write metadata (format-specific: `info.json` for LeRobot, headers for
   mcap, etc.).
8. Embed the collection configuration in the episode metadata (for replay
   support, see user story §3.3).
9. Organize the directory layout per the chosen format.
10. Notify Storage with the completed episode.

**Supported format backends**:

- **LeRobot v2.1** (default): `data/chunk-XXX/episode_NNNNNN.parquet`,
  `videos/chunk-XXX/<camera>/episode_NNNNNN.<ext>`, `meta/info.json`.
- **LeRobot v3.0**: sharded layout as specified in the LeRobot v3.0 format
  spec.
- **mcap**: for interoperability with other robotics tools.

On `episode_discard`, the Assembler drops all buffered data for the current
episode without writing anything.

### 6. Storage (Rust)

Writes completed episodes to storage backends. Decoupled from episode format —
it receives a directory (or file set) and persists it.

**Supported backends** (initial):

- **Local filesystem**: move/copy the episode directory to a configured output
  path.
- **HTTP upload**: POST to a configured endpoint. A companion **simple HTTP
  receive server** is provided for the receiving end.

**Future backends** (deferred):

- **S3-compatible remote storage**: upload via the S3 API.

**Internal queue**: configurable size. When full, rejects new episodes and
publishes a `backpressure` event to the Controller. For HTTP upload, endpoint
availability is tested during the setup phase to catch connectivity issues
early.

### 7. Visualizer (Rust)

Bridge between the iceoryx2 data plane and the UI process. The Visualizer is
the **sole gateway** between the iceoryx2 world and the WebSocket/UI world.

**Responsibilities**:

- Subscribe to camera frame topics on iceoryx2, downsample/compress to JPEG
  for preview, and serve to the UI via **WebSocket binary protocol** (matching
  the protocol validated in the react-tui prototype: type-tagged binary frames
  supporting JPEG, H.264 chunks, or raw RGB24).
- Subscribe to robot state topics on iceoryx2 and forward to the UI via
  WebSocket (JSON messages).
- Subscribe to Controller status topics (recording state, episode count,
  warnings) and forward to the UI.
- **Receive control commands from the UI** (episode start/stop/keep/discard,
  device parameter changes during setup) via WebSocket and **publish them to
  iceoryx2** for the Controller to process.

A single multiplexed WebSocket connection carries both preview data
(high-bandwidth, binary) and control messages (low-bandwidth, JSON), using the
binary protocol's type field for discrimination.

**Robustness**: the Visualizer runs even without connected UI clients or
active data sources. It begins serving as soon as either side becomes
available.

### 8. UI (TypeScript / React / Ink)

Terminal UI built with React and **Ink** (React renderer for the terminal).
Renders camera previews as ANSI 256-color half-block art, robot states as
animated bar/gauge components, and interactive forms for the setup wizard.

**Rendering approach**: the react-tui prototype demonstrates that ANSI
half-block rendering (Unicode U+2584, 256-color palette) achieves acceptable
preview quality with frame-rate latency — which matters more than pixel
fidelity for a "make sure it's working" preview.

**Responsibilities**:

- Connect to the Visualizer via WebSocket.
- Render camera preview frames (decoded from JPEG/RGB24 → ANSI half-blocks).
- Render robot state readouts (bar charts, numeric displays).
- Capture keyboard input for episode control (start, stop, keep, discard) and
  send commands to the Visualizer via WebSocket.
- During setup: render the interactive wizard (device selection, parameter
  configuration, pairing strategy) and the preview page.
- During collect: render the same preview layout plus recording status
  indicators (episode count, recording state, backpressure warnings).
- Adapt layout to terminal size (responsive).

**Robustness**: the UI renders a meaningful interface even before data arrives
from the Visualizer. Components show placeholder states and update live as
data becomes available.

Packaged as a single executable (via `npx` or a bundled Node.js binary) with
CLI arguments, so the Controller can launch it like any other module.

**Terminal ownership**: the UI is the only child process that inherits the
Controller's terminal (stdin/stdout/stderr). It takes over the terminal in
raw mode for full-screen Ink rendering and keyboard capture. When the UI
process exits (user quits), the Controller detects this and initiates graceful
shutdown of all remaining modules.

### 9. Controller (Rust)

The central orchestrator. It owns no data — it manages lifecycles and routes
events.

**Responsibilities**:

- Parse the configuration file (TOML).
- Validate the configuration: syntax, device references, pairing consistency.
- Launch all other modules as child processes with appropriate arguments.
  The **UI process** inherits the Controller's terminal (stdin/stdout/stderr);
  **all other children** have their stdio redirected to per-process log files
  for debugging without interfering with the TUI.
- Monitor child process health (restart on crash if appropriate, or report
  failure). If the UI process exits, initiate graceful shutdown of the entire
  system.
- Manage the **episode state machine**:

  ```
  Idle ──[start]──► Recording ──[stop]──► Pending ──[keep]──► Idle
                                                   ──[discard]──► Idle
  ```

  Transitions are triggered by control events received from the Visualizer
  (originating from user keyboard input in the UI). The Controller publishes
  corresponding iceoryx2 events to Encoder, Episode Assembler, etc.

- Track backpressure state: if encoder or storage queues are full, block
  episode start or trigger episode discard (see §Backpressure Behavior).
- Manage graceful shutdown: send shutdown events to all modules, wait for
  acknowledgment, then exit.
- During setup: coordinate the probe/validate/capabilities cycle, relay
  results to the UI.

The Controller is the **`rollio` CLI entry point** itself. The top-level
`rollio` binary dispatches to subcommands (`setup`, `collect`, `replay`), each
of which orchestrates a different subset of modules.

**Driver discovery**: device driver executables are expected to be available
on `PATH` using a naming convention (e.g. `rollio-camera-realsense`,
`rollio-camera-pseudo`, `rollio-robot-airbot-play`). During setup, the
Controller attempts to invoke each known driver name with the `probe`
subcommand. If a driver binary is not found, the Controller emits a warning
(e.g. "RealSense driver not installed — skipping") and continues with the
remaining drivers. No manifest file or special installation step is required
beyond placing the binaries on `PATH`.

---

### 10. Monitor (Rust)

A standalone process that aggregates health and performance metrics from all
running modules and evaluates them against configurable thresholds.

**Process identity**: every monitored process is assigned a **unique ID** in
the master config file (e.g. `camera.top`, `encoder.camera_top`,
`robot.leader_left`). When a module publishes metrics, it **must** tag each
message with this assigned ID. The Monitor uses the ID to correlate incoming
metrics with the corresponding threshold rules.

**Reporting contract**: every module periodically publishes a metrics message
to a dedicated iceoryx2 **metrics topic**, tagged with its assigned process
ID. The reporting frequency is **configurable** per-process (defaulting to a
low frequency such as 1 Hz to avoid overhead). Example metrics by module:

| Module            | Example metrics                                              |
|-------------------|--------------------------------------------------------------|
| Image Sensors     | Frame capture latency, dropped frames, actual vs target FPS  |
| Robots            | Control loop jitter, command-to-execution latency            |
| Encoder           | Queue depth, queue capacity, encoding latency per frame      |
| Episode Assembler | Buffer size (bytes/rows), assembly duration                  |
| Storage           | Queue depth, queue capacity, write/upload throughput (MB/s)  |
| Visualizer        | WebSocket client count, preview FPS, frame delivery latency  |
| Teleop Router     | Mapping latency, message rate                                |

**Threshold configuration**: thresholds are defined in the `[monitor]` section
of the master config, keyed by process ID and metric name. Each threshold
specifies an **explanation** (human-readable description shown in the UI when
triggered) and a **condition**. Supported condition forms:

| Condition    | Meaning                                          | Example                  |
|--------------|--------------------------------------------------|--------------------------|
| `gt`         | warn if value > threshold                        | `gt = 50.0`              |
| `lt`         | warn if value < threshold                        | `lt = 25.0`              |
| `gte`        | warn if value >= threshold                       | `gte = 100`              |
| `lte`        | warn if value <= threshold                       | `lte = 0`                |
| `outside`    | warn if value outside range                      | `outside = [10.0, 90.0]` |
| `inside`     | warn if value inside range                       | `inside = [0.0, 1.0]`    |
| `occurred`   | warn on any non-zero/non-null value (abnormal    | `occurred = true`        |
|              | once it happens at all)                          |                          |
| `gap`        | warn if consecutive values jump by more than     | `gap = 2.0`              |
|              | the given delta (continuity break)               |                          |
| `repeated`   | warn if the same value appears consecutively     | `repeated = true`        |
|              | (stale / stuck metric)                           |                          |

Example config fragment:

```toml
[monitor.thresholds."encoder.camera_top".queue_depth]
explanation = "Encoder queue for top camera is nearly full"
gt = 80

[monitor.thresholds."camera.top".actual_fps]
explanation = "Top camera FPS dropped below target"
lt = 28.0

[monitor.thresholds."storage.main".write_throughput_mbps]
explanation = "Storage write speed too low to keep up"
lt = 50.0
```

**Threshold evaluation**: the Monitor evaluates incoming metrics against
the configured thresholds. When a condition is met, the Monitor publishes a
**warning event** on iceoryx2 containing the process ID, metric name, current
value, and the explanation string. Downstream handling:

- The **Controller** subscribes to warning events and takes action where
  needed (e.g. queue-full warnings trigger the backpressure behavior described
  in §Data Flow).
- The **Visualizer** forwards warnings to the UI via WebSocket.
- The **UI** displays warnings prominently (e.g. a status bar alert).

Warnings are non-fatal by default — they inform the user but do not
automatically stop recording. The Monitor itself never takes corrective
action; it only observes and reports. Decisions are the Controller's
responsibility.

---

## Mock Devices

Mock (pseudo) devices are **first-class components**, not afterthoughts. They
serve two purposes:

1. **Testing without hardware**: the full pipeline (capture → encode →
   assemble → store) can be exercised on any development machine using
   synthetic data.
2. **Reference implementations**: they demonstrate the device driver interface
   contract, making it straightforward to add support for new hardware.

### Pseudo Camera (C++)

Generates synthetic frames (color bars with a burned-in timestamp, or
configurable test patterns). Supports all probe/validate/capabilities/run
subcommands. Publishes frames to iceoryx2 at a configurable resolution and
FPS, identical to a real camera driver.

### Pseudo Robot (Rust)

Simulates a robot arm with configurable degrees of freedom. In free-drive
mode, joint states follow a slow random walk or a sine pattern. In
command-following mode, joints track received commands with configurable
latency and noise. Publishes state and accepts commands on iceoryx2, identical
to a real robot driver.

---

## Technical Choices

### 1. TUI Framework

React + **Ink** (approach validated in a prototype). Camera preview rendered
as ANSI 256-color half-blocks (low resolution is acceptable; latency is
prioritized over fidelity). Ink's render throttle patched to 8ms for smooth
preview updates.

### 2. IPC

**iceoryx2** for all inter-process communication:

- **Pub-sub topics** for high-bandwidth data (camera frames, robot states).
  Zero-copy shared memory avoids frame copies between producers and consumers.
- **Events** for control signals (episode start/stop/keep/discard, shutdown,
  backpressure notifications, mode-switch commands). Low-latency notification
  delivery without polling.

The only exception is the **Visualizer ↔ UI** path, which uses WebSocket
(since the UI is a Node.js process and iceoryx2 does not have native
JavaScript bindings).

**Cross-language shared types**: iceoryx2 message payloads are defined as
Rust structs with `#[repr(C)]` and `#[derive(ZeroCopySend)]`, annotated with
`#[type_name("...")]` for a stable service-level type name. For C++ modules
(camera drivers), matching C/C++ struct headers are maintained alongside the
Rust definitions with identical layout. iceoryx2 enforces compatible type
names and sizes at the service layer. Python modules (robot drivers) use
iceoryx2's Python bindings (PyO3-based). All shared type definitions live in
a dedicated crate/directory in the repo; C/C++ headers are kept in sync
manually (validated by CI tests comparing sizes and alignment).

### 3. Probe Output Format

All probe/validate/capabilities subcommands output **JSON** to stdout.
Compact by default, with a `--pretty` flag for human-readable formatting.
Parseable by the Controller and by shell scripts.

### 4. Unit Tests and Mocking

Every module ships with unit tests. Mock backends are provided for all
external dependencies:

- Pseudo Camera and Pseudo Robot for device-level testing.
- In-memory storage backend for testing the Storage module without disk/network.
- Synthetic iceoryx2 publishers for testing consumers in isolation.

### 5. Robot Modes (initial)

Two modes: **free-drive** and **command-following**. Planning mode (single-shot
command execution) is deferred to a later design iteration.

### 6. Target Platforms

The framework must build and run on:

- **linux/amd64**: standard x86-64 Linux hosts (development workstations,
  servers). Hardware-accelerated encoding via NVENC (NVIDIA desktop/server
  GPUs) or VAAPI (Intel/AMD) when available, software fallback otherwise.

- **linux/arm64**: specifically **NVIDIA Jetson AGX Orin** and **AGX Thor**.
  These platforms provide NVENC, NVDEC, and other Jetson-specific hardware
  (DLA, multimedia API) that the Encoder module should leverage. Key
  considerations:
  - The Encoder must support **Jetson NVENC** (via Video Codec SDK or
    Jetson Multimedia API / V4L2 M2M) for hardware-accelerated H.264/H.265
    encoding on arm64.
  - NVDEC may be used by the Visualizer for efficient preview decode if
    the camera delivers pre-compressed streams (e.g. H.264 from some IP
    cameras).
  - All C++ and Rust code must cross-compile for `aarch64-unknown-linux-gnu`
    (or be natively compiled on the Jetson). Dependencies (iceoryx2, libjpeg-
    turbo, ffmpeg/libav, etc.) must be available for both architectures.
  - The UI (Node.js / Ink) runs on arm64 Node.js without architecture-
    specific concerns, but the `sharp` dependency used for image processing
    must be built for arm64 (it ships prebuilt binaries for both platforms).

All modules should avoid architecture-specific assumptions in their code.
Platform-specific paths (e.g. NVENC vs VAAPI, V4L2 M2M vs Video Codec SDK)
are selected at runtime based on capability probing, not compile-time `#ifdef`
branching where possible.

---

## Build Strategy

Each module is an **independent subproject** with its own build system:

| Module            | Language       | Build System         |
|-------------------|----------------|----------------------|
| Image Sensors     | C++            | CMake                |
| Robots            | Python / C++   | pip / CMake          |
| Teleop Router     | Rust           | Cargo                |
| Encoder           | Rust           | Cargo                |
| Episode Assembler | Rust           | Cargo                |
| Storage           | Rust           | Cargo                |
| Visualizer        | Rust           | Cargo                |
| Monitor           | Rust           | Cargo                |
| UI                | TypeScript     | npm                  |
| Controller        | Rust           | Cargo                |

The Rust modules (Teleop Router, Encoder, Episode Assembler, Storage,
Visualizer, Monitor, Controller) may share a **Cargo workspace** for convenience
(shared dependency versions, unified build cache) while still producing
separate binaries. Each module can also be built independently.

**Packaging**: a top-level script collects all built executables into a single
distributable package (tarball, .deb, or similar) for end-user delivery.
Separate packages are produced for **amd64** and **arm64** (Jetson). During
development, each module is built and tested in isolation.
