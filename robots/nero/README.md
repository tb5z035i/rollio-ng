# rollio-device-agx-nero

Python device driver for the **AGX Nero** 7-DOF arm with the AGX parallel
gripper, built for the Rollio framework. It ships a `rollio-device-agx-nero`
executable that is a drop-in peer of the Rust
[`rollio-device-airbot-play`](../airbot_play_rust) on the iceoryx2 + TOML
contract, so `rollio setup` / `rollio collect` pick up the Nero with no
controller-side changes.

Underlying hardware control uses the Agilex
[`pyAgxArm`](../../third_party/pyAgxArm) SDK; gravity compensation, FK and IK
use [Pinocchio](https://github.com/stack-of-tasks/pinocchio) with a bundled
Nero URDF.

## Install

From the workspace root, with [uv](https://docs.astral.sh/uv/):

```bash
git submodule update --init third_party/iceoryx2 third_party/pyAgxArm
uv pip install -e robots/nero
```

This installs the `rollio-device-agx-nero` executable and the `rollio_device_nero`
Python package.

## CLI

`rollio-device-agx-nero` mirrors the four subcommands of `rollio-device-airbot-play`:

```bash
rollio-device-agx-nero probe                       # list candidate Nero arms (CAN ifaces)
rollio-device-agx-nero validate can0               # connect+enable on can0 (matches test.py)
rollio-device-agx-nero query can0 --json           # emit DeviceQueryResponse for rollio setup
rollio-device-agx-nero run --config nero.toml      # run the device, mode-driven by IPC
```

A minimal device config (`nero.toml`):

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

## Modes

Arm channel:

| Mode | Behaviour |
|---|---|
| `disabled` | Smoothly ramps to `q=0` over 3 s via MIT (kp=10, kd=0.5, ff=g(q)), then holds at zero. **Never calls `robot.disable()`** — keeping the motors energised prevents the arm from dropping. The same ramp also runs as a shutdown homing phase on SIGINT/SIGTERM regardless of the current mode (see "Shutdown" below). |
| `identifying` | Same control shape as `free-drive` (kp=0, kd=0, ff=g(q)); reported as a distinct mode so the rollio setup wizard can highlight it. |
| `free-drive` | Truly floating arm: gravity compensation only (kp=0, kd=0, ff=g(q)). The operator can move it by hand without fighting any MIT damping. |
| `command-following` | MIT (kp=10, kd=0.5, ff=g(q)) tracking `joint_position` / `joint_mit` / `end_pose` commands. Cartesian commands are mapped to joint targets through a damped-pseudo-inverse Pinocchio CLIK. |

Gripper channel mirrors the AIRBOT G2 contract (open=0 m, closed=0.07 m,
identifying = sine open/close pattern, free-drive = hold, command-following =
forward `parallel_position` / `parallel_mit` to `move_gripper_m`).

## Tool tip / TCP for FK & IK

The arm's published `end_effector_pose` (and any Cartesian command target
fed to IK) is reported relative to a Pinocchio operational frame attached
to `joint7`. Two defaults:

| Config | Tip frame | SE3 placement relative to `joint7` |
|---|---|---|
| gripper channel **enabled** (with_gripper=True) | AGX gripper TCP -- midpoint between the jaws at the fingertip plane | `gripper_flange * SE3(xyz=(0, 0, 0.1413))` |
| gripper channel **disabled** | bare tool flange | `SE3(xyz=(0.032, 0, -0.0235), rpy=(-π/2, 0, -π/2))` |

The 0.1413 m gripper depth (`GRIPPER_TCP_DEPTH_M` in `gravity.py`) is the
manually-measured length of the AGX gripper assembly from its mounting face
to the fingertip plane along the gripper's outward z-axis. It is treated as
a fixed constant -- the parallel-gripper midpoint stays on the centerline
regardless of how open the jaws are, so the TCP does not track the live
gripper opening width.

Pass `tip_offset=pin.SE3(...)` to `NeroModel(...)` if you need to point FK
/ IK at a different point (e.g. a custom payload origin).

## Shutdown

On `SIGINT` / `SIGTERM` / `SIGHUP`:

1. The signal handler sets a single shutdown flag; subsequent signals are
   ignored to keep the homing sequence un-interruptible (use `kill -9` to
   force-quit).
2. The arm thread finishes its current control tick, forces itself into
   `disabled` mode, and runs the standard ramp+hold for `RAMP_DURATION_S`
   (3 s) + `HOMING_SETTLE_S` (1 s) so the arm reaches all-zero positions
   under MIT control.
3. The gripper thread finishes its current tick and exits without
   actuating — keeping whatever grasp the operator had.
4. The orchestrator then disconnects the CAN socket; motors stay
   energised at zero so the arm holds its safe pose.

## Tests

```bash
cd robots/nero
pytest -q
```

Tests that need `pinocchio` and the bundled URDF will be skipped when
`pin` is not installed on the host.
