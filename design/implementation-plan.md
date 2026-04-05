# Implementation Plan

Sprints are ordered so that each checkpoint produces a system a human tester
can run end-to-end (with progressively more capability). The Visualizer and UI
are built first to establish the visual feedback loop early; the Controller
follows to provide orchestration; then data-path modules are added one at a
time, each sprint extending what a recorded episode contains.

---

## Sprint 0 — Project Scaffolding and Shared Types

**Goal**: establish the monorepo layout, build infrastructure, and shared
definitions that all subsequent modules depend on.

**Deliverables**:

- Top-level repository structure: one directory per module, `design/` for
  docs, `config/` for example configs.
- **Cargo workspace** for all Rust modules (Visualizer, Controller, Teleop
  Router, Encoder, Episode Assembler, Storage, Monitor) — initially empty
  crates with `main.rs` stubs.
- **CMake** project skeleton for Image Sensors (C++).
- **npm** project skeleton for UI (TypeScript / Ink), ported from the
  the react-tui prototype (reference only, not a dependency).
- **Shared iceoryx2 message schemas**: define the data types exchanged over
  iceoryx2 topics. At minimum:
  - `CameraFrame` (topic name, timestamp, width, height, pixel format, raw
    data pointer/offset).
  - `RobotState` (topic name, timestamp, joint positions, velocities, efforts,
    optional EE pose).
  - `RobotCommand` (timestamp, joint targets or Cartesian target).
  - `ControlEvent` (event type enum: recording_start, recording_stop,
    episode_keep, episode_discard, shutdown, mode_switch, ...).
  - `MetricsReport` (process ID, timestamp, key-value pairs of metric names
    and values).
  - `WarningEvent` (process ID, metric name, current value, explanation).
  - `VideoReady` (process ID, episode index, file path).
  - `BackpressureEvent` (process ID, queue name).
- **TOML config schema** draft: define the top-level sections and key fields
  (devices, pairing, storage, encoder, monitor, episode format). Write an
  example `config.example.toml`.
- **CI skeleton**: build all crates, run `cargo check`, `npm run build`,
  `cmake --build`.

**Tests**:

- _Unit — shared types_:
  - Each message type (`CameraFrame`, `RobotState`, `RobotCommand`,
    `ControlEvent`, `MetricsReport`, `WarningEvent`, `VideoReady`,
    `BackpressureEvent`) serializes and deserializes to the same values
    (round-trip identity).
  - `CameraFrame` correctly represents a 640×480 RGB24 buffer (width ×
    height × 3 bytes).
  - `RobotState` with 6 joints: all fields (positions, velocities, efforts)
    have length 6; optional EE pose is `None` when omitted and `Some` when
    provided.
  - `ControlEvent` enum covers all defined variants; unknown variant
    deserialization fails with a clear error.
- _Unit — config schema_:
  - Parse `config.example.toml` successfully via the schema.
  - Missing required fields (e.g. no `[[devices]]`) → descriptive error
    with field name and line number.
  - Invalid values (e.g. `fps = -1`, `codec = "nonexistent"`) → rejected
    with per-field error messages.
  - Duplicate device names → rejected.
- _Smoke — build_:
  - `cargo build --workspace` succeeds on both amd64 and arm64 targets.
  - `cmake --build` for the C++ skeleton succeeds.
  - `npm run build` for the UI skeleton succeeds.
  - CI pipeline passes all three.

**Checkpoint**: `cargo build --workspace` and `npm run build` succeed. No
runnable system yet — this is pure infrastructure.

---

## Sprint 1 — Visualizer + UI Skeleton

**Goal**: a working iceoryx2 → WebSocket → TUI pipeline. A human can see
synthetic data rendered in the terminal.

**Modules built**:

- **Visualizer** (Rust):
  - Subscribe to `CameraFrame` and `RobotState` topics on iceoryx2.
  - Downsample/compress camera frames to JPEG.
  - Serve a WebSocket endpoint (binary protocol: type-tagged frames for
    JPEG/RGB24; JSON messages for robot states and control status).
  - Accept incoming WebSocket messages (control commands) and publish to
    iceoryx2 (stubbed — no Controller to consume them yet).
  - Robustness: runs without subscribers or publishers, begins serving when
    either side appears.
- **UI** (TypeScript / Ink):
  - WebSocket client connecting to Visualizer.
  - `StreamPanel` component: decode JPEG → ANSI half-block rendering (port
    from the react-tui prototype; rewritten, not imported).
  - `RobotStatePanel` component: animated bars/gauges for joint values.
  - Responsive layout: side-by-side camera panels, robot state panels below.
  - Placeholder states when no data is arriving.
- **Test publisher** (Rust, small utility binary in the workspace):
  - Publishes synthetic `CameraFrame` messages (color bars with timestamp) and
    `RobotState` messages (sine-wave joint values) to iceoryx2 at configurable
    rates.
  - Used for manual testing; not part of the shipped product.

**Tests**:

- _Unit — Visualizer WebSocket protocol_:
  - Start Visualizer with no iceoryx2 publishers. Connect a WebSocket client.
    Verify the connection is accepted and stays open (no crash, no data sent).
  - Publish one `CameraFrame` (640×480 RGB24, solid red) on iceoryx2. Verify
    the WebSocket client receives a binary message with type tag = JPEG. Decode
    the JPEG payload — verify dimensions are ≤ 640×480 and the dominant color
    is red.
  - Publish a `RobotState` (6 joints, positions = [0.1, 0.2, ..., 0.6]).
    Verify the WebSocket client receives a JSON message with the same joint
    values.
  - Publish 100 `CameraFrame` messages at 30 FPS. Verify the Visualizer
    delivers frames without accumulating unbounded latency (last frame
    received within 200ms of last frame published).
- _Unit — Visualizer JPEG compression_:
  - Input: 1920×1080 RGB24 frame. Output JPEG must be smaller than the raw
    buffer. Decode the JPEG back — dimensions match, pixel values are close
    (PSNR > 30 dB).
- _Unit — Visualizer control forwarding_:
  - Send a JSON control command (e.g. `{"type": "episode_start"}`) from a
    WebSocket client. Verify it is published to the iceoryx2 control event
    topic with the correct `ControlEvent` variant.
- _Unit — UI StreamPanel_:
  - Feed a 320×240 JPEG of known content (red/green/blue vertical stripes) to
    `StreamPanel`. Snapshot the ANSI output. Verify it contains the expected
    color escape sequences (red in left third, green in middle, blue in right).
  - Feed a 1×1 JPEG (edge case). Verify it renders without crash (at least
    one character of output).
- _Unit — UI RobotStatePanel_:
  - Feed joint values `[0.0, 0.5, 1.0]` (min/mid/max of normalized range).
    Snapshot the output. Verify 3 bars are rendered with visually distinct
    fill levels.
  - Feed an empty joint array `[]`. Verify it renders a "no data" placeholder.
- _Unit — UI layout responsiveness_:
  - Render the layout at 80×24 (standard terminal) and 200×60 (large
    terminal). Verify both produce valid output without overflow or crash.
