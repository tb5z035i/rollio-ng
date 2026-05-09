# rollio-bus-tap

**Debug tap:** subscribe to a **hand-picked** set of iceoryx2 services and print **one JSON line per sample** (plus end-of-run summaries) for a fixed wall-clock duration. Useful when you want proof that publishers exist before opening gdb on a full `rollio collect`.

---

## Concepts & behaviors

### What it is good for

- Verifying **frame headers** (`timestamp_us`, `frame_index`) look sane.
- Watching **`control/events`** alongside **`control/episode-status`** while pressing UI buttons.
- Measuring **command latency** on legacy **`robot/{name}/command`** topics when `--follower` is set.

### Topic layout caveat

Many flags target the **legacy** `camera/*` and `robot/*` names (same as [`rollio-test-publisher`](../test-publisher/README.md)). Hierarchical `{bus_root}/...` sessions need different tooling or a small one-off subscriber — this binary is intentionally narrow.

### CLI behavior

- **`--camera NAME`** — repeat per camera (legacy path).
- **`--robot-state NAME`** — legacy `RobotState` stream.
- **`--leader NAME`** — ensures that robot’s state stream is tapped (even if you forgot `--robot-state`).
- **`--follower NAME`** — additionally tap **`robot/{follower}/command`**.
- **`--duration-s`** — auto-exit (can end early on signal).

Prints JSON lines to stdout; ends with aggregate stats (fps medians, command latency percentiles).

---

## iceoryx2

**Subscribe-only** open-or-create on: selected `camera/*`, `robot/*/state`, optional `robot/*/command`, **`control/events`**, **`control/episode-status`**.

---

## Lifecycle

Manual engineering use.

---

## Built product & dependencies

**Binary:** `rollio-bus-tap`; workspace Rust + iceoryx2 only.

## See also

- [`rollio-bus`](../../rollio-bus/README.md).
