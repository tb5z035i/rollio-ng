# `rollio-device-umi` — UMI bridge device

A static C++ executable that subscribes to **cora**-published FastDDS topics
(H264 `CompressedVideo` cameras, `sensor_msgs::msg::Imu` inertial samples) and
republishes them onto rollio's iceoryx2 bus with loaned samples. Designed
for the UMI / XF9600 data-collection workflow where the encoded video pipeline
runs inside cora and rollio is the consumer.

## Subcommands

| Subcommand | Purpose |
|---|---|
| `probe [--json]` | Fast static probe; returns `["umi"]`. No DDS contact. |
| `validate <id> --channel-type <type>... [--json]` | Accept the singleton id `umi`. |
| `capabilities <id>` | Print TOML config schema hint. |
| `query <id> [--json]` | Emit `DeviceQueryResponse`; honours `--config-inline`. |
| `run --config <path> | --config-inline <toml> [--dry-run]` | Run the bridge loop. |

## TOML config

The bridge consumes the standard rollio `BinaryDeviceConfig` shape. Bridge-specific
fields live in each channel's flattened `extra: toml::Table`:

```toml
name = "umi"
driver = "umi"
id = "umi"
bus_root = "umi"

# Device-level DDS settings (flattened into BinaryDeviceConfig.extra).
[dds]
domain_id = 0
use_shm = true
use_udp = false

[[channels]]
channel_type = "head_left"
kind = "camera"
enabled = true
profile = { width = 1280, height = 1088, fps = 20, pixel_format = "h264" }
# Bridge-specific (flattened into channel.extra):
dds_topic = "rt/robot/camera/head/left/video_encoded"

[[channels]]
channel_type = "imu_head"
kind = "imu"
enabled = true
dds_topic = "rt/robot/imu/head/data"
```

Default channel-type mapping for XF9600 (check with the operator before adopting):

| `channel_type` | Cora topic |
|---|---|
| `head_left` | `rt/robot/camera/head/left/video_encoded` |
| `head_right` | `rt/robot/camera/head/right/video_encoded` |
| `left_wrist_left` | `rt/robot/camera/left_wrist/left/video_encoded` |
| `left_wrist_right` | `rt/robot/camera/left_wrist/right/video_encoded` |
| `right_wrist_left` | `rt/robot/camera/right_wrist/left/video_encoded` |
| `right_wrist_right` | `rt/robot/camera/right_wrist/right/video_encoded` |
| `imu_head` | `rt/robot/imu/head/data` |
| `imu_left_wrist` | `rt/robot/imu/left_wrist/data` |
| `imu_right_wrist` | `rt/robot/imu/right_wrist/data` |

## IDL provenance and regeneration

The `idl/` directory contains six IDL files copied verbatim from the cora
repository (under its `framework/dds/msg/...` tree). They are vendored here
so this device has **zero build- or run-time dependency on the cora repo**.

The C++ types and PubSubTypes under `src/generated/` are equivalent to what
fastddsgen would emit for these IDLs. They are hand-written rather than
generated because the rollio dev workflow does not assume a JDK is available
(fastddsgen ships as a Java tool). To regenerate using fastddsgen once a JDK
is available, the canonical recipe is:

```bash
fastddsgen -typeros2 -replace -cs \
    -d devices/umi/src/generated \
    devices/umi/idl/foxglove_msgs/msg/CompressedVideo.idl \
    devices/umi/idl/sensor_msgs/msg/Imu.idl \
    devices/umi/idl/std_msgs/msg/Header.idl \
    devices/umi/idl/builtin_interfaces/msg/Time.idl \
    devices/umi/idl/geometry_msgs/msg/Quaternion.idl \
    devices/umi/idl/geometry_msgs/msg/Vector3.idl
```

Wire-format note: cora publishes via Fast-DDS 1.x with the `-typeros2` flag,
which produces XCDRv1 (PLAIN_CDR) on the wire. Our hand-written PubSubTypes
declare PLAIN_CDR for serialisation and let fastcdr's deserialiser pick up
the wire endianness from the encapsulation header.

## Architecture

- One `eprosima::fastdds::dds::DomainParticipant` shared across all bridged topics.
- One `std::thread` per bridged channel. Each thread owns a long-lived sample
  buffer (avoids per-iteration `std::vector` malloc churn), calls
  `take_next_sample`, and republishes via iceoryx2 `loan_slice_uninit` + `send`
  for cameras or `loan_uninit` + `send` for IMUs.
- Reader QoS: `RELIABLE` + `VOLATILE` + `KEEP_LAST(1)` to match cora's
  `reliableQoS()` writer side.
- Frame index is a local counter that resets on bridge restart (cora's
  `CompressedVideo` IDL doesn't carry one).
- Listens on `CONTROL_EVENTS_SERVICE` for `Shutdown`.

## Out of scope

- GNSS / tactile / encoder / button channels that XF9600 also publishes. Easy
  to add by extending the TOML schema later.
- Bidirectional bridging. The bridge is unidirectional (cora -> rollio).
- DDS-side discovery in `probe` / `query`. Both run static; adding a
  `--probe-dds` flag would be a follow-up.
- Visualizer preview for H264 channels — disabled by the encoder's
  passthrough mode (`encoder/src/media.rs`).
