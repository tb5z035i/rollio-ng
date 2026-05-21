# rollio-device-coracam

`rollio-device-coracam` is the C++ Rollio camera bridge for Cora/Fast-DDS
camera topics. It subscribes to Cora raw image and H.264 video topics through
the Cora SDK, then republishes them as standard Rollio camera frame services:

```text
{bus_root}/{channel_type}/frames
CameraFrameHeader + [u8 payload]
```

The device does not record, mux, resize, decode, or transcode. Raw channels are
published as BGR24 frames. H.264 channels are published as
`PixelFormat::H264AnnexB` and are handled later by `rollio-encoder` using the
H.264 passthrough path.

## Binary Model

There is one executable and one driver name:

```text
rollio-device-coracam
driver = "coracam"
```

The three physical Coracam mount points are selected by `BinaryDeviceConfig.id`:

| id | Default device name | Default Cora topic prefix |
| --- | --- | --- |
| `cora-head` | `coracam_head` | `rt/robot/camera/head` |
| `cora-lefthand` | `coracam_lefthand` | `rt/robot/camera/left_wrist` |
| `cora-righthand` | `coracam_righthand` | `rt/robot/camera/right_wrist` |

`probe --json` returns all three entries. `query`, `validate`, and `run` use the
id to choose the descriptor. When multiple Coracam devices are present in a
project config, the controller starts multiple `rollio-device-coracam`
processes with different inline configs.

## Channels

Each descriptor exposes the same four fixed camera channels:

| channel_type | Cora input | Rollio pixel format | Default profile |
| --- | --- | --- | --- |
| `left_raw` | raw image | `bgr24` | `640x480 @ 25Hz` |
| `right_raw` | raw image | `bgr24` | `640x480 @ 25Hz` |
| `left_h264` | compressed video | `h264-annex-b` | `640x480 @ 25Hz` |
| `right_h264` | compressed video | `h264-annex-b` | `640x480 @ 25Hz` |

Only enabled fixed channels are started. Unknown extra channels are ignored so
future device revisions can add channels without breaking older binaries. At
least one fixed channel must be enabled.

Default DDS topic suffixes:

```text
left_raw   -> /left/image
right_raw  -> /right/image
left_h264  -> /left/video_encoded
right_h264 -> /right/video_encoded
```

Default DDS type names:

```text
raw  -> sensor_msgs::msg::dds_::Image_
h264 -> foxglove_msgs::msg::dds_::CompressedVideo_
```

ROS 2 tools often display topics with a leading `/`, but this driver normalizes
`/rt/...` mapping entries to `rt/...` because the Fast-DDS wire topic name is
matched without the leading slash.

## Build

Coracam is wired into the top-level camera CMake build:

```bash
make cpp-build
```

Requirements:

- `ROLLIO_BUILD_CORACAM` is enabled by the root `Makefile` by default.
- CMake must be able to find the Cora SDK via `find_package(cora CONFIG)`.
- The default SDK root is
  `prebuild/cora-sdk_1.2.0_20260517124657_linux_aarch64/opt/cora`.

To skip this driver on machines without the Cora SDK:

```bash
make ROLLIO_BUILD_CORACAM=OFF cpp-build
```

The package script ships one binary:

```text
rollio-device-coracam
```

On arm64, packaging also stages the Cora SDK runtime closure under `/opt/cora`
when the Coracam binary is present.

## CLI

```text
rollio-device-coracam probe [--json]
rollio-device-coracam query [--json] <id>
rollio-device-coracam validate [--json] [--config <path>] [--mapping <path>] <id>
rollio-device-coracam run (--config <path> | --config-inline <toml>) [--mapping <path>] [--dry-run]
```

Examples:

```bash
rollio-device-coracam probe --json
rollio-device-coracam query --json cora-righthand
rollio-device-coracam validate --json --config device-config.toml cora-righthand
rollio-device-coracam run --config device-config.toml --dry-run
```