- _Smoke — end-to-end preview_:
  - Start test publisher (2 cameras at 30 FPS, 1 robot with 6 joints at
    50 Hz). Start Visualizer. Start UI. Verify UI renders within 2 seconds.
    Let it run for 10 seconds — no crash, no memory growth > 50 MB.
  - Kill the test publisher. Verify the UI shows placeholder states within
    1 second (not frozen on last frame).
  - Restart the test publisher. Verify the UI resumes showing live data.

**Checkpoint** (development-only, multi-terminal): in one terminal run the
test publisher, in another run the visualizer, in a third run the UI. The TUI
shows a live color-bar camera preview and oscillating robot joint bars.
Resizing the terminal reflows the layout. (From Sprint 2 onward, the
Controller provides a single-command entry point.)

---

## Sprint 2 — Controller + Pseudo Devices

**Goal**: `rollio collect -c config.toml` launches all processes, shows live
preview with synthetic data, and shuts down cleanly.

**Modules built**:

- **Controller** (Rust):
  - Parse and validate the TOML config file.
  - Spawn child processes (Visualizer, UI, device drivers) with config passed
    via `--config-inline`.
  - Monitor child process health (detect crashes, log errors).
  - Graceful shutdown: on SIGINT/SIGTERM or UI quit, send `shutdown` event on
    iceoryx2, wait for children to exit, then terminate.
  - `rollio collect -c <config>` subcommand wired up.
- **Pseudo Camera** (C++):
  - `probe`: return a list of pseudo camera IDs (configurable count).
  - `validate <id>`: always succeeds.
  - `capabilities <id>`: return configurable resolutions and FPS.
  - `run`: publish synthetic frames (color bars + burned-in timestamp + frame
    counter) to iceoryx2 at configured resolution/FPS. Listen for shutdown
    event.
- **RealSense camera driver** (C++):
  - `probe`: enumerate via `rs2::context`, return serial numbers.
  - `validate`: open pipeline with given serial.
  - `capabilities`: query supported stream profiles (color, depth, infrared)
    with resolutions and FPS.
  - `run`: start pipeline, publish frames. Multiple streams from same device
    → multiple iceoryx2 topics (one `run` invocation per stream).
- **Pseudo Robot** (Rust):
  - `probe`: return a list of pseudo robot IDs (configurable count and DoF).
  - `validate <id>`: always succeeds.
  - `capabilities <id>`: return DoF count, supported modes.
  - `run --mode free-drive`: publish sine-wave joint states to iceoryx2.
  - `run --mode command-following`: subscribe to command topic, track received
    commands with configurable latency/noise, publish resulting state.
  - Listen for shutdown and mode-switch events.
- **AIRBOT Play robot driver** (Python / C++):
  - `probe`: scan CAN interfaces for AIRBOT Play devices.
  - `validate`: ping device on CAN bus.
  - `capabilities`: report DoF: 6 (arm only; gripper is a separate G2
    device), supported modes, and connected end-effectors as metadata.
  - `run`: enter free-drive or command-following mode. Publish joint states.
    Accept joint commands. FK via built-in URDF/kinematics. Internal control
    loop at vendor-recommended frequency.

**Tests**:

- _Unit — Controller config parsing_:
  - Parse `config.example.toml` with 2 pseudo cameras + 2 pseudo robots.
    Verify all device entries are loaded with correct types, IDs, and
    parameters.
  - Parse a config with a RealSense camera entry — verify stream/channel
    fields are parsed correctly (color, depth, infrared as separate entries).
  - Parse a config referencing a nonexistent device type → descriptive error.
  - Parse a config with a teleop pair referencing a device name not in
    `[[devices]]` → rejected with "unknown device" error naming the bad ref.
- _Unit — Controller process lifecycle_:
  - Spawn a Pseudo Camera via the Controller. Verify it appears in the process
    table. Send a shutdown event on iceoryx2. Verify the process exits within
    2 seconds.
  - Spawn a Pseudo Camera and immediately `kill -9` it. Verify the Controller
    detects the crash within 1 second and logs an error with the process ID
    and exit code.
  - Spawn 5 child processes (2 cameras, 2 robots, 1 visualizer). Send
    SIGINT to the Controller. Verify all 5 children exit within 3 seconds
    and no orphan processes remain (`pgrep` returns empty).
- _Unit — Pseudo Camera_:
  - `probe` with `count=3`: output is valid JSON, contains exactly 3 entries,
    each with a unique ID.
  - `validate` with a valid pseudo ID → exit code 0, JSON `{"valid": true}`.
  - `capabilities` → JSON contains at least one (width, height, fps)
    combination and at least one pixel format.
  - `run` at 640×480, 30 FPS: subscribe to the iceoryx2 topic, collect 60
    frames over 2 seconds. Verify: frame count is 58–62 (±2 tolerance),
    all frames are 640×480, timestamps are monotonically increasing,
    inter-frame interval is 30–36ms (mean ± jitter).
  - `run` with shutdown event: send shutdown after 1 second. Verify the
    process exits within 500ms.
- _Unit — Pseudo Robot_:
  - `probe` with `count=2, dof=6`: output is valid JSON with 2 entries.
  - `capabilities` → JSON includes `dof: 6`, lists `free-drive` and
    `command-following` as supported modes.
  - `run --mode free-drive`: collect 100 state messages. Verify each has
    exactly 6 joint positions, values follow a sine pattern (fit to sine
    curve, R² > 0.95), timestamps are monotonic.
  - `run --mode command-following`: publish a step command (all joints to
    1.0). Collect states for 500ms. Verify joints converge to 1.0 ± 0.05.
    Publish a second step command (all joints to 0.0). Verify convergence
    again.
  - Mode switch: start in free-drive, send mode_switch event to
    command-following. Publish a command. Verify the robot begins tracking
    the command (state changes within 200ms).
- _Unit — RealSense driver (no hardware)_:
  - `probe` output is valid JSON (empty list when no device connected).
  - `capabilities` called with an invalid serial → exit code non-zero,
    error message contains the serial.
  - `run` called with an invalid serial → exits with error within 1 second.
- _Hardware — RealSense driver (requires D435i)_:
  - `probe` → JSON list with at least 1 entry matching the device serial.
  - `validate <serial>` → exit code 0.
  - `capabilities <serial>` → JSON lists color stream with at least
    (640, 480, 30) and (1280, 720, 30); depth stream with at least one
    profile.
  - `run` for color stream at 640×480, 30 FPS: collect 60 frames over 2s.
    Verify frame count ≈ 60, dimensions are 640×480, pixel format is RGB24.
  - `run` for depth stream at 640×480, 30 FPS: collect frames, verify
    dimensions and single-channel 16-bit format.
- _Unit — AIRBOT Play driver (no hardware)_:
  - `probe` output is valid JSON (empty list when no CAN interface available).
  - `capabilities` with an invalid ID → exit code non-zero.
