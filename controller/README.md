# rollio (controller)

Cargo package name: **`rollio`**. Binary: **`rollio`**.

Central **orchestration CLI**: parses project config, discovers device drivers, and runs **`setup`** (interactive or guided configuration) and **`collect`** (multi-process capture session).

## Commands

- **`rollio setup`** — Device discovery (`probe` / `query` on `rollio-device-*` executables), wizard-style flow, optional `--sim-pseudo N` to inject synthetic [`rollio-device-pseudo`](../robots/pseudo/README.md) instances. May start auxiliary servers (e.g. WebSocket bridges) as part of the flow.
- **`rollio collect`** — Requires `--config` or `--config-inline` with a full `ProjectConfig`. Spawns encoder, visualizer, teleop routers, LeRobot episode staging (`rollio-episode-lerobot`), storage, monitor (when wired), device `run` processes, and the terminal UI as planned for the config. Non-UI children typically log to files; the UI inherits the terminal.

Implementation lives under `src/` (`cli`, `collect`, `setup`, `discovery`, `process`, etc.).

## Relationship to design docs

[`design/components.md`](../design/components.md) describes episode state machines, backpressure, and module borders. The **current** CLI only exposes `setup` and `collect` (no `replay` subcommand yet).

## See also

- [`AGENTS.md`](../AGENTS.md) — workspace build and packaging.
- Per-process modules: encoder, visualizer, control-server, episode-lerobot, storage, teleop-router, devices.
