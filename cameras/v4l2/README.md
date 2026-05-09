# rollio-device-v4l2

**Linux V4L2** capture driver: reads `/dev/video*` via mmap and publishes frames on Rollio’s **hierarchical** camera topics so **`rollio-encoder`** and the rest of the pipeline can treat a webcam like any other camera channel.

---

## Concepts & behaviors

### One V4L device ≈ one logical Rollio device (today)

The current driver expects **exactly one enabled camera channel** per config (typically `channel_type = "color"`). That channel becomes the only streaming surface for that process.

### Cameras vs robots (terminology)

This driver only implements **camera** semantics:

- There is **no per-channel joint “mode”** IPC loop like on arms (no `.../control/mode` subscription in this binary).
- **Enabled** in practice means “**`run` is alive** and publishing frames”; discovery still advertises enabled/disabled **modes** in **`query`** for wizard consistency.

### Pixel formats (mental model)

- The **bus** `pixel_format` in config (rgb24, bgr24, yuyv, mjpeg) tells consumers what’s in the iceoryx payload.
- **Heavy** conversion (YUYV→RGB, JPEG decode) is intentionally pushed to **`rollio-encoder`** when you ship raw YUYV/MJPEG from the device. This driver only does **cheap** fixes (RGB↔BGR swap, grey→RGB expand). If you pick an impossible combo, **`run`** fails fast at startup with an actionable error.

### Subcommands

#### `probe`

Scans `/dev/video*`, skips non-capture nodes and likely **RealSense** V4L nodes (those use a dedicated driver). **`--json`** returns ID list.

#### `validate <id>`

Ensures the path opens as capture and (if **`--channel-type`** is given) that it’s compatible with **`color`**.

#### `capabilities <path>`

JSON dump of modes (native fourcc, resolution, fps). Older workflows; **`query`** is preferred for setup.

#### `query <id>`

Builds **`DeviceQueryResponse`** with realistic **`CameraChannelProfile`** rows (each profile states which **bus** `pixel_format` matches which V4L native format — important so MJPEG-only webcams don’t get mis-labeled as rgb24).

#### `run`

Single-threaded loop: mmap capture → optional cheap convert → publish on **`{bus_root}/{channel_type}/frames`**.

- **`--config`** xor **`--config-inline`** (TOML `BinaryDeviceConfig`).
- **`--dry-run`** parses and exits.
- Stops when it sees **`ControlEvent::Shutdown`** on **`control/events`**.

---

## iceoryx2

- **Publish:** `{bus_root}/{channel_type}/frames` (`[u8]` + `CameraFrameHeader`).
- **Subscribe:** `control/events`.

**Preview:** produced by **`rollio-encoder`** on `{bus_root}/{channel_type}/preview`, not here.

---

## Lifecycle

**Launched by:** `rollio` for `driver = "v4l2"`.

**Children:** none.

---

## Built product & dependencies

- **Binary:** `rollio-device-v4l2`.
- **System:** V4L2 device nodes + kernel drivers.
- **APT:** `v4l-utils` optional for manual debugging.

## See also

- [`rollio-encoder`](../../encoder/README.md), [`design/device-as-binaries.md`](../../design/device-as-binaries.md).
