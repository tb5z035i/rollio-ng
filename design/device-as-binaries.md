# Device As Binaries

The current device integration is too coupled to framework code. The framework
should primarily understand the shared iceoryx bus contract, while device
families should describe their own capabilities and runtime behavior through a
common executable interface.

## Core Model

- A physical device can expose multiple channels.
- Each channel has a `kind`, such as `camera` or `robot`. `imu` can be added in
  the future.
- Channels use a fixed `channel_type` vocabulary defined by the driver family,
  not a user-defined runtime name.
  - AIRBOT family examples: `arm`, `g2`, `e2`
  - RealSense family examples: `color`, `depth`, `infrared`
- A concrete runtime channel is identified by `(device_id, channel_type)`.
- A driver family may define several possible channel types, but a specific
  physical device instance only exposes the subset that is actually available.

Examples:

- AIRBOT Play with a mounted E2 is one physical device with channels
  `arm` and `e2`.
- AIRBOT Play with a mounted G2 is one physical device with channels
  `arm` and `g2`.
- A RealSense camera is one physical device with channels `color`, `depth`,
  and `infrared`.

Each channel has its own mode:

- Camera channels: `enabled`, `disabled`
- Robot channels: `free-drive`, `command-following`, `disabled`

## Driver Discovery

- Device support is provided by executables.
- All device executables follow the unified `rollio-device-{driver}` naming
  convention. The legacy `rollio-camera-*` / `rollio-robot-*` split is gone:
  a single device may expose camera channels, robot channels, or a mix.
- The controller discovers devices via a hybrid model:
  - **Explicit registry** in [`controller/src/discovery.rs`](../controller/src/discovery.rs)
    for in-tree drivers (`known_device_executables()`); these are always
    probed even when the workspace `target/debug` builds aren't on `$PATH`.
  - **PATH scan** for any executable whose filename starts with
    `rollio-device-`. Third-party drivers installed via `pip install` /
    `cargo install` are picked up automatically with no framework changes.
  - `rollio-device-pseudo` is excluded from both paths and surfaces only
    when the controller is invoked with `--sim-pseudo N`.
- The framework keeps **no per-driver tables** — all capability, naming,
  pairing, and pixel-format metadata flows through the device's own
  `query --json` response.

## Executable Contract

Each device executable should support the following subcommands.

### 1. `probe`

- Returns a human-friendly listing of connected devices.
- With `--json`, returns a JSON list of discovered device ids.
- Device id format is vendor-defined.

Current examples:

- AIRBOT Play: serial number
- RealSense: serial number
- V4L2: bus info such as `usb-0000:af:00.0-4`
- Mock camera: configured synthetic id

### 2. `validate`

- Validates that a device id still exists and that the requested channel types
  are still available.
- Returns normal human-friendly output and process success or failure for
  programmatic use.
- With `--json`, returns a structured validation report.

### 3. `query`

- Replaces the current `capabilities` contract.
- Returns everything setup and collect need in order to avoid framework-owned
  device heuristics.
- Should produce human-friendly output by default and a JSON structure with
  `--json`.

For each discovered device, `query` should report:

- `device_class`, such as `airbot-play`, `realsense`, `v4l2`, `mock-camera`
- `device_label`, such as `AIRBOT Play`
- `id`
- `channels`
- optional device-level metadata

For each channel, `query` should report:

- `channel_type`
- `kind`
- `available`
- supported modes
- camera profiles, when `kind = camera`
- `dof`, when `kind = robot`
- supported state topics, when `kind = robot`
- supported command topics, when `kind = robot`
- `supports_fk`, when `kind = robot`
- `supports_ik`, when `kind = robot`
- default control frequency
- default command parameters such as `kp` and `kd`
- optional channel-level metadata

For direct-joint teleoperation, robot channels should also report directional
compatibility metadata:

- `direct_joint_compatibility.can_lead`
- `direct_joint_compatibility.can_follow`

Each list contains objects of the form:

```json
{ "driver": "airbot-play", "channel_type": "arm" }
```

This metadata only answers whether two channel families are compatible for
direct-joint teleoperation. The actual state and command topic choice remains a
framework-level pairing decision.

### 4. `run`

- Accepts `--config <path>` or `--config-inline <toml>`.
- Accepts `--dry-run` to validate the same config without starting the runtime.
- Receives one physical-device config, not the entire project config.
- The project-level controller config should store one such device block per
  physical device and pass the extracted block into the device binary.

### Recommended per-device `run` config

