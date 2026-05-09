# rollio-test-publisher

**Synthetic traffic generator** for quick experiments: publishes **demo camera frames** and **demo robot joint states** on the **legacy** topic layout (`camera/{id}/frames`, `robot/{id}/state`).

---

## Concepts & behaviors

### When to use it

- Bring up **`rollio-visualizer`** prototypes without configuring full hierarchical configs.
- Stress-test JPEG/WebSocket pipelines.
- Classroom / CI scenarios where deterministic sine-wave joints are enough.

### Limitation vs modern `rollio collect`

 **`rollio collect`** defaults to **`{bus_root}/{channel}/...`** hierarchical names. **`rollio-test-publisher`** does **not** emit those paths unless you hand-hack naming to match — it is deliberately **legacy** so keep that mismatch in mind before debugging “missing frames.”

### CLI behavior (overview)

Runs forever until **`Ctrl+C`**.

Key flags (**see `src/main.rs` for exhaustive list**):

- **`--cameras`**, **`--robots`** — how many parallel publishers.
- **`--fps`**, **`--width`**, **`--height`** — timing + buffer sizing.
- **`--camera-file`** — loop a media file through FFmpeg into every camera topic.
- **`--camera-device`** — grab real V4L2 through FFmpeg (**mutually exclusive** with `--camera-file`).

Produces RGB24 payloads with **`CameraFrameHeader`** + scrolling **color bars** by default.

---

## iceoryx2

**Publish only:** legacy `camera/*` / `robot/*` service names via [`rollio-bus`](../../rollio-bus/README.md) helpers.

---

## Lifecycle

**Manual** developer / CI invocation — **not** spawned by `rollio`.

**Children:** optional FFmpeg ingest objects in-process only.

---

## Built product & dependencies

**Binary:** `rollio-test-publisher`. Optional host **FFmpeg** libraries if using media/V4L sources.

## See also

- [`rollio-bus`](../../rollio-bus/README.md).
