# rollio-control-server

**WebSocket bridge for the control plane**: runs on **`127.0.0.1:<port>`** and forwards JSON between UI clients and iceoryx2 **setup** or **collect** channels, depending on **`role`** in `ControlServerConfig`.

## Modes

- **`setup`** — Forwards setup commands and state snapshots (wizard / configuration UI).
- **`collect`** — Forwards episode commands, episode status, and backpressure-related snapshots to clients.

## CLI

Requires **`--config`** or **`--config-inline`** with TOML matching `ControlServerConfig` (`port`, `role`).

## See also

- [`rollio-visualizer`](../visualizer/README.md) — preview WebSocket, not control.
- Library API: `src/lib.rs` (`run`, `ControlServerRole`, `UiCommand`).
- [`design/components.md`](../design/components.md) — intended split between preview and control (high level).
