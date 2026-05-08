# rollio-visualizer

**iceoryx2 → WebSocket bridge** for **preview** traffic: subscribes to camera (and robot/state) sources configured in `VisualizerRuntimeConfig`, downsamples/compresses preview frames (JPEG pipeline), and serves a WebSocket for the terminal UI.

## CLI

Accepts **`--config`** / **`--config-inline`** (TOML `VisualizerRuntimeConfig`), plus overrides such as **`--port`**, **`--max-preview-width`**, **`--jpeg-quality`**, **`--preview-fps`**, **`--preview-workers`**.

## Role

This process is the main path for **low-latency previews** in the Ink terminal UI. It is distinct from [`rollio-control-server`](../control-server/README.md), which bridges **setup/collect control** messages.

Wire format notes: [`design/websocket-protocol.md`](../design/websocket-protocol.md) (may not match every message type in code — verify against `src/protocol` if needed).

## See also

- [`ui/terminal/`](../ui/terminal/) — React/Ink client.
- [`rollio-types`](../rollio-types/README.md) — `VisualizerRuntimeConfigV2` and related types.
