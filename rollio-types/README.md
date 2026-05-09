# rollio-types

Shared **`serde`** schemas for **configuration** (what `rollio.toml` / wizard materialize into) and **ICE payloads** (what actually rides the shared-memory services). Almost every binary depends on this crate — it is the vocabulary of the system.

---

## Concepts & behaviors

### Why split `rollio-types` and `rollio-bus`?

- **`rollio-types`** answers *what shape is the message?* (`Pose7`, `EpisodeCommand`, `BinaryDeviceConfig`, …).
- **`rollio-bus`** answers *what string names the service?* (`control/events`, `{bus_root}/…`).

Keeping them separate lets drivers in other languages depend on a **mirrored** naming module without pulling the whole Rust type graph.

### Configuration vs runtime messages

| Area | Examples in this crate |
|------|------------------------|
| **Project / device config** | `ProjectConfig`, `BinaryDeviceConfig`, encoder/assembler/teleop slices. |
| **ICE payloads** | `ControlEvent`, `EpisodeCommand`, `JointVector15`, `CameraFrameHeader`, … |

Changes here ripple to **every** producer/consumer — treat field additions as protocol upgrades.

### Helper binary: `rollio-config`

Built from [`src/bin/rollio-config.rs`](src/bin/rollio-config.rs). Dev-focused utilities (dump/validate workflows — see **`--help`** after build). **Not** part of the capture hot path.

---

## iceoryx2

This crate defines **types only**; it never opens iceoryx2 itself. Service strings live in [`rollio-bus`](../rollio-bus/README.md).

---

## Lifecycle

Compiled into dependents. **`rollio-config`** is optional output of the same crate graph.

---

## Built product & dependencies

- **Artifacts:** library + optional **`rollio-config`** binary.
- **APT / system:** Rust toolchain only.

## See also

- [`rollio-bus`](../rollio-bus/README.md), [`design/device-as-binaries.md`](../design/device-as-binaries.md).
