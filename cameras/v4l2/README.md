# rollio-device-v4l2

**V4L2 (Linux video capture) device driver** for Rollio: publishes camera frames on the hierarchical iceoryx2 topics expected by the controller and visualizer.

## CLI

- **`probe`** — List devices; **`--json`** for machine output.
- **`validate <id>`** — Check device + channel types; **`--json`** optional.
- **`capabilities <path>`** — Legacy human-readable capability dump (some stacks still call this; newer flow prefers **`query`**).
- **`query <id>`** — Device + channel metadata for setup; **`--json`** for structured output.
- **`run`** — Stream frames from config: **`--config`**, **`--config-inline`**, **`--dry-run`**.

Implementation uses the `v4l` crate (mmap capture), supports common pixel formats (e.g. RGB3, YUYV, MJPEG), and maps into `rollio_types::config::BinaryDeviceConfig` / query responses like other `rollio-device-*` binaries.

## See also

- [`design/device-as-binaries.md`](../../design/device-as-binaries.md) — intended unified device contract.
- [`rollio-bus`](../../rollio-bus/README.md) — topic naming.
