# rollio-device-airbot-play

Driver for the **AIRBOT Play** arm and its **mounted parallel end-effector** (E2/G2 family). Low-level motion and CAN I/O live in **`airbot_play_rust`**; this binary is the Rollio adapter (iceoryx2 + `BinaryDeviceConfig`).

---

## Concepts & behaviors

### One process, multiple channels

A single physical AIRBOT stack is usually one **logical device** with **`bus_root`** shared by:

- An **`arm`** channel (6-DOF + FK/IK-capable poses).
- Optionally a **parallel gripper** channel (`channel_type` **`e2`** or **`g2`**, etc.).

Those channels are **independent** at the IPC layer:

- Each has its **own** `DeviceChannelMode` on **`{bus_root}/{channel_type}/control/mode`** / **`.../info/mode`**.
- The arm can be in **command-following** while the gripper is in **free-drive**, and vice versa.

State telemetry is also **per channel**: arm publishes joint/pose topics; gripper publishes `parallel_*` topics (see iceoryx section).

### Subcommands

#### `probe`

Scans CAN interfaces for AIRBOT Play stacks, returns device IDs (typically product serial). **`--timeout-ms`** bounds empty-bus latency; **`--json`** prints ID list.

#### `validate <id>`

Confirms the probe result still resolves. If **`--channel-type`** is non-empty, validation today effectively checks **arm** compatibility (see source for exact rules). **`--json`** for automation.

#### `query <id>`

Emits **`DeviceQueryResponse`**: dof, modes, **`supported_states` / `supported_commands`**, joint/gripper value limits, and **direct_joint_compatibility** (which channels may lead/follow in teleop). **New colleagues:** teleop pairing legality comes from **`query --json`**, not from hard-coded controller tables.

Notable **`query`** facts:

- **E2** is passive — **`query`** may advertise **free-drive only** for that channel (cannot follow servo commands).
- **G2** has full mode set where hardware allows.

#### `run`

Connects **`extra.interface`** (SocketCAN name, e.g. `can0`) and spins **one thread per enabled channel**.

- Parses **`BinaryDeviceConfig`** via **`--config`** or **`--config-inline`**.
- **`--dry-run`** loads config then exits without CAN.
- **Shutdown:** reacts to **`ControlEvent::Shutdown`** on **`control/events`** **and** `SIGINT`/`SIGTERM` so motors can ramp to **`Disabled`** safely.

**Command following:** the arm only consumes **`joint_position`**, **`joint_mit`**, and **`end_pose`** topics when mode is **`command-following`**; gripper consumes **`parallel_position`** / **`parallel_mit`** in that mode.

---

## iceoryx2

All channels:

- **Subscribe:** `control/events`; `{bus_root}/{channel_type}/control/mode`.
- **Publish:** `{bus_root}/{channel_type}/info/mode`.

**Arm** (`channel_type == "arm"`):

- **Publish:** `states/joint_position`, `joint_velocity`, `joint_effort`, `end_effector_pose` (subset from config).
- **Subscribe:** `commands/joint_position`, `joint_mit`, `end_pose`.

**Gripper** (`e2` / `g2` / …):

- **Publish:** `states/parallel_position`, `parallel_velocity`, `parallel_effort`.
- **Subscribe:** `commands/parallel_position`, `parallel_mit`.

### Mode → hardware (summary)

| Mode | Arm | Gripper |
|------|-----|---------|
| **Disabled** | Safe disable | Servo off |
| **FreeDrive** | Hand-guidable | Passive / hold |
| **Identifying** | Free-drive + LED cue | G2 runs identify motion |
| **CommandFollowing** | Tracks commands | Actuates parallel commands |

---

## Lifecycle

**Launched by:** `rollio` when `driver = "airbot-play"` (default executable name `rollio-device-airbot-play`).

**Children:** Tokio + per-channel threads (no extra OS processes).

---

## Built product & dependencies

- **Binary:** `rollio-device-airbot-play`.
- **Hardware:** SocketCAN + reachable AIRBOT firmware.
- **APT / system:** bring up CAN with your distro’s tools (`ip link set can0 up` etc.).

## See also

- [`third_party/airbot-play-rust/`](../../third_party/airbot-play-rust/), [`rollio-teleop-router`](../../teleop-router/README.md).