- _Hardware — AIRBOT Play driver (requires CAN + arm)_:
  - `probe` → JSON list with at least 1 entry.
  - `validate <id>` → exit code 0.
  - `capabilities <id>` → JSON shows `dof: 6` (arm only; gripper is a
    separate G2 device), lists `free-drive` and `command-following` modes.
  - `run --mode free-drive`: collect 200 state messages over 1s. Verify
    6 joint positions per message, timestamps monotonic, publishing rate
    ≥ 100 Hz.
- _Smoke — single-command launch_:
  - `rollio collect -c config.example.toml` (2 pseudo cameras, 2 pseudo
    robots). Verify: all child processes are alive (check PIDs), UI is
    rendering (terminal has non-empty output), camera previews update
    (Visualizer WebSocket delivers ≥ 10 frames in first 2 seconds).
  - Send SIGINT (Ctrl+C). Verify all processes exit within 3 seconds.
    Confirm no orphan processes.
  - Run again with `--config-inline` instead of `--config`. Verify same
    behavior.

**Checkpoint**: write a `config.example.toml` with 2 pseudo cameras and 2
pseudo robots. Run `rollio collect -c config.example.toml`. The TUI shows 2
live camera previews (color bars with ticking timestamps) and 2 sets of robot
joint bars (oscillating sine waves). Press Ctrl+C — everything shuts down
within 2 seconds with no orphan processes.

**Hardware checkpoint** (requires physical devices): swap pseudo devices for
a RealSense camera and an AIRBOT Play arm in the config. Run
`rollio collect` — the TUI shows a real camera feed and real joint states.

---

## Sprint 3 — Teleop Router + Episode Lifecycle

**Goal**: leader–follower teleop works live. User can start/stop/keep/discard
episodes from the keyboard (state machine only — no actual recording yet).

**Modules built**:

- **Teleop Router** (Rust):
  - Subscribe to leader robot's state topic.
  - Apply direct joint mapping (index remap + scaling).
  - Publish commands to follower robot's command topic.
  - Configuration via `--config-inline`.
- **Controller** — additions:
  - Episode state machine: Idle → Recording → Pending → Idle.
  - Publish `recording_start`, `recording_stop`, `episode_keep`,
    `episode_discard` events on iceoryx2 (consumed by nobody yet, but the
    state transitions work).
  - Receive control commands from Visualizer (originating from UI keyboard).
  - Spawn Teleop Router processes for configured pairs.
- **UI** — additions:
  - Status bar showing episode state (Idle / Recording / Pending), episode
    count, and elapsed recording time.
  - Keyboard bindings: configurable keys for start, stop, keep, discard.
  - Send control commands to Visualizer via WebSocket.
- **Visualizer** — additions:
  - Forward control commands from UI (WebSocket) to Controller (iceoryx2).
  - Forward controller status (episode state, episode count) to UI.

**Tests**:

- _Unit — Teleop Router direct joint mapping_:
  - Leader publishes 6-joint state `[0.1, 0.2, 0.3, 0.4, 0.5, 0.6]`.
    Follower topic receives command with identical values (identity mapping).
  - Configure joint index remap `[5, 4, 3, 2, 1, 0]` (reverse). Verify
    follower receives `[0.6, 0.5, 0.4, 0.3, 0.2, 0.1]`.
  - Configure scaling factors `[2.0, 1.0, 1.0, 1.0, 1.0, 0.5]`. Verify
    follower joint 0 is `0.2` (0.1 × 2.0) and joint 5 is `0.3` (0.6 × 0.5).
- _Unit — Teleop Router latency_:
  - Publish leader states at 200 Hz. Measure time between leader publish and
    follower command publish. Verify median latency < 1ms, p99 < 5ms.
- _Unit — Teleop Router Cartesian forwarding_:
  - Leader publishes state with EE pose `{x: 0.3, y: 0.0, z: 0.5, ...}`.
    Configure Cartesian mode. Verify follower command topic receives the EE
    pose as a Cartesian command (not joint values).
- _Unit — Teleop Router shutdown_:
  - Send shutdown event on iceoryx2. Verify the router exits within 500ms.
- _Unit — Controller episode state machine_:
  - Idle → start → verify state is Recording.
  - Recording → stop → verify state is Pending.
  - Pending → keep → verify state is Idle, episode count increments by 1.
  - Pending → discard → verify state is Idle, episode count unchanged.
  - Invalid transitions are rejected:
    - Idle → stop → state remains Idle, error logged.
    - Idle → keep → state remains Idle, error logged.
    - Recording → start → state remains Recording, error logged.
    - Recording → keep → state remains Recording, error logged.
    - Pending → start → state remains Pending, error logged.
  - Rapid transitions: start → stop → keep → start → stop → discard in
    < 100ms total. Verify final state is Idle, episode count is 1 (only the
    first keep counted).
- _Unit — Controller episode event publishing_:
  - Trigger start → verify `recording_start` event appears on iceoryx2.
  - Trigger stop → verify `recording_stop` event.
  - Trigger keep → verify `episode_keep` event.
  - Trigger discard → verify `episode_discard` event.
  - Verify each event contains the correct episode index.
- _Unit — Visualizer control forwarding (round-trip)_:
  - Send `{"type": "episode_start"}` on WebSocket. Verify Controller receives
    a `recording_start` command on iceoryx2. Verify Controller publishes
    updated status. Verify Visualizer forwards the status JSON back to the
    WebSocket client with `state: "recording"`.
- _Unit — UI status bar_:
  - Receive status `{state: "idle", episode_count: 0}`. Snapshot output.
    Verify it shows "Idle" and "0 episodes".
  - Receive status `{state: "recording", elapsed_ms: 5000}`. Verify it shows
    "Recording" and "0:05".
  - Receive status `{state: "pending"}`. Verify it shows "Pending" with
    keep/discard prompts.
- _Smoke — teleop end-to-end_:
  - Config: 1 leader pseudo robot (free-drive, 6 DoF), 1 follower pseudo
    robot (command-following, 6 DoF), 2 pseudo cameras, direct joint mapping.
  - `rollio collect -c config.toml`. Collect follower state for 2 seconds.
    Compute cross-correlation between leader and follower joint 0 time
    series — verify correlation > 0.95 (follower tracks leader).
  - Press start key. Verify iceoryx2 `recording_start` event is published.
    Verify UI status bar shows "Recording". Press stop. Verify
    `recording_stop` event. Press keep. Verify `episode_keep` event and
    episode count = 1.

**Checkpoint**: config with 1 leader pseudo robot + 1 follower pseudo robot +
2 pseudo cameras. Run `rollio collect`. Follower robot bars mirror leader
bars in the TUI. Press start key — status bar shows "Recording" with a
running timer. Press stop — shows "Pending". Press keep — returns to "Idle",
episode count increments. Press start again, stop, discard — count stays the
same. No data is saved to disk yet.

---

## Sprint 4 — Encoder

**Goal**: during a recorded episode, camera frames are encoded to video files
on disk.

**Modules built**:

- **Encoder** (Rust):
  - `probe`: detect available codecs (H.264/H.265 via NVENC, VAAPI, or
    software libx264/libx265; FFV1; MJPEG). Output JSON.
  - `run`: subscribe to a camera frame topic on iceoryx2. On `recording_start`
    event, open a new video file and begin encoding frames. On
    `recording_stop`, flush the encoder pipeline, close the file, publish
    `video_ready` event with the file path.
  - Internal queue with configurable size. On queue full, publish
    `backpressure` event and reject incoming frames.