`run` reads the selected mount point from the config's `id` field rather than
from a positional CLI argument. The device CLI expects a single
`BinaryDeviceConfig`, not a full Rollio project config.

Minimal standalone device config:

```toml
name = "coracam_righthand"
driver = "coracam"
id = "cora-righthand"
bus_root = "coracam_righthand"
dds_domain_id = 31
dds_shm_segment_size = 67108864
dds_callback_threads = 4

[[channels]]
channel_type = "left_raw"
kind = "camera"
enabled = true
[channels.profile]
width = 640
height = 480
fps = 25
pixel_format = "bgr24"

[[channels]]
channel_type = "left_h264"
kind = "camera"
enabled = true
[channels.profile]
width = 640
height = 480
fps = 25
pixel_format = "h264-annex-b"
```

## Controller Integration

`rollio setup` can discover `rollio-device-coracam` directly because the
controller has it in the known executable list. Installed binaries can also be
found through the generic `$PATH` scan for `rollio-device-*`.

In a full Rollio project config, Coracam appears as a normal `[[devices]]`
entry:

```toml
[[devices]]
name = "coracam_righthand"
executable = "rollio-device-coracam"
driver = "coracam"
id = "cora-righthand"
bus_root = "coracam_righthand"
dds_shm_segment_size = 67108864
dds_callback_threads = 4
# coracam_mapping_file = "./coracam-topics.toml"

[[devices.channels]]
channel_type = "left_raw"
kind = "camera"
enabled = true
profile = { width = 640, height = 480, fps = 25, pixel_format = "bgr24" }

[[devices.channels]]
channel_type = "left_h264"
kind = "camera"
enabled = true
record_enabled = true
preview_enabled = true
profile = { width = 640, height = 480, fps = 25, pixel_format = "h264-annex-b" }

[devices.channels.preview_config]
output_mode = "encoded"
color_codec = "h264"
backend = "auto"
width = 640
height = 480
fps = 25
```

For H.264 Annex-B preview, `preview_config.output_mode` must be `"encoded"`.
With `pixel_format = "h264-annex-b"` and encoded H.264 preview, the controller
uses the preview-role encoder as a fixed-source passthrough relay: it does not
decode, scale, or re-encode the Cora bytes. The effective preview dimensions
therefore stay at the source profile size even if `preview_config.width` /
`height` are present.

`record_enabled` and `preview_enabled` are independent. Recording uses the
recording-role encoder and publishes `recording-config` / `recording-packets`.
Live Web UI preview uses the preview-role encoder, visualizer WebSocket, and
browser decoder. Browser-side preview decoder options do not change the
recorded data.

The setup terminal preview path forces JPEG preview, so it is not suitable for
previewing H.264 Annex-B camera sources directly. Use the web UI for Coracam
encoded preview.

The DDS domain id is not stored in `config.toml`. Set it when starting collect:

```bash
ROLLIO_DDS_DOMAIN_ID=31 rollio collect --config config.toml
```

If the variable is omitted, collect injects domain id `0`.

## Mapping File

The optional mapping file is a small TOML subset for Cora-specific overrides.
Use it for topic names, type names, packet limits, participant name, and H.264
validation behavior. Do not use it only to set the DDS domain id; prefer
`ROLLIO_DDS_DOMAIN_ID` for collect runs.

```toml
domain_id = 31 # fallback only; collect-injected dds_domain_id wins
participant_name = "rollio_coracam_head"
max_packet_bytes = 4194304
annex_b_validation = "scan" # scan | metadata | auto
metadata_validation_packets = 16

[[topics]]
channel_type = "left_raw"
topic = "/rt/robot/camera/head/left/image"
type = "sensor_msgs::msg::dds_::Image_"
max_packet_bytes = 8388608
raw_expected_encoding = "bgr8"

[[topics]]
channel_type = "left_h264"
topic = "/rt/robot/camera/head/left/video_encoded"
type = "foxglove_msgs::msg::dds_::CompressedVideo_"
annex_b_validation = "scan"
metadata_validation_packets = 32
```

