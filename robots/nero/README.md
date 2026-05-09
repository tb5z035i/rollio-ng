# rollio-device-agx-nero

Python driver for the **AGX Nero** 7-DOF arm plus **AGX parallel gripper**. It installs as **`rollio-device-agx-nero`** and follows the same **`probe` / `validate` / `query` / `run`** contract as Rust drivers so **`rollio`** needs no Nero-specific code paths.

---

## Concepts & behaviors

### Physical hardware, logical device, independent channels

- One **`run`** process typically controls **one CAN-connected Nero** but exposes it as **two Rollio channels**:
  - **`arm`** — joints, FK/IK, Cartesian commands.
  - **`gripper`** — parallel jaw motion (`parallel_*` semantics).
- **`bus_root`** prefixes **all** iceoryx services for that process. **`channel_type`** disambiguates arm vs gripper.
- **Independence:** each channel owns its **mode** (`disabled`, `identifying`, `free-drive`, `command-following`) on **`{bus_root}/{channel}/control/mode`** / **`.../info/mode`**. The arm can follow joint commands while the gripper idles in free-drive, etc.
- **State streams** (e.g. `joint_position`, `parallel_position`) are **high-rate telemetry** on **`.../states/{kind}`** — orthogonal to **mode**. New colleagues: do not conflate “state topic” with “operational mode.”

### Motion / control stack (where logic lives)

- Real-time hardware: Agilex **[`pyAgxArm`](../../third_party/pyAgxArm)** over CAN.
- Gravity, FK, IK: **[Pinocchio](https://github.com/stack-of-tasks/pinocchio)** + bundled URDF (see **Tool tip** below for TCP frames).

### Subcommands

#### `probe`

Lists candidate interfaces / arms you could pass as **`id`** (commonly a CAN iface such as `can0`). Use this to discover what to type into setup.

#### `validate <id>`

Lightweight “can we talk to hardware on this id?” check (connect + enable path used in upstream tests).

#### `query <id> [--json]`

Prints **`DeviceQueryResponse`** for the wizard: DOF, supported modes per channel, limits, compatible state/command kinds, optional metadata. **`--json`** is what automation and **`rollio setup`** consume.

#### `run --config …` / `--config-inline …`

Long-lived driver: opens iceoryx2, spawns **arm** and **gripper** worker threads, reacts to **`control/events`** **`Shutdown`** and per-channel **mode** messages.

---

## Arm channel modes (behavioral)

| Mode | What the operator experiences |
|------|------------------------------|
| **disabled** | Ramps joints toward **q = 0** over ~3 s under MIT with gravity feed-forward, then holds. Motors **stay energized** (does not call `robot.disable()`) so the arm does not collapse. Same ramp runs on shutdown. |
| **identifying** | Same torques as **free-drive** (pure gravity comp); distinct **mode** so the setup wizard can flash/highlight hardware. |
| **free-drive** | True backdrivable feel: gravity compensation only, no position loop fighting the human. |
| **command-following** | Tracks **`joint_position`**, **`joint_mit`**, and **`end_pose`** commands (Cartesian solved with damped pseudo-inverse IK). |

## Gripper channel modes (behavioral)

Mirrors the **AIRBOT-style parallel gripper contract** (open ≈ 0 m, closed ≈ 0.07 m):

- **identifying** — sinusoidal open/close pattern.
- **free-drive** — hold / observe.
- **command-following** — applies **`parallel_position`** / **`parallel_mit`** to `move_gripper_m`.

---

## iceoryx2

Helpers in [`ipc/services.py`](src/rollio_device_nero/ipc/services.py) mirror [`rollio-bus`](../../rollio-bus/README.md).

- **Subscribe:** `control/events`; `{bus_root}/{channel}/control/mode`.
- **Publish:** `{bus_root}/{channel}/info/mode`.
- **Arm:** publish `states/joint_*`, `end_effector_pose`; subscribe commands when following.
- **Gripper:** publish `states/parallel_*`; subscribe parallel commands when following.

---

## Lifecycle

**Launched by:** `rollio` when `driver = "agx-nero"`, or manually for bring-up.

**Children:** Python threads (arm loop + gripper loop); coordinated shutdown below.

---

## Install & dependencies

```bash
git submodule update --init third_party/iceoryx2 third_party/pyAgxArm
uv pip install -e robots/nero
```

Installs **`rollio-device-agx-nero`** + `rollio_device_nero` package. Needs **iceoryx2 Python wheel** from the submodule and (for FK/IK tests) **Pinocchio** when installed on the host.

### Minimal `nero.toml` example

```toml
name = "agx_nero"
driver = "agx-nero"
id = "can0"
bus_root = "agx_nero"
interface = "can0"

[[channels]]
channel_type = "arm"
kind = "robot"
mode = "free-drive"
dof = 7
publish_states = ["joint_position", "joint_velocity", "joint_effort", "end_effector_pose"]

[[channels]]
channel_type = "gripper"
kind = "robot"
mode = "free-drive"
dof = 1
publish_states = ["parallel_position", "parallel_velocity", "parallel_effort"]
```

---

## Tool tip / TCP for FK & IK

`end_effector_pose` and Cartesian **`end_pose`** targets use a Pinocchio frame on **`joint7`**:

| Config | Frame |
|--------|--------|
| Gripper channel **enabled** | TCP at fingertip plane (~0.1413 m along gripper z from mount). |
| Gripper **disabled** | Bare flange offset (see source constants). |

Override via `tip_offset` on `NeroModel` if you mount a custom tool.

---

## Shutdown

On **`SIGINT` / `SIGTERM` / `SIGHUP`:** ignore further signals, finish current tick, arm runs **disabled** ramp to zero, gripper stops actuating, CAN closes — motors remain holding last safe command.

## Tests

```bash
cd robots/nero && pytest -q
```

Pinocchio-dependent tests skip if `pin` is missing.
