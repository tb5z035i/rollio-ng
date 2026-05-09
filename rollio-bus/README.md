# rollio-bus

**Library crate** (`rollio_bus`): the **single source of truth** for how Rollio names iceoryx2 services and how deep robot-state rings should be. Binaries link this crate so publishers and subscribers **always agree** on strings like `control/events` and `{bus_root}/arm/states/joint_position`.

---

## Concepts (for new colleagues)

### Why this crate exists

iceoryx2 identifies a pub/sub channel by **service name string**. If the camera driver and encoder disagree by one character, frames vanish silently. **`rollio-bus`** centralizes those strings as `const fn` / small helpers so Rust, C++, and Python glue can mirror the same names ([`robots/nero` Python helpers](../robots/nero/src/rollio_device_nero/ipc/services.py)).

### Two layers of naming

1. **Global session services** — fixed names (`control/events`, `assembler/episode-ready`, …). Cross-cutting: controller, UI bridge, storage, encoders.
2. **Per-device hierarchical topics** — prefixed by **`bus_root`** and **`channel_type`** from config. Same physical robot can expose **`arm`** and **`g2`** channels; each gets **its own** subtree — **modes** and **state streams** never merge across channels.

### “Modes” vs `states/{kind}`

- **Operational mode** lives on **`.../control/mode`** ↔ **`.../info/mode`** (`DeviceChannelMode`): *should this channel accept operator commands right now?*
- **Telemetry streams** live on **`.../states/{kind}`** (`joint_velocity`, `parallel_effort`, …): *what are the sensors reporting at hundreds of Hz?*

### Ring buffers (`STATE_BUFFER`, …)

Robot drivers often publish **~250 Hz**. If a consumer stalls (e.g. assembler staging), iceoryx2 would overwrite samples after only a couple of milliseconds with defaults. **`STATE_BUFFER`** bumps queue depth cooperatively — **every** participant opening that service must request compatible caps.

This crate ships **only naming + constants** — no sockets, no processes.

---

## iceoryx2 naming reference

### Global control-plane services

| Service | Typical payload role |
|---------|---------------------|
| `control/events` | Session-wide `ControlEvent` broadcast. |
| `control/episode-command` | UI/commands → controller. |
| `control/episode-status` | Controller → UI progress. |
| `setup/command`, `setup/state` | Wizard messages. |
| `encoder/video-ready`, `encoder/backpressure` | Encoder/assembler/controller coordination. |
| `assembler/episode-ready`, `storage/episode-stored` | Episode staging commit pipeline. |

### Hierarchical patterns (`bus_root`, `channel_type`)

See [`src/lib.rs`](src/lib.rs) for helpers:

| Pattern | Meaning |
|---------|---------|
| `{bus_root}/{channel}/frames` | Raw camera payload + header. |
| `{bus_root}/{channel}/preview` | Encoder RGB preview for UI. |
| `{bus_root}/{channel}/states/{kind}` | High-rate robot observations. |
| `{bus_root}/{channel}/commands/{kind}` | Commands (when following). |
| `{bus_root}/{channel}/info/mode`, `.../control/mode` | Channel mode telemetry / requests. |

### Legacy helpers

`camera/{name}/frames`, `robot/{name}/state`, `robot/{name}/command` — older demos; [`rollio-test-publisher`](../test/test-publisher/README.md) and [`rollio-bus-tap`](../test/bus-tap/README.md) still use them.

---

## Lifecycle

Linked into other crates; **no standalone process**.

---

## Built product & dependencies

Rust **rlib** only; no apt packages of its own.

## See also

- [`rollio-types`](../rollio-types/README.md), [`design/device-as-binaries.md`](../design/device-as-binaries.md).