- **Controller** — additions:
  - Spawn one Encoder process per camera stream at startup.
  - Pass codec, output directory, and iceoryx2 topic via config.
  - Handle `backpressure` events: block episode start or discard current
    episode.

**Tests**:

- _Unit — Encoder probe_:
  - Run `encoder probe`. Verify output is valid JSON with a `codecs` array.
    Each entry has `name`, `type` (hardware/software), and `pixel_formats`.
  - On a machine with NVENC: verify `h264_nvenc` or `hevc_nvenc` appears.
  - On a machine without GPU: verify only software codecs appear (libx264,
    libx265, ffv1).
- _Unit — Encoder H.264 software encoding_:
  - Publish 90 synthetic frames (640×480 RGB24, 30 FPS) to iceoryx2. Send
    `recording_start`, wait for 90 frames, send `recording_stop`. Verify:
    - A `video_ready` event is published with a valid file path.
    - The output file exists, is a valid MP4, contains 90 frames at 30 FPS
      (verify with `ffprobe`: `nb_frames=90`, `r_frame_rate=30/1`,
      `width=640`, `height=480`, `codec_name=h264`).
    - Decoding the video back to frames: first and last frame pixel values
      match input (PSNR > 30 dB against the synthetic original).
- _Unit — Encoder FFV1 (depth)_:
  - Publish 30 synthetic depth frames (640×480, 16-bit single-channel) to
    iceoryx2. Record. Verify output is a valid MKV with `codec_name=ffv1`.
    Decode back — verify pixel values are **identical** to input (lossless).
- _Unit — Encoder empty episode_:
  - Send `recording_start` immediately followed by `recording_stop` (0
    frames). Verify the encoder handles this gracefully: either produces
    a valid empty container or publishes no `video_ready` event (no crash).
- _Unit — Encoder back-to-back episodes_:
  - Record episode 1 (30 frames), stop, keep. Immediately record episode 2
    (30 frames), stop, keep. Verify two separate video files are produced,
    each with 30 frames. File names contain distinct episode indices.
- _Unit — Encoder backpressure_:
  - Set queue size to 5. Publish frames at 60 FPS to a codec configured for
    slow software encoding. Verify a `backpressure` event is published on
    iceoryx2 within 2 seconds.
  - After backpressure fires, verify the encoder continues processing queued
    frames (does not deadlock). The output video has fewer frames than
    published (frames were dropped).
- _Unit — Encoder shutdown_:
  - Start encoding, send shutdown event mid-recording. Verify the encoder
    flushes the current file (valid video, not truncated) and exits within
    2 seconds.
- _Smoke — Encoder in pipeline_:
  - Config: 2 pseudo cameras (640×480, 30 FPS), codec = libx264. Run
    `rollio collect`. Start episode, wait 3 seconds, stop, keep. Verify
    2 MP4 files appear in the output directory within 5 seconds. Each is
    playable and ≈ 90 frames long.

**Checkpoint**: config with 2 pseudo cameras and encoder codec set to
`libx264`. Run `rollio collect`. Start an episode, wait a few seconds, stop,
keep. Two `.mp4` files appear in the configured output directory. Play them
with `ffplay` — each shows the pseudo camera's color bars with correct timing.

---

## Sprint 5 — Episode Assembler + Local Storage

**Goal**: a complete record → keep cycle produces a valid LeRobot v2.1 episode
on disk — video files, Parquet tabular data, and metadata.

**Modules built**:

- **Episode Assembler** (Rust):
  - Subscribe to all robot state topics on iceoryx2. On `recording_start`,
    begin buffering timestamped states. On `recording_stop`, freeze the buffer.
  - Receive `video_ready` events from Encoder(s). Once all expected videos are
    received, begin assembly.
  - Resample robot states to nominal FPS.
  - Capture action vectors by subscribing to follower command topics (the
    same topics the Teleop Router publishes to).
  - Write Parquet file (columns: timestamp, frame_index, episode_index, index,
    per-robot state columns, action).
  - Write `meta/info.json` with features, FPS, episode/frame counts, video
    info.
  - Embed the full TOML config in episode metadata.
  - Organize the directory layout per LeRobot v2.1
    (`data/chunk-XXX/episode_NNNNNN.parquet`,
    `videos/chunk-XXX/<camera>/episode_NNNNNN.<ext>`).
  - On `episode_discard`, drop all buffered data.
  - Notify Storage when the episode directory is ready.
- **Storage** (Rust) — local backend only:
  - Receive completed episode path from Episode Assembler.
  - Move/copy the episode directory to the configured output location.
  - Publish `episode_stored` event to Controller.
  - Internal queue with configurable size; `backpressure` event on full.
- **Controller** — additions:
  - Spawn Episode Assembler and Storage at startup.
  - Track `episode_stored` events to update episode count.

**Tests**:

- _Unit — Episode Assembler robot state buffering_:
  - Publish 150 `RobotState` messages at 50 Hz (3 seconds of data) between
    `recording_start` and `recording_stop`. Verify the assembler buffers
    exactly 150 entries with correct timestamps.
  - Publish states for 2 robots (leader + follower). Verify both are buffered
    independently with correct process IDs.
- _Unit — Episode Assembler resampling_:
  - Buffer 150 states at 50 Hz, nominal FPS = 30. Verify resampled output has
    90 rows (3s × 30 FPS). Verify timestamps are evenly spaced at 33.33ms
    intervals. Verify joint values are interpolated (not duplicated or
    dropped).
- _Unit — Episode Assembler action vector_:
  - Configure a teleop pair (leader → follower, direct joint mapping, 6 DoF).
    Verify the `action` column in the Parquet output has 6 values per row,
    matching the commands that were sent to the follower.
- _Unit — Episode Assembler Parquet output_:
  - Assemble an episode with 2 cameras, 2 robots (6 DoF each), 90 frames.
    Read the Parquet file. Verify:
    - Row count = 90.
    - Columns: `timestamp` (float64), `frame_index` (int), `episode_index`
      (int, constant), `index` (int, globally unique).
    - Per-robot columns: `observation.state.<robot_name>.position` (6 floats),
      `.velocity` (6 floats), `.effort` (6 floats).
    - `action` column (6 floats).
    - All values are finite (no NaN, no Inf).
- _Unit — Episode Assembler metadata_:
  - Assemble an episode. Read `meta/info.json`. Verify:
    - `codebase_version` is present and non-empty.
    - `fps` matches the configured FPS.
    - `total_episodes` = 1, `total_frames` = 90.
    - `features` lists each camera with `dtype: "video"` and `video_info`
      containing codec, resolution, and FPS.
    - `features` lists each robot's state channels.
    - Embedded config is present (a string or object containing the full TOML).
