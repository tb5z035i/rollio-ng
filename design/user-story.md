# Rollio User Story

## Overview

Rollio is a CLI-based framework for fast hardware discovery, setup, and data
collection. It targets robotic teleoperation workflows where cameras and robot
arms are recorded into structured episode datasets, with LeRobot v2.1 (default)
and v3.0 as supported output formats.

The framework is organized around three commands: **setup**, **collect**, and
**replay**.

---

## 1. `rollio setup` — Hardware Discovery and Configuration

### 1.1 Device Probing

When the user runs `rollio setup`, the framework automatically probes all
connected devices (cameras, robot arms, and any other supported peripherals).

For each discovered device, an identification mechanism helps the user
physically locate and recognize it:

- **Cameras**: a live preview stream is shown so the user can tell which
  physical camera corresponds to which logical entry.
- **Robots**: LED lights are activated if supported; alternatively the robot is
  put into a free-drag mode so the user can physically wiggle it to identify
  which arm is which.

### 1.2 Device Selection and Channel Configuration

The user selects which discovered devices should participate in data recording.
For each selected device the user also specifies which **channels** to record.
Channels are device-type-specific data streams — for example:

- **Camera channels**: color, depth, infrared (depending on hardware
  capability).
- **Robot channels**: joint position, joint velocity, joint effort/torque,
  end-effector pose, gripper state, etc.

### 1.3 Device Parameters

While configuring each device the user can adjust detailed parameters. These
are per-device settings that affect how data is captured:

- **Camera parameters**: resolution (width × height), framerate (FPS), pixel
  format / frame type (e.g. MJPEG vs YUYV), color space, exposure, white
  balance, etc.
- **Robot parameters**: control frequency, joint limits, communication
  interface settings, etc.
- **Other device parameters**: as defined by the device type's driver.

Parameters have sensible defaults and the user only needs to change what
matters for their use case.

### 1.4 Pairing Strategy

After device selection, the user configures how devices relate to each other
during operation:

- **Initial robot state**: each robot is assigned an initial mode —
  **free-drive** (gravity-compensated, human can drag the arm) or **command
  following** (the arm tracks commands from a leader or a policy). This state
  is intended to be **switchable at runtime** during collection.

- **Following relationships (mimic / teleop pairs)**: the user specifies which
  devices follow which. A typical setup pairs a leader arm (in free-drive, the
  human moves it) with a follower arm (in command following, it mimics the
  leader). The mapping strategy between leader and follower (e.g. direct joint
  mapping, FK/IK-based Cartesian mapping) is also configurable here.

### 1.5 Preview Page

After all configuration is complete, a **preview page** is displayed. This
serves as a final review before persisting the configuration:

- **Camera previews**: all selected cameras display real-time video streams.
- **Sensor readouts**: robot joint states and other sensor values are shown as
  real-time updating bars/gauges, providing immediate visual feedback for the
  user's physical actions (e.g. dragging a robot arm and seeing the joint
  angles change live).
- **Device state**: robots are in their configured initial state (free-drive or
  command following), and teleop pairs are active — the setup behaves exactly
  as it would during actual data collection, except nothing is being recorded.

The preview page is **the same layout** reused during actual collection (see
§2), ensuring what the user sees during setup is what they get during
recording.

**Setup-only capabilities** (not available during collection):

- Alter parameters of any device on the fly (e.g. tweak camera resolution).
- Add or remove devices from the configuration.
- Adjust pairing relationships.

### 1.6 Configuration Output

The setup process produces a **configuration file** (TOML format) that captures
all information necessary for data collection:

- Selected devices and their channels.
- Per-device parameters.
- Pairing strategy and initial robot states.
- Storage backend settings (see §2.3).
- Episode format settings (see §2.4).

### 1.7 Resuming Setup from an Existing Configuration

When `rollio setup` is launched with an existing configuration file (e.g.
`rollio setup -c config.toml`), it:

1. **Validates** the configuration — checks for syntax errors and verifies
   that all referenced devices are physically present and reachable.
2. **Enters the preview page directly**, skipping the discovery and selection
   wizard, allowing the user to review and optionally modify the configuration.

---

## 2. `rollio collect` — Data Collection

### 2.1 Launching Collection

The user runs:

```
rollio collect -c config.toml
```

The framework:

1. Loads and validates the configuration file.
2. Verifies all referenced devices are present and operational.
3. Opens the collection UI — the same general layout as the setup preview page
   (§1.5), with live camera feeds and sensor readout bars.