Mapping path sources:

- `--mapping <path>` on the device CLI.
- `ROLLIO_CORACAM_MAPPING_FILE` when `--mapping` is omitted.
- Project config keys `coracam_mapping_file`, `cora_mapping_file`,
  `mapping_file`, or `mapping`; the controller converts these to `--mapping`
  for Coracam devices.

## Runtime Environment

| Variable | Effect |
| --- | --- |
| `ROLLIO_DDS_DOMAIN_ID=<u32>` | Read by `rollio collect`, then injected into each device inline config as `dds_domain_id`. |
| `ROLLIO_CORACAM_MAPPING_FILE=<path>` | Default mapping path when the device CLI has no `--mapping`. |
| `ROLLIO_CORACAM_NO_DDS=1` | Use the internal mock generator instead of Cora DDS input. Useful for tests and offline smoke runs. |
| `ROLLIO_ADVANCED_PIPELINE_LOGS=1` | Enables full 10s pipeline statistics. The controller derives this from `[runtime].advanced_pipeline_logs`. |
| `ROLLIO_CORACAM_H264_DUMP=1` | Dumps parsed Annex-B payloads to `$ROLLIO_LOG_DIR/h264-coracam/<channel_type>.h264`. |
| `ROLLIO_CORACAM_H264_DUMP_DIR=<path>` | Enables H.264 dump and overrides the output directory. |
| `ROLLIO_LOG_DIR=<path>` | Log directory provided by the controller during collect. |

## H.264 Behavior

The H.264 input type is `foxglove_msgs::msg::dds_::CompressedVideo`. Its payload
must contain Annex-B start codes. The worker scans SPS, PPS, and IDR NAL units,
waits until SPS/PPS are known, and publishes the original bytes as a Rollio
camera frame. It does not convert to AVCC, decode, mux, or transcode.

If upstream publishes one NAL unit per sample instead of complete access units,
the current runtime does not yet assemble them on the data path.

## Logs and Debugging

When started by `rollio collect`, stdout and stderr are captured in:

```text
$ROLLIO_LOG_DIR/device-<device_name>.log
```

With no `ROLLIO_LOG_DIR`, collect uses `./rollio-logs` relative to the directory
where `rollio collect` was invoked.

Useful H.264 checks:

```bash
ROLLIO_CORACAM_H264_DUMP=1 \
ROLLIO_VISUALIZER_H264_DUMP=1 \
rollio collect --config config.toml
```

Then compare:

```text
./rollio-logs/h264-coracam/<channel_type>.h264
./rollio-logs/h264-visualizer/<channel_id>.h264
```

If the Coracam dump is already invalid, check the Cora DDS payload and camera
encoder. If Coracam is valid but visualizer is invalid, check encoder
passthrough and preview packet handling. If both are valid but the browser is
wrong, check WebCodecs config and chunk boundaries.

## Tests

When C++ tests are enabled, CTest runs `rollio-devices-coracam-tests` with
`ROLLIO_CORACAM_NO_DDS=1`, so it does not need a live Cora publisher.

```bash
make cpp-test
```

The test binary covers:

- `probe`
- `run --dry-run`
- mock publish smoke path
- mapping parser
- CDR parser golden bytes
- H.264 Annex-B helpers

## Compatibility

Older configs that used the split binaries and drivers are not compatible with
the current implementation:

```text
rollio-device-coracam-head       driver = "coracam-head"
rollio-device-coracam-lefthand   driver = "coracam-lefthand"
rollio-device-coracam-righthand  driver = "coracam-righthand"
```

Use `rollio-device-coracam`, `driver = "coracam"`, and one of
`id = "cora-head"`, `id = "cora-lefthand"`, or `id = "cora-righthand"`.
