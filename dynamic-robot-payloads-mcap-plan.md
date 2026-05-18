# Dynamic Robot Payloads and MCAP Schema Resolution

## Summary

- Migrate the whole v2 robot data-flow bus from fixed max-size structs to semantic dynamic slice payloads across assembler, teleop-router, visualizer, pseudo/AIRBOT/Nero, and LeRobot episode assembler.
- Do not add a separate runtime "type id" field. The service topic plus generated runtime config identifies the semantic stream; the iceoryx2 payload/user-header type identifies the wire contract.
- Keep camera data unchanged: raw/encoded camera streams already use dynamic `[u8]` payloads with user headers.
- Do not fake kp/kd MCAP encoding now. Preserve kp/kd on the command bus, but defer MCAP gain topics until a valid gain `.fbs` exists.

## Interface Changes

- Add shared robot sample wire types in `rollio-types::messages`:
  - `SampleHeader { timestamp_us: u64 }`, `#[repr(C)]`, used as iceoryx2 user header.
  - `MitCommandElement { position: f64, velocity: f64, effort: f64, kp: f64, kd: f64 }`, `#[repr(C)]`, used as a dynamic slice element.
- Replace active v2 robot services:
  - Joint/parallel position, velocity, effort, end-effector pose, and end-pose commands: `publish_subscribe::<[f64]>().user_header::<SampleHeader>()`.
  - Joint/parallel MIT commands: `publish_subscribe::<[MitCommandElement]>().user_header::<SampleHeader>()`.
- Keep old `JointVector15`, `ParallelVector2`, `Pose7`, `JointMitCommand15`, and `ParallelMitCommand2` only as deprecated legacy definitions during this change; no v2 runtime path should open services with them.
- Python mirrors:
  - Add `SampleHeader` and `MitCommandElement` ctypes structures.
  - Use `iox2.Slice[ctypes.c_double]` and `iox2.Slice[MitCommandElement]`.
  - Update send/drain helpers to copy slice payloads plus user headers, not fixed structs.

## Implementation Changes

- Runtime config generation stays in `ProjectConfig::assembler_runtime_config_v2`.
  - For each observation/action, convey: `channel_id`, structured `device_name` and `channel_type`, topic, semantic kind, expected length, and transport payload kind.
  - Do not convey MCAP schema names in config; assembler owns semantic-to-schema resolution.
- Add a small shared resolver for robot stream contracts:
  - `RobotStateKind::{JointPosition, JointVelocity, JointEffort, ParallelPosition, ParallelVelocity, ParallelEffort}` -> `F64Vector`.
  - `RobotStateKind::EndEffectorPose` and `RobotCommandKind::EndPose` -> `F64Vector` with exact length 7.
  - `RobotCommandKind::{JointPosition, ParallelPosition}` -> `F64Vector`.
  - `RobotCommandKind::{JointMit, ParallelMit}` -> `MitCommandElementVector`.
  - `EndEffectorTwist/Wrench` remain unsupported for MCAP until a valid schema exists; config validation should reject recording them for MCAP.
- Update all v2 producers/consumers to the new contracts:
  - `episode-mcap`, `episode-lerobot`, `teleop-router`, `visualizer`, `robots/pseudo`, `robots/airbot_play_rust`, and `robots/nero`.
  - Use `initial_max_slice_len(expected_len)` for publishers.
  - Treat length mismatch as a contract error: skip the sample/command and log; never truncate or pad silently.
- Update MCAP writing:
  - Cameras remain `CompressedVideo`.
  - Joint/parallel position -> `foxglove.JointStates.position`.
  - Joint/parallel velocity -> `foxglove.JointStates.velocity`.
  - Joint/parallel effort -> `foxglove.JointStates.effort`.
  - MIT commands -> `foxglove.JointStates` with position/velocity/effort populated; kp/kd retained only in the bus payload for now.
  - End-effector pose/end-pose -> `foxglove.PoseInFrame`; add `SchemaType::PoseInFrame` and encoder support.
  - Schema resolution is based on semantic kind, not channel name or driver.

## Test Plan

- Rust unit tests:
  - Verify new `SampleHeader` and `MitCommandElement` layout/type names.
  - Verify resolver mapping from state/command kinds to transport payload kind and MCAP schema.
  - Verify MCAP encoders populate JointStates fields correctly for position, velocity, effort, and MIT.
  - Verify PoseInFrame encoding for 7-element pose vectors.
- Integration tests:
  - Update teleop-router tests to publish/consume `[f64] + SampleHeader` and `[MitCommandElement] + SampleHeader`.
  - Update episode-mcap roundtrip tests to cover dynamic vector observations, MIT actions, and pose channels.
  - Update episode-lerobot loss/buffer tests to use dynamic `[f64]`.
- Python tests:
  - Update Nero ctypes layout tests for `SampleHeader` and `MitCommandElement`.
  - Update runtime mode tests to use dynamic-slice helper objects instead of fixed structs.
- Run:
  - `cargo test -p rollio-types -p teleop-router -p rollio-episode-mcap -p rollio-episode-lerobot`
  - Nero Python tests that do not require a live iceoryx2 wheel.
  - Full `make rust-test` if the local toolchain is ready.

## Assumptions

- This is a coordinated v2 data-flow migration, not dual support. Old and new robot data-flow services are intentionally incompatible at iceoryx2 type-negotiation level.
- Control-flow messages stay unchanged: `ControlEvent`, `DeviceChannelMode`, episode/storage/backpressure messages, and legacy test-only `RobotState`/`RobotCommand`.
- kp/kd MCAP side-channels are postponed until a valid gain schema is added to `mcap_spec`; this plan leaves a narrow encoder extension point but does not invent an invalid schema now.
