# rollio-bus

Shared **iceoryx2 service naming** and default **pub/sub capacity** constants for the Rollio workspace.

## What it provides

- **Control-plane service names** — e.g. `control/events`, `control/episode-command`, `setup/command`, `encoder/video-ready`, `assembler/episode-ready`, `storage/episode-stored`, encoder backpressure.
- **Hierarchical device topics** — helpers that build service names under a `bus_root` for multi-channel drivers (`{bus_root}/{channel_type}/frames`, `states/{kind}`, `commands/{kind}`, mode/profile control, preview tap, etc.).
- **Legacy camera/robot helpers** — `camera/{device}/frames`, `robot/{device}/state`, `robot/{device}/command` for older naming layouts.
- **Ring buffer defaults** — `STATE_BUFFER`, `STATE_MAX_PUBLISHERS`, `STATE_MAX_SUBSCRIBERS`, `STATE_MAX_NODES` so publishers and subscribers agree on iceoryx2 queue depth (important at ~250 Hz robot control).

This crate is a **library only**; it does not run as a process. Anything that opens matching iceoryx2 services should use the same names and buffer settings as the rest of the stack.

## See also

- [`rollio-types`](../rollio-types/README.md) — payloads and configuration types.
- [`design/components.md`](../design/components.md) — high-level architecture (may lag the code).
- [`design/device-as-binaries.md`](../design/device-as-binaries.md) — intended device CLI and topic layout.
