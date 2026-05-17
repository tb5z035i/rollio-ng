---
name: Sensor Channel Kind
overview: Add a third `DeviceType::Sensor` channel kind alongside Camera and Robot. Initial sensor variants are `ImuAccelGyro` (6 floats fixed) and `TactilePointCloud2` (N x 6 floats, N driver-reported). Plumbed through config validation, iceoryx2 bus topics, pseudo reference driver, LeRobot dataset writer, and the web UI device sidebar.
todos:
  - id: types-and-validation
    content: Add `DeviceType::Sensor`, `SensorStateKind`, `ChannelStateKind` union, `sample_rate_hz`, validation arms, and `resolved_sensor_channels` in rollio-types; add three config tests
    status: pending
  - id: bus-and-message-envelope
    content: Add `SensorFrameHeader` in rollio-types/messages.rs and `channel_sample_service_name` + dynamic-payload service helpers in rollio-bus
    status: pending
  - id: pseudo-driver-sensor-channel
    content: Extend `robots/pseudo` with `run_sensor_channel` that publishes `ImuAccelGyro` and `TactilePointCloud2` synthetic samples and reports `shape_hints` from `query --json`
    status: pending
  - id: controller-spawn-and-wizard
    content: Update `controller` (device_query / setup wizard / collect integration test) to surface sensor channels through `query --json` and the interactive setup
    status: pending
  - id: lerobot-assembler-columns
    content: Extend `episode-lerobot` to write sensor observation columns under `observation.sensor.{channel}.{kind}` with multi-dimensional shapes and subscribe to `samples/{kind}` topics in the assembler
    status: pending
  - id: web-ui-and-visualizer-placeholder
    content: Add a `Sensors` section to the web UI device sidebar, declare protocol kind 0x04 as a sensor passthrough placeholder, and reserve the visualizer subscription path without implementing decode
    status: pending
  - id: example-config-and-docs
    content: Update `config/config.example.toml` with sensor blocks and refresh `design/device-as-binaries.md` to reflect that sensor is no longer "future work"
    status: pending
isProject: true
---

# Sensor Channel Kind

## Context

The framework currently supports two channel kinds — `Camera` (publishes camera frames) and `Robot` (publishes fixed-length float vectors on `states/{kind}` topics). The roadmap note at `design/device-as-binaries.md:11-12` reserves a sensor slot ("imu can be added in the future") but no code exists.

Confirmed scope:
- **IMU**: a single combined `ImuAccelGyro` variant, layout `[ax, ay, az, gx, gy, gz]`, 6 floats. Combined to keep accel and gyro on the same iceoryx2 topic so consumers cannot observe cross-topic skew between two pieces of the same IMU packet.
- **Tactile**: `TactilePointCloud2`, layout `[x, y, z, fx, fy, fz]` per point, N points per frame. N is fixed for the lifetime of a channel and reported by the driver via `query --json`.
- End-to-end: config -> bus -> pseudo reference driver -> assembler writes LeRobot observation columns -> web UI sidebar lists the sensors.
- Bus envelope: a single `SensorFrameHeader` + dynamic payload (mirrors camera frame transport), with self-describing `dtype`, `ndim`, `shape[6]`.
- Channel field naming: reuse `publish_states` / `recorded_states`; introduce a new `sample_rate_hz` (replaces `control_frequency_hz` for sensor channels).
- UI: device sidebar only — name, sample rate, online indicator. No charts. Visualizer reserves WebSocket binary kind `0x04` for future sensor passthrough but does not implement decode.

## Design Decisions

1. **`SensorStateKind` is parallel to `RobotStateKind`, not merged.** The `publish_states` / `recorded_states` fields widen from `Vec<RobotStateKind>` to `Vec<ChannelStateKind>`, where `ChannelStateKind` is a `#[serde(untagged)]` union `Robot(RobotStateKind) | Sensor(SensorStateKind)`. Downstream call sites narrow with `.as_robot()` / `.as_sensor()` at the boundary; the validator enforces that the channel's `kind` matches the variant family. This keeps teleop and dataset code that depends on the typed `RobotStateKind` unchanged.

2. **New bus topic prefix `samples/`.** Sensor channels publish on `{bus_root}/{channel_type}/samples/{sensor_kind}` (not `states/`). The iceoryx2 service type is a dynamic-payload pub-sub with `SensorFrameHeader` user header — different from the fixed-size `JointVector15` / `Pose7` services on `states/`. Sharing the path would conflict.

3. **Self-describing payloads.** `SensorFrameHeader` carries `dtype`, `ndim`, `shape[6]`. For `ImuAccelGyro` the layout is `ndim=1, shape=[6]`. For `TactilePointCloud2` it is `ndim=2, shape=[N_points, 6]`. The assembler reads shape from `resolved_sensor_channels().shape_hints` (filled at `query --json` time) so the LeRobot `info.json` features carry the correct shape and per-frame samples have a stable width — no ragged-array support needed in the Parquet writer.

## Implementation Outline

### A. `rollio-types`

- `src/config.rs`:
  - Add `Sensor` variant to `DeviceType`.
  - Add `SensorStateKind` enum (`ImuAccelGyro`, `TactilePointCloud2`) with `topic_suffix()`, `fixed_value_len() -> Option<u32>`, `is_variable_shape()`.
  - Add `ChannelStateKind` untagged union with `From<RobotStateKind>` / `From<SensorStateKind>` / `as_robot()` / `as_sensor()` / `topic_suffix()`.
  - Widen `DeviceChannelConfigV2.publish_states` and `recorded_states` to `Vec<ChannelStateKind>`.
  - Add `sample_rate_hz: Option<f64>` to `DeviceChannelConfigV2`.
  - Extend `DeviceChannelConfigV2::validate` with a `Sensor` arm and tighten the existing `Robot` arm to reject sensor variants in its `publish_states`.
  - Add `resolved_sensor_channels()` returning `Vec<ResolvedSensorChannel>` with derived sample topic names, recorded_states, sample_rate_hz, shape_hints.