- _Unit — Episode Assembler directory layout_:
  - Assemble episode index 0. Verify file paths:
    - `data/chunk-000/episode_000000.parquet` exists.
    - `videos/chunk-000/<camera_name>/episode_000000.mp4` exists for each
      camera.
    - `meta/info.json` exists.
  - Assemble episode index 7 (testing non-zero index and chunk boundaries).
    Verify correct `episode_000007.parquet` naming.
- _Unit — Episode Assembler discard_:
  - Start buffering, publish 50 states, send `episode_discard`. Verify no
    files are written to disk. Buffer memory is released (measure RSS before
    and after — increase < 1 MB).
- _Unit — Episode Assembler missing video_:
  - Config expects 2 cameras. Only 1 `video_ready` event arrives. Verify the
    assembler waits up to a configurable timeout, then logs an error and
    discards the episode (not a hang or crash).
- _Unit — Storage local backend_:
  - Create a temp directory with a mock episode. Notify Storage. Verify the
    directory is moved to the configured output path. Original temp path no
    longer exists.
  - Verify `episode_stored` event is published with the correct episode index.
- _Unit — Storage backpressure_:
  - Set queue size to 2. Submit 5 episodes rapidly. Verify a `backpressure`
    event is published after the 3rd submission. Verify the first 2 episodes
    are stored correctly.
- _Smoke — full pipeline_:
  - Config: 2 pseudo cameras, 2 pseudo robots (leader + follower), codec =
    libx264, output = temp directory, FPS = 30. Run `rollio collect`.
  - Record 3 episodes (start, wait 2s, stop, keep × 3). Verify:
    - Output directory contains `meta/info.json`, 3 Parquet files, and
      3 × 2 = 6 video files.
    - `info.json` shows `total_episodes: 3`.
    - Each Parquet file has ≈ 60 rows (2s × 30 FPS).
    - Each video file is playable and ≈ 2 seconds long.
  - Record episode 4: start, wait 1s, stop, discard. Verify no 4th episode
    appears on disk. `info.json` still shows `total_episodes: 3`.
- _Validation — LeRobot compatibility_:
  - Load the output directory with the `lerobot` Python library (or a
    standalone validation script). Verify it is recognized as a valid v2.1
    dataset. Read episode 0 — verify frame count, feature shapes, and video
    playback.

**Checkpoint**: full pipeline with pseudo devices. Record 3 episodes (start,
wait, stop, keep × 3). Output directory contains:

```
my_dataset/
  meta/info.json
  data/chunk-000/episode_000000.parquet
  data/chunk-000/episode_000001.parquet
  data/chunk-000/episode_000002.parquet
  videos/chunk-000/camera_top/episode_000000.mp4
  videos/chunk-000/camera_top/episode_000001.mp4
  ...
```

`info.json` lists 3 episodes. Parquet files contain timestamped joint state
rows. The embedded config is present in the metadata. A discard cycle (start,
stop, discard) leaves no trace on disk.

---

## Sprint 6 — Setup Wizard

**Goal**: `rollio setup` walks the user through device discovery, selection,
configuration, and pairing, then saves a config file. The generated config can
be used directly with `rollio collect`.

**Modules built**:

- **Controller** — `setup` subcommand:
  - Discover available device driver executables (pseudo + any real drivers
    installed).
  - For each driver, run `probe` to discover devices, then `capabilities` for
    each discovered device.
  - Aggregate results and relay to UI.
  - After user confirms, write the TOML config file.
  - If launched with `-c config.toml`, validate and skip to preview page.
- **UI** — setup wizard flow:
  - **Step 1 — Device discovery**: show a list of all discovered devices
    (cameras and robots), grouped by type. For cameras, show a live preview
    thumbnail (launch the camera driver temporarily). For robots, indicate
    identification method (LED / free-drag).
  - **Step 2 — Device selection**: user checks/unchecks devices. For each
    selected device, choose which channels to record.
  - **Step 3 — Device parameters**: for each selected device, show editable
    parameter fields with defaults from `capabilities`. Camera: resolution,
    FPS, pixel format. Robot: control mode, etc.
  - **Step 4 — Pairing**: assign leader/follower relationships. Choose mapping
    strategy per pair.
  - **Step 5 — Storage & format**: select storage backend (local / HTTP),
    set output path or endpoint, select episode format (LeRobot v2.1 /
    v3.0 / mcap). If HTTP: test endpoint availability.
  - **Step 6 — Preview page**: launch all selected devices in their configured
    modes. Show the same layout as `rollio collect` (camera previews + robot
    state bars + teleop active). Allow parameter tweaks, device add/remove.
  - **Confirm & save**: write `config.toml`.
- **Visualizer / device drivers**: no changes — reused as-is.

**Tests**:

- _Unit — Controller probe orchestration_:
  - Mock 2 camera drivers and 1 robot driver with known probe outputs. Run
    the setup probe cycle. Verify the aggregated result lists all 3 drivers
    with correct device counts and IDs.
  - One driver `probe` fails (exits non-zero). Verify the error is reported
    for that driver and the other 2 drivers' results are still available.
  - One driver `probe` hangs (exceeds 200ms timeout). Verify it is killed
    and reported as timed out. Other drivers proceed normally.
- _Unit — Controller config generation_:
  - Feed the Controller a complete set of user selections (devices, params,
    pairing, storage). Verify the written TOML file:
    - Parses back without errors.
    - Contains the correct number of `[[devices]]` entries.
    - Device parameters match the user's choices (not just defaults).
    - `[pairing]` section correctly references device names.
    - `[storage]` section matches the selected backend and path.
- _Unit — Controller config resume_:
  - `rollio setup -c config.toml` where `config.toml` is valid and all
    devices exist (pseudo). Verify: no probe cycle runs, the Controller
    enters preview mode directly.
  - `rollio setup -c bad_config.toml` with a syntax error. Verify: error
    message with line number, process exits non-zero.
  - `rollio setup -c config.toml` where one device ID doesn't exist. Verify:
    validation error naming the missing device.
- _Unit — UI wizard step rendering_:
  - Step 1 (discovery): feed a device list with 2 cameras + 1 robot. Snapshot
    test. Verify: devices are grouped by type, camera entries show a preview
    placeholder, robot entry shows identification method.
  - Step 2 (selection): render with 3 devices, toggle device 2 off. Verify
    the selection state updates and only devices 1 and 3 are marked selected.
  - Step 3 (parameters): render with a camera device, defaults from
    capabilities (resolution: 640×480, FPS: 30). Change resolution to
    1280×720. Verify the parameter value updates in the component state.
  - Step 4 (pairing): render with 2 robots. Assign robot A as leader, robot B
    as follower. Verify the pairing data structure is correct.
  - Step 5 (storage): select local storage, set path to `/tmp/test`. Verify.
    Select HTTP, enter URL. Verify the endpoint test button is shown.
- _Unit — UI preview page parameter editing_:
  - In the preview page, change a camera's FPS from 30 to 15. Verify the
    config state updates. Verify the camera driver is restarted with new
    parameters (mock: verify the restart command is issued).
  - Add a device in preview. Verify it appears in the layout.
  - Remove a device in preview. Verify it disappears and the config updates.
