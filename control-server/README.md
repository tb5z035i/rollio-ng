# rollio-control-server

**Bridges human UIs to the machine-oriented iceoryx2 control plane.** It listens on **`127.0.0.1:<port>`** for WebSocket clients, converts JSON lines to typed messages, and vice versa. **Preview video** is *not* here — that is [`rollio-visualizer`](../visualizer/README.md).

---

## Concepts & behaviors

### Two roles, one binary

| Role | What humans do | What the server does on iceoryx2 |
|------|----------------|----------------------------------|
| **`setup`** | Wizard configures devices/channels | **Publish** `setup/command`, **subscribe** `setup/state`. |
| **`collect`** | Operator starts/stops episodes | **Publish** `control/episode-command`, **subscribe** `control/episode-status` + `encoder/backpressure`. |

**Why not listen to `control/events`?** Design choice: session-wide shutdown for preview swaps would kill the long-lived WebSocket if the control server subscribed to every `ControlEvent`. Process exit is driven by **SIGINT/SIGTERM** from the parent controller instead (see source comments in [`ipc.rs`](src/ipc.rs)).

### Who applies device mode changes during setup?

**Not this binary.** The wizard sends **`SetupCommandMessage`** payloads; **`rollio`** (controller) interprets them and writes **`DeviceChannelMode`** onto each channel’s **`{bus_root}/{channel}/control/mode`**. New colleagues: the control-server is **thin transport**, not policy.

### CLI / configuration

Requires **`--config`** or **`--config-inline`**:

```toml
port = 9001
role = "setup"   # or "collect"
```

Logging: `env_logger` (default **`info`**).

---

## iceoryx2

See tables above — exact services in [`ipc.rs`](src/ipc.rs).

---

## Lifecycle

**Spawned by:** `rollio` with a freshly picked loopback port embedded into UI configs.

**Children:** Tokio WS task + blocking IPC poll thread.

---

## Built product & dependencies

**Binary:** `rollio-control-server`; Tokio + iceoryx2 + serde stack.

## See also

- [`rollio` controller](../controller/README.md), [`rollio-visualizer`](../visualizer/README.md).
