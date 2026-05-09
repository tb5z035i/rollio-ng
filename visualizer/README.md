# rollio-visualizer

Takes **preview-load** from iceoryx2 (downscaled RGB camera taps + selected robot state streams), **JPEG-compresses** frames, and exposes a **WebSocket** for the terminal UI (and any client that speaks the same protocol).

---

## Concepts & behaviors

### Preview vs raw camera (important for new colleagues)

Operators want **low-latency thumbnails**, not full-res raw streams on the UI socket.

- **Device drivers** publish **full or native** frames on `{bus_root}/{channel}/frames`.
- **`rollio-encoder`** decodes/scales to RGB and publishes an always-on **`{bus_root}/{channel}/preview`** topic.
- **`rollio-visualizer` subscribes only to `preview` + configured robot state topics** — it never does MJPEG/YUYV decode work.

### Robot UI: one panel per channel × state kind

Visualizer config lists **which** `state_topic` + `state_kind` to plot (e.g. joint positions for `arm`, parallel position for `g2`). **Modes** (free-drive vs command-following) come from the **control-plane WebSocket** via [`rollio-control-server`](../control-server/README.md), not from extra IPC inside the visualizer poller.

### “Run” behavior — single process, no subcommands

There is only one main entrypoint. Configuration is **TOML** or flags:

- **`--config`** / **`--config-inline`** (`VisualizerRuntimeConfigV2`).
- Overrides: **`--port`**, **`--max-preview-width`**, **`--max-preview-height`**, **`--jpeg-quality`**, **`--preview-fps`**, **`--preview-workers`** (thread pool size for compression).

**Shutdown:** polling watches **`control/events`** for **`Shutdown`** so **`rollio`** can tear down preview stacks during identify swaps without waiting on OS kill timeouts.

Wire format: [`design/websocket-protocol.md`](../design/websocket-protocol.md) (verify vs `src/protocol`).

---

## iceoryx2

**Subscribe only:**

- `control/events`
- Encoder **preview** topics from config (`preview_topic` per camera source).
- Robot **state** topics (`state_topic` + typed `state_kind`).

No AV data is **published** back to iceoryx2 from this binary.

---

## Lifecycle

**Spawned by:** `rollio` setup/collect plans ([`build_visualizer_spec`](../controller/src/runtime_plan.rs)).

**Children:** none (async Tokio server + poll loop).

---

## Built product & dependencies

**Binary:** `rollio-visualizer`. Standard Rollio Rust deps per `Cargo.toml`; no extra apt packages beyond workspace baseline.

## See also

- [`rollio-control-server`](../control-server/README.md), [`ui/terminal/`](../ui/terminal/).
