# tactile-cora

rollio device driver: subscribes to a `sensor_msgs::msg::PointCloud2` Cora topic
and publishes `SensorStateKind::TactilePointCloud2` samples (shape `[N, 6]`,
f32 little-endian) on the rollio iceoryx2 bus.

Binary name: `rollio-device-tactile-cora`. CLI mirrors the rollio device
contract (`probe / query / validate / run`).

## Cora SDK discovery

See `sensors/imu_cora/README.md` — the discovery + RPATH logic is identical
(env var `CORA_SDK_ROOT`, arch-specific overrides, or default CMake search).

Local dev:

```bash
export CORA_SDK_ROOT=$(pwd)/examples/cora_sdk/cora_x86_64
cargo build -p tactile-cora
```

On non-Linux hosts `build.rs` skips cmake/link; `cargo check -p tactile-cora`
works for IDE/CI on macOS dev machines.

## Configuration

```toml
[[devices]]
name = "tactile_left"
driver = "tactile-cora"
id = "tactile_cora_0"
bus_root = "tactile_left"
[devices.extra]
cora_domain_id = 0
cora_participant_name = "rollio_tactile_left"
cora_use_shared_memory = true
cora_use_udp = true
cora_callback_threads = 2

[[devices.channels]]
channel_type = "tactile"
kind = "sensor"
enabled = true
sample_rate_hz = 30
publish_states = ["tactile_point_cloud2"]
[devices.channels.extra]
cora_topic = "rt/tactile/left/points"   # required
cora_qos = "best_effort"                # optional; default "reliable"
tactile_point_count = 1024              # required; fixed for channel lifetime
# Six slot mappings into [x, y, z, fx, fy, fz]. Empty "" leaves that slot at 0.
pointcloud_field_map = ["x","y","z","fx","fy","fz"]
```

Strict Phase 1 requirements (messages outside these constraints are dropped,
counted, and warned about once):

* every named field in `pointcloud_field_map` has `datatype = FLOAT32` (code 7);
* `is_bigendian = false`;
* `width * height == tactile_point_count`.

If a slot's name is empty (`""`), the slot stays at 0.0 — useful for sources
that only publish `[x, y, z]` while the rollio side expects six channels.

## CLI

```bash
rollio-device-tactile-cora probe --json                         # [] (static-config)
rollio-device-tactile-cora query tactile_cora_0 --json          # kind=sensor, tactile_point_cloud2
rollio-device-tactile-cora validate tactile_cora_0 --channel-type tactile
rollio-device-tactile-cora run --config-inline "$(cat my.toml)"
```

`run` opens an iceoryx2 publisher on
`<bus_root>/<channel_type>/samples/tactile_point_cloud2` (dynamic payload `[u8]`,
user header `SensorFrameHeader`), subscribes to the configured Cora topic, and
for every accepted message emits one sample with
`SensorFrameHeader { ndim: 2, shape: [N, 6, …] }` plus `6 * N * 4` bytes of
little-endian float32 payload (point-major, then slot-major). SIGINT/SIGTERM or
`ControlEvent::Shutdown` on `control/events` cleanly tears it all down.

## Verification

```bash
CORA_SDK_ROOT=... cargo build -p tactile-cora
# Run with a synthetic 1024-point Cora talker on rt/tactile/left/points
cargo run -p bus-tap -- --channel tactile_left/tactile --kind sample
# After `rollio collect`, expect Parquet column observation.sensor.tactile.tactile_point_cloud2 with shape [1024, 6].
```
