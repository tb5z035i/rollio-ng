# Robots — device drivers (`rollio-device-*`)

This folder holds **hardware and simulation drivers** Rollio shells out to. Each driver is one **executable** (Rust or Python) that obeys the same four-entry CLI so **`rollio setup`** and **`rollio collect`** do not special-case vendors.

---

## Concepts (read this first)

### Physical rig vs logical “device” in config

- A **configured device** (`BinaryDeviceConfig` in TOML) is one **`rollio`** process child: one OS process running one driver's **`run`**.
- That logical device often represents **one physical appliance** (e.g. one AIRBOT base) but can expose **several independent channels** at once (e.g. arm + parallel gripper).

### `bus_root` — namespace for IPC

Each device has a **`bus_root`** string (e.g. `airbot_play`). **All** iceoryx2 topics for that device hang under that prefix:

- Cameras: `{bus_root}/{channel_type}/frames`, …
- Robots: `{bus_root}/{channel_type}/states/...`, `.../commands/...`, `.../control/mode`, …

Two different devices must use different `bus_root` values so their topics never collide.

### Channels — independent units of behavior

A **channel** is one row in `[[channels]]` in the device TOML:

- Has a **`channel_type`** string label (`arm`, `color`, `g2`, …) — unique **within that device**.
- Is either a **camera** or **robot** kind for data typing.
- **Robot channels are independent:** the arm can be in **command-following** while the gripper is in **free-drive**, because each channel has its **own** `.../control/mode` and `.../info/mode` pair. Do not assume one mode bit applies to the whole physical robot.

### “Mode” vs “state streams” (easy to confuse)

- **Mode** (disabled / free-drive / command-following / identifying, etc.) is the **operator-level** behavior for that channel. It is carried as **`DeviceChannelMode`** on **`{bus_root}/{channel_type}/control/mode`** (commands in) and **`.../info/mode`** (telemetry out). Cameras often collapse this to enabled/disabled.
- **State streams** are **high-rate telemetry** on **`{bus_root}/{channel_type}/states/{kind}`** — e.g. `joint_position`, `joint_velocity`, `end_effector_pose`, `parallel_position`. There can be **multiple state kinds per channel**, each on its own topic. These are *not* the same thing as “mode.”

### Standard driver CLI (all four must exist)

| Command | Purpose |
|---------|---------|
| **`probe`** | List candidate device IDs on this machine (USB paths, CAN ifaces, serials, …). |
| **`validate <id>`** | Cheap check that `id` still exists and optional **`--channel-type`** filters pass. Used before `collect` heavy startup. |
| **`query <id>`** | Emit a structured **`DeviceQueryResponse`** (JSON with **`--json`**) so setup can populate limits, profiles, supported modes, teleop compatibility, etc. **This is the contract** between driver and framework — no hidden tables in the controller. |
| **`run`** | Long-lived process: publish/subscribe on the hierarchical topics, react to **`control/events`** (`Shutdown`), honor per-channel mode. |

---

## iceoryx2 summary

Every real driver should:

- **Publish** observations on `{bus_root}/{channel_type}/states/<kind>` (and camera frames on `.../frames` when applicable).
- **Subscribe** to commands on `{bus_root}/{channel_type}/commands/<kind>` when the channel supports command-following.
- **Subscribe** `{bus_root}/{channel_type}/control/mode` and **publish** `.../info/mode` for mode sync.
- **Subscribe** global **`control/events`** and exit cleanly on `Shutdown`.

Exact topic strings and buffer defaults: [`rollio-bus`](../rollio-bus/README.md).

---

## Layout in this repo

- **`pseudo/`** — `rollio-device-pseudo` (CI / dev). Opt-in via `rollio setup --sim-pseudo N`.
- **`airbot_play/`** — Python wrapper (legacy / extras).
- **`airbot_play_rust/`** — `rollio-device-airbot-play` (CAN arm + gripper).
- **`nero/`** — `rollio-device-agx-nero` (Python + Pinocchio).

---

## Adding a new driver

See also [`design/device-as-binaries.md`](../design/device-as-binaries.md).

1. Ship **`rollio-device-<name>`** (binary or setuptools console script).
2. Implement **`probe` / `validate` / `query` / `run`** as above.
3. Populate **`query --json`** fully (`value_limits`, `modes`, `supported_states`, `supported_commands`, `direct_joint_compatibility`, …) — [`rollio-types`](../rollio-types/README.md).

## Controller resolution (`rollio collect`)

1. `target/.../rollio-device-<name>` next to workspace build
2. Adjacent dirs / `cameras/build`
3. Any **`rollio-device-*`** on `$PATH`

 **`rollio-device-pseudo`** is not on the default discovery PATH — use **`--sim-pseudo`**.
