# rollio-device-airbot-play

**AIRBOT Play** integration: CAN-based arm (+ mounted end-effector types) as a `rollio-device-*` binary. Wraps the in-repo **`airbot_play_rust`** stack (kinematics, MIT control, CAN worker).

## CLI

- **`probe`** — Discovers arms on CAN interfaces (timeouts tuned for fast empty-bus behavior).
- **`validate`** / **`query`** — Same contract as other Rollio devices (`--json` where applicable).
- **`run`** — Real-time control loop: publishes joint/EE/parallel states, consumes commands, honors mode and profile control services under `bus_root`.

## Hardware

Requires a working **SocketCAN** interface (e.g. `can0`) and a reachable AIRBOT Play per config `extra.transport` / `interface`.

## See also

- [`third_party/airbot-play-rust/`](../../third_party/airbot-play-rust/) — low-level protocol and tests.
- [`rollio-teleop-router`](../../teleop-router/README.md) — leader/follower forwarding on top of published states.
