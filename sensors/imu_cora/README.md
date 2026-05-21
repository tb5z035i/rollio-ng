# imu-cora

rollio device driver: subscribes to a `sensor_msgs::msg::Imu` Cora topic and
publishes `SensorStateKind::ImuAccelGyro` samples (6 × f32, `[ax, ay, az, gx,
gy, gz]`) on the rollio iceoryx2 bus.

Binary name: `rollio-device-imu-cora`. The controller treats it like any other
rollio device driver — `probe / query / validate / run` subcommands.

## Cora SDK discovery

The crate does not vendor the Cora SDK. `build.rs` resolves the install
location at compile time, in this order:

1. `CORA_SDK_ROOT` — generic override.
2. `CORA_SDK_X86_64_ROOT` / `CORA_SDK_AARCH64_ROOT` — per-target overrides for CI / cross builds.
3. CMake's default `find_package(cora)` search path (e.g. `/usr/local`).

Nothing in this crate's build script points at the in-repo
`examples/cora_sdk/` directory — that path is reference-only. Local dev:

```bash
export CORA_SDK_ROOT=$(pwd)/examples/cora_sdk/cora_x86_64    # or cora_aarch64
cargo build -p imu-cora
```

On non-Linux hosts (macOS dev workstations) `build.rs` skips the cmake +
link steps and just runs bindgen, so `cargo check -p imu-cora` works. A
functional binary requires Linux + the SDK.

## Runtime library lookup (RPATH)

`build.rs` bakes an absolute `-Wl,-rpath,$CORA_SDK_ROOT/lib` into the binary so
development workflows can run without `LD_LIBRARY_PATH` gymnastics. The Cora
SDK's own libraries use `$ORIGIN`, so they resolve their internal Fast-DDS /
FastCDR dependencies automatically once the binary finds
`libcora_framework.so`.

For packaging (deb / image), override at build time:

```bash
CORA_SDK_ROOT=/build/cora_sdk \
CORA_SDK_RUNTIME_RPATH='$ORIGIN/../lib/cora' \
cargo build --release -p imu-cora
```

…and copy the SDK's `lib/` contents to `<install_prefix>/lib/cora/` in the
final artifact.

## Configuration

```toml
[[devices]]
name = "imu_head"
driver = "imu-cora"
id = "imu_cora_0"
bus_root = "imu_head"
[devices.extra]
cora_domain_id = 0
cora_participant_name = "rollio_imu_head"
cora_use_shared_memory = true
cora_use_udp = true
cora_callback_threads = 2

[[devices.channels]]
channel_type = "imu"               # must match what `query --json` reports
kind = "sensor"
enabled = true
sample_rate_hz = 200               # nominal; not enforced — Cora drives the rate
publish_states = ["imu_accel_gyro"]
[devices.channels.extra]
cora_topic = "rt/imu/head/data"    # required
cora_qos = "reliable"              # optional; "reliable" | "best_effort"
```

`device.extra.cora_*` defaults: domain_id=0, participant_name=`rollio_<device.name>`,
use_shared_memory=true, use_udp=true, callback_threads=2.

## CLI

```bash
rollio-device-imu-cora probe --json
rollio-device-imu-cora query imu_cora_0 --json
rollio-device-imu-cora validate imu_cora_0 --channel-type imu --json
rollio-device-imu-cora run --config-inline "$(cat my_device.toml)"
```

`probe --json` is a no-op (returns `[]`) — cora-* drivers are static-config; the
controller never auto-spawns one without an entry in `config.toml`.

`query --json <id>` reports `kind=sensor`, `channel_type=imu`,
`supported_sensor_kinds=["imu_accel_gyro"]`, `sensor_shape_hints={imu_accel_gyro:[6]}`.

`run --config-inline <toml>` is the main entry point used by the controller. It:

1. Initialises the Cora DDS participant.
2. Starts the SDK CallbackExecutor (worker threads dispatch Imu messages).
3. Opens one iceoryx2 publisher per enabled channel on service
   `<bus_root>/<channel_type>/samples/imu_accel_gyro` with a `SensorFrameHeader`
   user header.
4. Subscribes to the configured Cora topic; every callback drops an
   `ImuPublish` into a per-channel `crossbeam_channel`. A dedicated publisher
   thread drains it and emits one `SensorFrameHeader { sensor_kind:
   ImuAccelGyro, dtype: F32, ndim: 1, shape: [6, …] }` sample plus 6×f32 (LE)
   payload bytes.
5. On SIGINT/SIGTERM or `ControlEvent::Shutdown` on `control/events`, drops the
   bridge (which stops the executor and shuts the DDS participant down) and
   joins all publisher threads.

## Building & testing

```bash
# Linux with Cora SDK installed/exported:
CORA_SDK_ROOT=... cargo build -p imu-cora
CORA_SDK_ROOT=... cargo test -p imu-cora     # currently no smoke tests; uses cargo check

# macOS dev: only cargo check works
cargo check -p imu-cora
```

End-to-end test against a real Cora producer: build the SDK's
`examples/cora_sdk/cora_x86_64/examples/cpp/talker_node.cpp` adapted for
`sensor_msgs::msg::Imu` on `rt/imu/head/data`, then start
`rollio-device-imu-cora` with a matching config and observe samples on the
iceoryx2 bus via `bus-tap`:

```bash
cargo run -p bus-tap -- --channel imu_head/imu --kind sample
```