- _Smoke — setup round-trip_:
  - `rollio setup` with 2 pseudo cameras + 2 pseudo robots available. Walk
    through all 6 steps (scripted input or interactive). Save to
    `/tmp/test_config.toml`.
  - Verify the file exists and parses.
  - `rollio collect -c /tmp/test_config.toml` → launches successfully, TUI
    shows the configured devices, episode lifecycle works.
  - `rollio setup -c /tmp/test_config.toml` → skips to preview page, shows
    the same devices as the collect view.

**Checkpoint**: run `rollio setup`. The wizard discovers pseudo cameras and
pseudo robots. Walk through all steps — select devices, tweak resolution,
pair a leader and follower, choose local storage. Preview page shows live
feeds. Confirm. A `config.toml` file is written. Then run
`rollio collect -c config.toml` — it launches correctly and the preview
matches what was configured. Run `rollio setup -c config.toml` — it skips
straight to the preview page.

---

## Sprint 7 — Monitor

**Goal**: health and performance metrics from all modules are collected,
evaluated against thresholds, and warnings are displayed in the UI.

**Modules built**:

- **Monitor** (Rust):
  - Subscribe to the iceoryx2 metrics topic.
  - Load threshold configuration from master config.
  - Evaluate all condition types: `gt`, `lt`, `gte`, `lte`, `outside`,
    `inside`, `occurred`, `gap`, `repeated`.
  - Publish `WarningEvent` on threshold breach.
- **All existing modules** — additions:
  - Each module publishes `MetricsReport` messages to iceoryx2 at the
    configured frequency, tagged with its assigned process ID.
  - Image Sensors: frame capture latency, dropped frames, actual FPS.
  - Robots: control loop jitter, command latency.
  - Encoder: queue depth/capacity, encoding latency.
  - Episode Assembler: buffer size, assembly duration.
  - Storage: queue depth/capacity, write throughput.
  - Visualizer: client count, preview FPS, delivery latency.
  - Teleop Router: mapping latency, message rate.
- **Visualizer** — additions: forward `WarningEvent` to UI via WebSocket.
- **UI** — additions:
  - Warning bar/toast component: shows active warnings with process ID, metric
    name, current value, and explanation string.
  - Warnings auto-dismiss after the condition clears (on next healthy metric
    report).
- **Controller** — additions:
  - Spawn Monitor process.
  - Subscribe to `WarningEvent` for backpressure-related warnings.

**Tests**:

- _Unit — Monitor threshold evaluation (value-based)_:
  - `gt = 50`: feed values 49, 50, 51. Verify: no warning, no warning,
    warning.
  - `lt = 10`: feed values 11, 10, 9. Verify: no warning, no warning,
    warning.
  - `gte = 100`: feed 99, 100. Verify: no warning, warning.
  - `lte = 0`: feed 1, 0. Verify: no warning, warning.
  - `outside = [10, 90]`: feed 50 (ok), 5 (warning), 95 (warning), 10 (ok),
    90 (ok).
  - `inside = [0, 1]`: feed -1 (ok), 0 (warning), 0.5 (warning), 1
    (warning), 2 (ok).
- _Unit — Monitor threshold evaluation (stateful)_:
  - `occurred = true`: feed 0 (no warning), 0 (no warning), 1 (warning),
    0 (no warning). Verify warning fires exactly once on the value 1.
  - `gap = 2.0`: feed values 1, 2, 3, 10. Verify: no warning for 1→2
    (delta 1), no warning for 2→3 (delta 1), warning for 3→10 (delta 7 >
    2.0). Then feed 11 (delta 1, ok).
  - `repeated = true`: feed 1, 2, 3, 3. Verify: no warning for 1→2→3
    (all different), warning for 3→3 (repeated). Then feed 4 (ok, no
    warning).
- _Unit — Monitor process ID matching_:
  - Configure thresholds for process `encoder.camera_top` only. Publish
    metrics from `encoder.camera_top` (above threshold) and
    `encoder.camera_bottom` (also above threshold). Verify warning fires
    only for `camera_top`.
- _Unit — Monitor warning event content_:
  - Trigger a threshold breach. Verify the `WarningEvent` contains: process
    ID, metric name, current value, threshold value, condition type, and the
    configured explanation string verbatim.
- _Unit — Monitor no false positives under load_:
  - Publish 10,000 metrics messages from 5 different process IDs at 100 Hz.
    All values within thresholds. Verify zero `WarningEvent` messages
    published.
- _Unit — module metrics publishing_:
  - For each module (Pseudo Camera, Pseudo Robot, Encoder, Storage,
    Visualizer, Teleop Router): run the module for 5 seconds with metrics
    frequency = 2 Hz. Verify at least 8 `MetricsReport` messages are
    published, each tagged with the correct process ID and containing at
    least 1 non-zero metric value.
- _Unit — UI warning component_:
  - Feed a `WarningEvent` to the warning component. Snapshot test. Verify:
    process ID, metric name, current value, and explanation are all visible.
  - Feed a second warning for a different process. Verify both are displayed.
  - Send a healthy metric for the first process. Verify its warning
    auto-dismisses, second warning remains.
- _Smoke — threshold warning end-to-end_:
  - Config: pseudo camera with `actual_fps` threshold `lt = 28`. Set pseudo
    camera to publish at 15 FPS. Run `rollio collect`. Verify the UI shows a
    warning within 5 seconds containing the explanation string. Change the
    config to 30 FPS, restart. Verify no warning appears.
- _Smoke — backpressure via monitor_:
  - Config: encoder queue threshold `gt = 4`, queue size = 5. Record with a
    very slow codec and high-resolution camera to fill the queue. Verify the
    Monitor publishes a warning, the Controller receives it, and either
    blocks the next episode or discards the current one.

**Checkpoint**: add `[monitor.thresholds]` entries to the config (e.g.
camera FPS < 28, encoder queue > 80%). Run `rollio collect`. All modules
report metrics. Deliberately misconfigure a pseudo camera to run at low FPS
— the UI shows a warning: "Top camera FPS dropped below target (actual: 15,
threshold: < 28)". Fix the config, restart — warning disappears.

---

## Sprint 8 — Remote Storage Backend

**Goal**: episodes can be uploaded via HTTP in addition to local storage.
(S3 support is deferred to a future sprint.)

**Modules built**:

- **Storage** — additions:
  - **HTTP upload backend**: POST episode files to configured endpoint with
    configurable auth headers. Retry on transient failures.
  - Backend selection from config.
- **Companion HTTP receive server** (Rust, separate binary):
  - A minimal HTTP server that accepts episode uploads and writes them to a
    local directory. Intended as the receiving end for the HTTP upload backend.
  - Provides a health endpoint for availability testing.
- **Controller / setup wizard** — additions:
  - When HTTP upload is selected during setup, test the endpoint availability
    immediately and report success/failure to the UI.

**Tests**:

- _Unit — HTTP upload backend_:
  - Start companion server on localhost. Upload a mock episode directory (3
    files, 10 MB total). Verify all files appear on the server's filesystem
    with correct names and byte-identical content.
  - Verify the upload request includes configured auth headers (e.g.
    `Authorization: Bearer <token>`).
  - Simulate server returning 500 on first attempt. Verify the client retries
    (up to configured max retries) and succeeds on the second attempt.
  - Simulate server being unreachable (connection refused). Verify the upload
    fails with a clear error after retries are exhausted. Verify a
    `backpressure` event is published.