- `src/messages.rs`: add `SensorFrameHeader { timestamp_us, sample_index, sensor_kind, dtype, ndim, _pad, shape[6] }`.
- `src/schema.rs`: extend `kind` enum docs, scope `sample_rate_hz` to `["sensor"]`, list sensor state kinds.
- `tests/config.rs`: three new cases — happy parse, robot-kind-with-sensor-state rejection, missing-sample_rate_hz rejection.

### B. `rollio-bus`

- `channel_sample_service_name(bus_root, channel_type, sensor_kind) -> String`.
- iceoryx2 dynamic-payload service builder for sensor samples, `SensorFrameHeader` user header, payload `[u8]`, dedicated `SAMPLE_BUFFER` constant (default 256).

### C. `controller`

- `device_query.rs`: deserialize `supported_sensor_kinds` and `shape_hints` per sensor channel from `query --json`. Inject shape_hints into the in-memory `resolved_sensor_channels()`.
- `runtime_plan.rs` / `discovery.rs`: spawn path is driver-name based, no change needed for sensor.
- `setup.rs`: wizard handles `kind = "sensor"` — prompts `sample_rate_hz` and picks state kinds from the driver-reported list.
- `collect.rs`: add an integration test case that exercises a sensor channel end-to-end.

### D. `robots/pseudo`

- `src/bin/device.rs`:
  - Add `DeviceType::Sensor => run_sensor_channel(...)` to the exhaustive channel-kind match.
  - Implement `run_sensor_channel`: dynamic-payload service per published kind, IMU emits synthetic `[ax, ay, az, gx, gy, gz]`, tactile emits a `[N_points, 6]` array where `N_points` defaults to 256 and is overridable via `[devices.extra].tactile_point_count`. Sample period from `sample_rate_hz`.
  - Extend `query --json` to declare `supported_sensor_kinds` and `shape_hints` for the synthetic sensor channels.

### E. `episode-lerobot`

- `src/lerobot.rs`:
  - Observation column key: `observation.sensor.{channel_id}.{sensor_kind}` for sensor channels (robot stays `observation.state.{channel_id}.{kind}`).
  - Allow multi-dimensional `shape` in feature metadata (the current writer hard-codes 1D).
  - Sample sampler reads bytes from `SensorFrameHeader`-tagged frames and flattens to `Vec<f64>` with fixed channel-lifetime width.
  - Assembler subscribes to `samples/{kind}` topics for sensor channels and decodes via `SensorFrameHeader` + payload bytes.

### F. Web UI

- `ui/web/src/lib/protocol.ts`: add `FRAME_TYPE_SENSOR_SAMPLE = 0x04`, extend `StreamInfoMessage` with `sensors?: StreamInfoSensor[]`, add a no-op branch in `parseBinaryMessage` for 0x04.
- `ui/web/src/components/InfoPanel.tsx`: render a `Sensors` section after Cameras / Robots, each row showing name, sample rate, recorded states count, online indicator.
- `web-gateway`: include `sensors` in the `stream_info` message it broadcasts.

### G. Visualizer placeholder

- `visualizer`: subscribe to `samples/{kind}` and forward as binary kind `0x04` to WebSocket clients with a TODO comment — no decode, no chart. Reserves the seam for a follow-up sprint.

### H. Config example and roadmap doc

- `config/config.example.toml`: append an `imu` device (driver = `pseudo`, channel `imu/sensor` with `publish_states = ["imu_accel_gyro"]`) and a `tactile_left` device (channel `tactile/sensor` with `publish_states = ["tactile_point_cloud2"]`, `tactile_point_count = 256` in `[devices.extra]`).
- `design/device-as-binaries.md`: remove the "imu can be added in the future" note; replace with a short summary of the initial `SensorStateKind` variants.

## PR Sequencing

1. **PR1 — types + validation**: section A only. Land in `rollio-types` independent of runtime.
2. **PR2 — bus + envelope**: section B + `SensorFrameHeader`.
3. **PR3 — pseudo driver + controller**: sections D + C.
4. **PR4 — assembler**: section E. Verify Parquet column landing.
5. **PR5 — web UI + visualizer placeholder + example config**: sections F + G + H.

Each PR is independently reviewable.

## Verification

- `cargo test -p rollio-types -- config` passes including the three new sensor cases.
- `cargo build --workspace` succeeds — Rust exhaustive `match DeviceType { ... }` catches every uncovered site.
- `cargo run -p rollio -- collect --config config/config.example.toml --max-episodes 1` boots the synthetic `imu` and `tactile_left` devices alongside the existing camera and arm fixtures.
- Episode artifacts under `./output/<episode-id>`:
  - `data/chunk-000/episode_000000.parquet` contains `observation.sensor.imu.imu_accel_gyro` (shape `[6]`) and `observation.sensor.tactile_left.tactile_point_cloud2` (shape `[256, 6]`).
  - `meta/info.json` `features` block reports matching shapes and `dtype: "float32"`.
- Web UI at `localhost:3000` shows a `Sensors` section listing `imu @ 200 Hz` and `tactile_left @ 60 Hz`.
- Visualizer does not draw sensor data — the 0x04 subscription is a placeholder only.
