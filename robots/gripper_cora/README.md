# gripper-cora

rollio device driver: subscribes to a `sensor_msgs::msg::JointState` Cora
topic, picks one named joint, and publishes its position / velocity / effort
as `JointVector15` (slot 0 set, slots 1–14 zero) on the rollio iceoryx2 bus.

Binary name: `rollio-device-gripper-cora`. dof is fixed to 1 — for multi-joint
arms, copy this crate, change `joint_name` to a `joint_order: Vec<String>`,
and relax the dof check to `<= 15`. CLI mirrors the rollio device contract
(`probe / query / validate / run`).

## Cora SDK discovery

See `sensors/imu_cora/README.md` — the discovery + RPATH logic is identical
(env var `CORA_SDK_ROOT`, arch-specific overrides, or default CMake search).

Local dev:

```bash
export CORA_SDK_ROOT=$(pwd)/examples/cora_sdk/cora_x86_64
cargo build -p gripper-cora
```

On non-Linux hosts `build.rs` skips cmake/link; `cargo check -p gripper-cora`
works on macOS dev machines.

## Configuration

```toml
[[devices]]
name = "gripper_right"
driver = "gripper-cora"
id = "gripper_cora_0"
bus_root = "gripper_right"
[devices.extra]
cora_domain_id = 0
cora_participant_name = "rollio_gripper_right"
cora_use_shared_memory = true
cora_use_udp = true
cora_callback_threads = 2

[[devices.channels]]
channel_type = "gripper"
kind = "robot"
enabled = true
dof = 1
publish_states = ["joint_position", "joint_velocity", "joint_effort"]
[devices.channels.extra]
cora_topic = "rt/gripper/right/state"   # required
cora_qos = "reliable"                   # optional; default "reliable"
joint_name = "gripper_right_finger"     # required; matches one entry in JointState.name[]
```

Per `publish_states` entry, the driver opens a separate iceoryx2 publisher on
`<bus_root>/<channel_type>/states/<state_kind>` with `JointVector15` payload.

## CLI

```bash
rollio-device-gripper-cora probe --json
rollio-device-gripper-cora query gripper_cora_0 --json
rollio-device-gripper-cora validate gripper_cora_0 --channel-type gripper
rollio-device-gripper-cora run --config-inline "$(cat my.toml)"
```

`run` initialises the Cora DDS participant, opens publishers for each requested
state kind, subscribes to the configured JointState topic, and forwards every
message it can resolve: the first message containing `joint_name` in `name[]`
caches the index for fast subsequent lookups. If a later message's `name[]` is
reshuffled, the index is rebuilt by name.

If the requested joint is **never** present, every message is dropped and the
log shows a single warning until the publisher gets a message that does match.

SIGINT/SIGTERM or `ControlEvent::Shutdown` on `control/events` cleanly tears
the bridge and publishers down.

## Verification

```bash
CORA_SDK_ROOT=... cargo build -p gripper-cora
# Cora talker on rt/gripper/right/state publishes name=[gripper_right_finger],
# position=[0.42], velocity=[0.0], effort=[1.5]:
cargo run -p bus-tap -- --channel gripper_right/gripper --kind state
# After `rollio collect`, expect Parquet column observation.state.gripper.joint_position
# shape [15], row 0 = 0.42, slots 1..15 = 0.
```