- _Unit — HTTP upload large episode_:
  - Upload an episode with a 500 MB video file. Verify the upload completes
    without OOM (streaming upload, not buffered in memory). Verify the file
    is byte-identical on the server.
- _Unit — Companion HTTP server_:
  - Start server, hit `/health` endpoint. Verify 200 OK response.
  - Upload 3 episodes concurrently. Verify all 3 are stored correctly (no
    interleaving or corruption).
  - Start server with a read-only output directory. Verify upload returns
    a 500 error with a descriptive message.
- _Unit — Availability test_:
  - Companion server running → availability test returns success.
  - Companion server not running → availability test returns failure with
    "connection refused" error within 5 seconds.
  - Companion server running but returns 401 → availability test returns
    failure with "authentication error".
- _Smoke — HTTP upload in pipeline_:
  - Start companion server. Config: storage backend = HTTP, endpoint =
    `http://localhost:<port>`. Run `rollio setup` → endpoint test passes.
    Run `rollio collect`, record 2 episodes. Verify both episodes appear
    on the companion server's filesystem with valid LeRobot v2.1 structure.
  - Stop the companion server mid-collection. Record another episode. Verify
    the Storage module retries, eventually publishes `backpressure`, and the
    user is warned in the UI.

**Checkpoint**: start the companion HTTP server on the same or another
machine. Configure `rollio setup` with HTTP upload backend, endpoint URL is
tested and confirmed reachable. Run `rollio collect`, record episodes — they
appear on the companion server's filesystem with correct LeRobot v2.1
structure.

---

## Sprint 9 — Additional Hardware Drivers

**Goal**: expand hardware support beyond the core RealSense + AIRBOT Play
drivers shipped in Sprint 2.

**Modules built**:

- **V4L2 camera driver** (C++):
  - `probe`: enumerate `/dev/video*` devices, return IDs.
  - `validate`: open and close the device.
  - `capabilities`: query V4L2 for supported formats, resolutions, frame
    rates.
  - `run`: capture frames via V4L2 API, publish to iceoryx2. Support MJPEG
    and YUYV pixel formats (decode MJPEG to raw if needed for encoding).
- **AIRBOT G2 / E2 drivers**: similar structure to the AIRBOT Play driver,
  targeting the gripper and demonstrator hardware.

**Tests**:

- _Unit — V4L2 driver (no hardware)_:
  - `probe` output is valid JSON (empty list on a machine with no
    `/dev/video*` devices).
  - `capabilities` with an invalid device path → exit code non-zero, error
    message contains the path.
- _Hardware — V4L2 driver (requires USB webcam)_:
  - `probe` → JSON list with at least 1 entry. Entry contains device path
    (e.g. `/dev/video0`) and device name string.
  - `validate </dev/video0>` → exit code 0.
  - `capabilities` → JSON lists at least one (width, height, fps) entry and
    at least one pixel format (MJPEG or YUYV).
  - `run` at 640×480, 30 FPS, YUYV: collect 60 frames over 2s. Verify
    frame count ≈ 60, dimensions 640×480, pixel format RGB24 (converted from
    YUYV). Verify frames are not all-black or all-identical (live image).
  - `run` at 640×480, 30 FPS, MJPEG: collect frames. Verify MJPEG→RGB
    conversion produces valid frames.
- _Hardware — AIRBOT G2 driver (requires CAN + gripper)_:
  - `probe` → JSON list with at least 1 entry.
  - `capabilities` → JSON shows DoF = 1 (single-axis gripper), supported
    modes.
  - `run --mode command-following`: send open/close commands. Verify the
    gripper state reflects the commands within 200ms.
- _Smoke — mixed hardware_:
  - Config: 1 RealSense (color + depth), 1 V4L2 webcam, 2 AIRBOT Play arms,
    1 AIRBOT G2 gripper. Run `rollio collect`. Verify all 3 camera feeds and
    all 3 robot state panels render in the TUI. Record an episode — output
    contains 3 video files and state data for all 3 robots.

**Checkpoint**: connect 2 USB cameras, 1 RealSense (color + depth), 2
AIRBOT Play arms (leader + follower), and an AIRBOT G2 gripper. Run
`rollio setup` — all devices are discovered. Select all, configure pairing.
Run `rollio collect` — the TUI shows all camera feeds and all robot states.
The leader arm in free-drive moves the follower. Record an episode — the
output contains real video and real joint data in LeRobot v2.1 format.

---

## Sprint 10 — Replay + Additional Format Backends

**Goal**: `rollio replay` plays back trajectories on real hardware. LeRobot
v3.0 and mcap are available as episode format alternatives.

**Modules built**:

- **Controller** — `replay` subcommand:
  - Load episode from disk, extract embedded config.
  - Validate config (device availability, command-following support).
  - Spawn robot drivers in command-following mode.
  - Optionally spawn camera drivers + Visualizer + UI for visual confirmation.
  - Read action vectors from Parquet, publish commands to robot command topics
    on iceoryx2 at the original FPS timing.
  - On completion or user interrupt, stop gracefully.
- **Episode Assembler** — additions:
  - **LeRobot v3.0** format backend: sharded layout per the v3.0 spec.
  - **mcap** format backend: write episodes as mcap files.
- **UI** — additions:
  - Replay status display: progress bar, current/total frames, playback
    state.
  - Keyboard controls: pause, resume, stop replay.

**Tests**:

- _Unit — Replay config extraction_:
  - Load a LeRobot v2.1 episode (from Sprint 5 test output). Extract the
    embedded config. Parse it. Verify it matches the original config used
    during recording.
- _Unit — Replay device validation_:
  - Episode config references pseudo robot `pseudo_0`. Pseudo robot is
    available → validation passes.
  - Episode config references device `nonexistent_arm` → validation fails
    with "device not found: nonexistent_arm".
  - Episode config references a robot that only supports free-drive →
    validation fails with "device does not support command-following".
- _Unit — Replay trajectory timing_:
  - Record a 3-second episode at 30 FPS (90 frames) with a pseudo robot.
    Replay it. Measure the time between the first and last command published
    to the follower's command topic. Verify it is 3.0s ± 100ms.
  - Verify the inter-command interval is 33.3ms ± 5ms (30 FPS timing).
- _Unit — Replay trajectory accuracy_:
  - Record an episode where the pseudo leader follows a known sine wave.
    Replay the episode. Collect the commands received by the pseudo follower.
    Compare to the original recorded action vectors. Verify max absolute
    error < 0.001 per joint (the replay should reproduce the exact recorded
    values).
- _Unit — Replay pause/resume_:
  - Start replay. After 1 second, send pause command. Verify no commands are
    published for 2 seconds. Send resume. Verify commands resume from where
    they left off (next frame after pause, not from the beginning).
- _Unit — Replay stop_:
  - Start replay. After 1 second, send stop command. Verify the replay
    process exits cleanly within 1 second. Verify the robot driver receives
    no further commands after stop.
