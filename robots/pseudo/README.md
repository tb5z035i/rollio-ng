# rollio-device-pseudo

**Synthetic device driver** for CI and local development. It behaves like a real **`rollio-device-*`** binary (same **`probe` / `validate` / `query` / `run`** contract and the same iceoryx2 topic layout) but generates **fake** images and joint motion so you can test **`rollio setup` / `collect`** without hardware.

---

## Concepts & behaviors

### Why this exists

- Validates the **orchestration**, **UI**, and **IPC graph** without flaky hardware.
- **Not** discovered automatically: the controller only injects pseudo IDs when you run **`rollio setup --sim-pseudo N`**. Otherwise `probe` on PATH would never list fake devices.

### Multi-channel pseudo devices

Like a real driver, one **`run`** process can host **multiple channels** (e.g. several cameras + several arms). Each channel:

- Has its own **`channel_type`** segment in topic names under the shared **`bus_root`**.
- For **robot** channels, has its **own mode** stream (`.../control/mode` / `.../info/mode`). One arm can stay in free-drive while another is in command-following.

### Subcommands

#### `probe`

Lists **synthetic** device IDs derived from counts, e.g. `pseudo_camera_0`, `pseudo_robot_0_dof_6`.

- **`--sim-cameras`**, **`--sim-arms`**, **`--dof`** — shape the fake inventory.
- **`--json`** — array of IDs for scripts.

#### `validate <id>`

Checks that `id` is one of the pseudo patterns and (if **`--channel-type`** is repeated) that each named channel type exists on that fake device.

- Exits non-zero if invalid; **`--json`** prints a small report.

#### `query <id>`

Prints a full **`DeviceQueryResponse`** (human text or **`--json`**): labels, fake profiles, joint limits, supported states/commands, teleop self-compatibility metadata. **Setup uses this** to build the wizard UI.

#### `run`

The long-lived driver.

- **Requires** `--config` **or** **`--config-inline`** with a **`BinaryDeviceConfig`** whose **`driver`** field is **`pseudo`**.
- **`--dry-run`** — parse only, exit 0.
- **Spawns one thread per enabled channel**; if any thread errors, the whole process stops.

**Camera channels:** publish moving color bars at the configured FPS.

**Robot channels:** publish configured **state kinds** at `control_frequency_hz` and consume **joint_position** + **joint_mit** commands while in **command-following**. Modes come from IPC (see below).

---

## iceoryx2

### Camera channel

- **Publish:** `{bus_root}/{channel_type}/frames`, `{bus_root}/{channel_type}/info/mode`.
- **Subscribe:** `control/events` — exit on **`Shutdown`**.

### Robot channel

- **Publish:** `{bus_root}/{channel_type}/states/{kind}` for each configured kind; **`.../info/mode`** (current mode echo).
- **Subscribe:** `{bus_root}/{channel_type}/control/mode`; `control/events`; command topics `.../commands/joint_position`, `.../commands/joint_mit`.

### Robot mode ↔ IPC payload (arm)

| `DeviceChannelMode` | What pseudo simulates |
|---------------------|----------------------|
| **Disabled** | Joints held; still timestamps states. |
| **FreeDrive** | Smooth sinusoidal motion (stand-in for “no commands”). |
| **Identifying** | Same motion profile as free-drive (distinguishable only via mode IPC). |
| **CommandFollowing** | Integrates commanded joint targets from the bus. |
| **Enabled** (camera path) | Mapped to arm free-drive where that legacy path appears. |

---

## Lifecycle

**Launched by:** `rollio setup` / `collect` for `driver = "pseudo"`, or manually.

**Children:** OS threads only (no subprocesses).

---

## Built product & dependencies

- **Binary:** `rollio-device-pseudo` (`robots/pseudo`).
- **APT / system:** none beyond standard Rollio Rust build.

## See also

- [`rollio-bus`](../../rollio-bus/README.md), [`robots/README.md`](../README.md).