```toml
name = "airbot_left"
driver = "airbot-play"
id = "PZ60C02603000894"
bus_root = "airbot_left"

[extra]
transport = "can"
interface = "can0"

[[channels]]
channel_type = "arm"
kind = "robot"
enabled = true
mode = "free-drive"
publish_states = ["joint_position", "joint_velocity", "joint_effort", "end_effector_pose"]
control_frequency_hz = 250.0

[channels.command_defaults]
joint_mit_kp = [40.0, 40.0, 40.0, 25.0, 25.0, 10.0]
joint_mit_kd = [1.2, 1.2, 1.2, 0.8, 0.8, 0.3]

[[channels]]
channel_type = "e2"
kind = "robot"
enabled = true
mode = "command-following"
publish_states = ["parallel_position"]

[[channels]]
channel_type = "color"
kind = "camera"
enabled = true
profile = { width = 1280, height = 720, fps = 30, pixel_format = "rgb24" }
```

### Recommended `query --json` shape

```json
{
  "driver": "airbot-play",
  "devices": [
    {
      "id": "PZ60C02603000894",
      "device_class": "airbot-play",
      "device_label": "AIRBOT Play",
      "optional_info": {},
      "channels": [
        {
          "channel_type": "arm",
          "kind": "robot",
          "available": true,
          "modes": ["free-drive", "command-following", "disabled"],
          "supported_states": ["joint_position", "joint_velocity", "joint_effort", "end_effector_pose"],
          "supported_commands": ["joint_position", "joint_mit", "end_pose"],
          "supports_fk": true,
          "supports_ik": true,
          "dof": 6,
          "default_control_frequency_hz": 250.0,
          "direct_joint_compatibility": {
            "can_lead": [
              { "driver": "airbot-play", "channel_type": "arm" }
            ],
            "can_follow": [
              { "driver": "airbot-play", "channel_type": "arm" }
            ]
          },
          "defaults": {
            "joint_mit_kp": [40.0, 40.0, 40.0, 25.0, 25.0, 10.0],
            "joint_mit_kd": [1.2, 1.2, 1.2, 0.8, 0.8, 0.3]
          },
          "optional_info": {}
        },
        {
          "channel_type": "color",
          "kind": "camera",
          "available": true,
          "profiles": [
            { "width": 1280, "height": 720, "fps": 30, "pixel_format": "rgb24" }
          ],
          "optional_info": {}
        }
      ]
    }
  ]
}
```

## Runtime Bus Contract

For device data, use hierarchical topics rooted at `bus_root`.

All device-data timestamps should use milliseconds.

- `{bus_root}/info`
- `{bus_root}/shutdown`
- `{bus_root}/{channel_type}/status`
- `{bus_root}/{channel_type}/info/mode`
- `{bus_root}/{channel_type}/control/mode`
- `{bus_root}/{channel_type}/info/profile`
- `{bus_root}/{channel_type}/control/profile`
- `{bus_root}/{channel_type}/frames`
- `{bus_root}/{channel_type}/states/joint_position`
- `{bus_root}/{channel_type}/states/joint_velocity`
- `{bus_root}/{channel_type}/states/joint_effort`
- `{bus_root}/{channel_type}/states/end_effector_pose`
- `{bus_root}/{channel_type}/states/end_effector_twist`
- `{bus_root}/{channel_type}/states/end_effector_wrench`
- `{bus_root}/{channel_type}/states/parallel_position`
- `{bus_root}/{channel_type}/states/parallel_velocity`
- `{bus_root}/{channel_type}/states/parallel_effort`
- `{bus_root}/{channel_type}/commands/joint_position`
- `{bus_root}/{channel_type}/commands/joint_mit`
- `{bus_root}/{channel_type}/commands/end_pose`
- `{bus_root}/{channel_type}/commands/parallel_position`
- `{bus_root}/{channel_type}/commands/parallel_mit`

Camera frames should keep the existing zero-copy shape:

- `CameraFrameHeader { timestamp_us, ... } + [u8]`

Robot payloads should use bounded types instead of untyped variable-length
arrays. Proposed bounds:

- `MAX_DOF = 15`
- `MAX_PARALLEL = 2`

Suggested payload families:

- `JointVector15 { timestamp_us, len, values[15] }`
- `ParallelVector2 { timestamp_us, len, values[2] }`
- `Pose7 { timestamp_us, xyz_xyzw[7] }`
- `JointMitCommand15 { timestamp_us, len, position[15], velocity[15], effort[15], kp[15], kd[15] }`
- `ParallelMitCommand2 { timestamp_us, len, position[2], velocity[2], effort[2], kp[2], kd[2] }`

## Scope Note

Changing the non-device control plane is intentionally avoided in the first
migration phase, but it is not forbidden if later implementation work reveals a
clear simplification or correctness benefit.
