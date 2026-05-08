# rollio-device-pseudo

**Synthetic multi-channel device** for development and CI: emits configurable **camera** and **robot** channels with the same iceoryx2 surface as real hardware (states, commands, mode control, frames).

## CLI

Matches the unified device pattern:

- **`probe`** — Simulated device IDs; **`--sim-cameras`**, **`--sim-arms`**, **`--dof`**, **`--json`**.
- **`validate`** — **`--channel-type`** list support.
- **`query`** — Rich channel metadata for setup wizard; **`--json`**.
- **`run`** — Publish/subscribe loop from **`--config`** / **`--config-inline`**.

The controller does **not** auto-discover this binary on PATH; use **`rollio setup --sim-pseudo N`** so the orchestrator injects pseudo instances during discovery.

## See also

- [`robots/airbot_play_rust/README.md`](../airbot_play_rust/README.md) — real AIRBOT hardware driver.
- [`design/device-as-binaries.md`](../../design/device-as-binaries.md).