- _Unit — Replay UI_:
  - During replay, verify the UI shows: progress bar (e.g. "30/90 frames"),
    playback state ("Playing" / "Paused"), elapsed time.
- _Unit — LeRobot v3.0 format_:
  - Record an episode with format = v3.0. Verify output directory follows the
    v3.0 sharded layout (file names, directory structure, metadata format
    match the spec).
  - Verify a v3.0 episode can be loaded by the `lerobot` library (if v3.0
    reader is available) or passes structural validation.
- _Unit — mcap format_:
  - Record an episode with format = mcap. Verify the output is a valid mcap
    file. Open with the `mcap` library — verify topics are present for each
    camera and robot, message count matches frame count, timestamps are
    ordered.
- _Smoke — replay round-trip (pseudo)_:
  - `rollio collect` → record episode → `rollio replay -e <episode>`. Verify
    the pseudo follower's state time series during replay matches the original
    recording (correlation > 0.99).
- _Hardware — replay round-trip (AIRBOT, requires arms)_:
  - Record a slow arm movement with AIRBOT Play. Replay it. Visually verify
    the arm reproduces the motion. Compare replayed joint positions (read from
    the arm's state during replay) to the original recording — max error
    < 5 degrees per joint (real hardware has tracking error).

**Checkpoint**: with pseudo devices, record an episode, then
`rollio replay -e <episode>`. The pseudo follower robot's state bars in the
TUI trace through the same trajectory. With real AIRBOT arms: record a
pick-and-place motion, replay it — the robot physically reproduces the motion.

Switch episode format to LeRobot v3.0 in config, record episodes — output
directory follows the v3.0 layout. Same for mcap.

---

## Sprint 11 — Hardening + Packaging

**Goal**: production-quality error handling, performance, and a distributable
package.

**Deliverables**:

- **Error handling**: device disconnection during recording (warn in UI,
  discard episode if data integrity is lost), Encoder/Storage crash recovery
  (Controller detects, restarts, warns user), config validation edge cases.
- **Performance**: latency profiling of the full pipeline (camera capture →
  iceoryx2 → encoder, robot state → iceoryx2 → teleop router → follower).
  Tune iceoryx2 buffer sizes, queue lengths, encoder settings for typical
  hardware configurations.
- **Backpressure end-to-end**: stress test with high camera count / high
  resolution / slow storage to verify the backpressure protocol works
  correctly under load.
- **Documentation**: user-facing README, `rollio --help` polish, config file
  reference, driver development guide (how to add a new camera or robot type).
- **Packaging script**: collect all built executables (Rust binaries, C++
  binaries, Node.js bundle, Python packages) into a single distributable
  archive. Test on a clean machine.

**Tests**:

- _Stress — camera disconnection mid-recording_:
  - Config: 1 pseudo camera, 1 pseudo robot. Start recording. After 2s, kill
    the pseudo camera process (simulating unplug). Verify: Controller detects
    the crash within 1s, UI shows a warning naming the camera, current episode
    is discarded, system remains in Idle state and does not crash. Start a new
    recording with remaining devices — it works.
- _Stress — robot disconnection mid-recording_:
  - Same as above but kill the pseudo robot process. Verify: warning, episode
    discard, system stable.
- _Stress — Encoder crash recovery_:
  - Kill an Encoder process mid-recording. Verify: Controller detects crash,
    warns user, discards episode. Controller restarts the Encoder. Next
    recording works normally with the restarted Encoder.
- _Stress — Storage crash recovery_:
  - Kill the Storage process after an episode is kept. Verify: Controller
    detects crash, restarts Storage. The episode data is not lost (still in
    the temp directory). Storage picks up and stores it after restart.
- _Stress — high camera count_:
  - Config: 8 pseudo cameras (640×480, 30 FPS), 2 pseudo robots. Run
    `rollio collect`. Record 5 episodes of 5 seconds each. Verify: all 8
    camera previews render, all episodes produce 8 video files each, no
    frame drops (frame count = 150 per video ± 2), total latency from
    frame capture to preview < 100ms.
- _Stress — high resolution_:
  - Config: 2 pseudo cameras at 1920×1080, 30 FPS with H.265 NVENC (or
    software fallback). Record 3 episodes of 10 seconds each. Verify: no
    backpressure warnings, video files are valid, encoding does not fall
    behind (queue depth stays < 50% capacity).
- _Stress — slow storage_:
  - Config: local storage with output directory on a simulated slow filesystem
    (e.g. rate-limited via `cgroup` or a FUSE mount). Record 5 short episodes
    rapidly. Verify: backpressure eventually fires, user is blocked from
    starting the next episode, previously stored episodes are intact.
- _Stress — 50-episode session_:
  - Config: 2 pseudo cameras, 2 pseudo robots. Record 50 episodes of 3
    seconds each (start, wait 3s, stop, keep, repeat). Verify: all 50
    episodes are stored, no memory growth (RSS stays within 2× initial),
    no file descriptor leaks (lsof count stable), episode count in
    `info.json` = 50.
- _Packaging — clean install_:
  - Build the package for amd64 and arm64. On a fresh VM (no development
    tools), extract the archive. Run `rollio --help` → prints usage. Run
    `rollio collect -c config.toml` with pseudo devices → works without
    installing any additional dependencies.
- _Packaging — architecture verification_:
  - On an arm64 Jetson: extract the arm64 package. Run `encoder probe` →
    lists NVENC codecs. Run `rollio collect` with NVENC → videos are
    hardware-encoded.

**Checkpoint**: install rollio from the packaged archive on a fresh machine.
Run `rollio setup` with real hardware, `rollio collect` for a 50-episode
session with no crashes or data loss, `rollio replay` on a recorded episode.
Deliberately unplug a camera mid-recording — the UI warns and the system
remains operational.

---

## Sprint Summary

| Sprint | Key Deliverable                         | Human Can Test                                    |
|--------|-----------------------------------------|---------------------------------------------------|
| 0      | Repo scaffolding, shared types          | `cargo build` / `npm build` succeed               |
| 1      | Visualizer + UI skeleton                | Synthetic camera + robot data visible in TUI       |
| 2      | Controller + devices (pseudo + real)     | `rollio collect` with pseudo or RealSense + AIRBOT |
| 3      | Teleop Router + episode lifecycle       | Leader–follower mirroring; start/stop episodes     |
| 4      | Encoder                                 | Recorded episodes produce video files              |
| 5      | Episode Assembler + local Storage       | Valid LeRobot v2.1 episodes on disk                |
| 6      | Setup wizard                            | `rollio setup` generates usable config             |
| 7      | Monitor                                 | Threshold warnings visible in UI                   |
| 8      | Remote storage (HTTP)                   | Episodes uploaded to companion server              |
| 9      | Additional hardware (V4L2, G2/E2)       | Expanded device support, full hardware mix         |
| 10     | Replay + LeRobot v3.0 + mcap            | `rollio replay` drives robots; multi-format output |
| 11     | Hardening + packaging                   | Install from archive, 50-episode stress test       |