### 2.2 Episode Lifecycle

Data collection is organized around **episodes**. The user controls episode
boundaries via keyboard shortcuts:

| Action | Description |
|--------|-------------|
| **Start** | Begin recording a new episode. All device data is captured from this point. |
| **Stop** | End the current episode. Recording pauses. |
| **Keep** | After stopping, the user is prompted to keep or discard. Choosing "keep" enqueues the episode for background processing. |
| **Discard** | The stopped episode is thrown away; no data is written. |

**Continuity requirement**: once the user chooses "keep", the episode is
handed off to a background queue for encoding and storage, and the user can
**immediately** start recording the next episode without waiting for the
previous one to finish processing. This zero-downtime transition between
episodes is critical for efficient data collection sessions. The only
acceptable bottleneck is hardware-imposed (e.g. encoder saturation or upload
bandwidth limits).

### 2.3 Storage Backends

The storage backend is configured during setup (§1.6). Two backends are
supported initially:

- **Local storage**: episodes are encoded and written to a local directory.
- **Cloud upload (HTTP)**: episodes are encoded and uploaded to a remote
  endpoint via HTTP.

When HTTP upload is selected during setup, the framework tests endpoint
availability **immediately** (during the setup phase) to catch connectivity or
authentication issues before the user starts a long collection session.

Both backends operate through the background processing queue, so neither
blocks the user from continuing to record.

### 2.4 Episode Format

The output episode format is configurable. **LeRobot v2.1** is the default.
**LeRobot v3.0** is also supported. The format choice is part of the
configuration file.

A typical episode consists of:

- **Video files**: one per camera channel, encoded per the configured codec.
- **Tabular data**: timestamped rows of robot state and action vectors, stored
  as Parquet files.
- **Metadata**: episode-level information (FPS, feature descriptions, frame
  counts, etc.) as required by the chosen LeRobot format version.

---

## 3. `rollio replay` — Trajectory Replay

### 3.1 Purpose

`rollio replay` plays back recorded trajectories on the physical hardware. This
is used for:

- **Validation**: verify that a recorded episode looks correct when executed on
  the real robot.
- **Demonstration**: show a previously recorded behavior to observers or
  collaborators.

### 3.2 How It Works

The user runs:

```
rollio replay -e <episode_path>
```

The framework:

1. Reads the episode data and extracts the **embedded configuration** (see
   §3.3).
2. **Validates** the embedded configuration — checks syntax and verifies that
   all referenced devices are physically present and reachable. Robots must
   support **command following** mode for replay to work.
3. Puts the relevant robots into command following mode.
4. Replays the recorded action trajectories at the original timing, driving
   the robots through the recorded motions.

Camera feeds may optionally be shown during replay for visual confirmation.

### 3.3 Configuration Embedding

The configuration file used during collection is **embedded within each
recorded episode**. This serves two purposes:

- **Reproducibility**: the episode carries all the information about how it was
  recorded — which devices, what parameters, what pairing strategy.
- **Replay enablement**: `rollio replay` can reconstruct the device setup
  from the episode alone, without requiring the user to separately provide a
  configuration file.

The embedding mechanism stores the full configuration content as part of the
episode metadata, alongside the standard LeRobot format metadata.

**Note**: device ID remapping for replay (e.g. replaying on a replacement
robot with a different serial/ID) is deferred to a later iteration. Initially,
device IDs in the embedded config must match the connected hardware exactly.

---

## Non-Functional Requirements

### Latency and Throughput

- Camera preview and sensor readouts must be **real-time** (frame-rate
  latency, not batch latency).
- Episode encoding and upload must not block the recording loop. The
  background queue absorbs bursts; backpressure is only applied when hardware
  limits are reached.

### Extensibility

- New device types (cameras, robots, other sensors) should be addable without
  modifying the core runtime. The legacy codebase uses a factory/registry
  pattern for this, which should be preserved or improved.
- New storage backends and episode formats should be pluggable.

### Robustness

- Device disconnection during collection should be handled gracefully (e.g.
  warning, not a crash).
- Configuration validation catches problems early — during setup or at
  collection launch — rather than mid-recording.

### Usability

- The setup wizard should be approachable for users who are not deeply
  technical. Live previews and physical identification (LEDs, free-drag)
  reduce guesswork.
- Keyboard-driven episode control allows hands-free operation once the session
  is running (important when the user's hands are on robot arms).
