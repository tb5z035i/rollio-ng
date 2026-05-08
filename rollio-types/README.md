# rollio-types

Shared **configuration**, **iceoryx2 message types**, and **schema helpers** for Rollio.

## Contents

- **`config`** — Parsed TOML surface for the controller, devices, visualizer, encoder, teleop pairs, UI runtime, control server, etc. (`ProjectConfig` and related structs).
- **`messages`** — Zero-copy-friendly payloads used on the bus: camera headers + frame bytes, robot state/command types, control and episode events, device channel modes, etc.
- **`schema`** — Supporting types for validation or tooling.

The crate is depended on by almost every binary and by integration tests. **Integration tests** here cover config parsing and message invariants.

## Tools

The package also builds a small `rollio-config` helper binary (see `src/bin/rollio-config.rs`) for config-related CLI tasks used in development.

## See also

- [`rollio-bus`](../rollio-bus/README.md) — topic naming that carries these messages.
- [`design/device-as-binaries.md`](../design/device-as-binaries.md) — target shape for `query --json` and device blocks (implementation may differ in details).
