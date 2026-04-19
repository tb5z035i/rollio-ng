# Robots

In-repo robot drivers live under `robots/`.

## Layout

- `pseudo/`: synthetic Rust device driver (`rollio-device-pseudo`) used by CI
  and smoke tests. Opt-in via the controller's `--sim-pseudo N` flag.
- `airbot_play/`: Python AIRBOT Play wrapper.
- `airbot_play_rust/`: Rust AIRBOT Play driver (`rollio-device-airbot-play`).
- `nero/`: Python AGX Nero driver (`rollio-device-agx-nero`).

## Add A New Robot Driver

Robot drivers are now just *device drivers* — there is no separate naming or
registration story for cameras vs robots. A single device may expose camera
channels, robot channels, or a mix of both.

1. Create a folder under `robots/<driver_name>/` (or `cameras/`, doesn't
   matter — the framework only cares about the executable basename).
2. Expose either:
   - a binary named `rollio-device-<driver_name>`, or
   - a Python package with `pyproject.toml` that installs a console script
     called `rollio-device-<driver_name>`.
3. Implement the device CLI contract:
   - `probe [--json]`
   - `validate <id> [--channel-type ...] [--json]`
   - `query <id> [--json]` -- returns a `DeviceQueryResponse` (see
     [rollio-types/src/config.rs](../rollio-types/src/config.rs))
   - `run --config <path>` or `run --config-inline <toml>`
4. Publish per-channel state to `{bus_root}/{channel_type}/states/<state>`
   and consume commands from `{bus_root}/{channel_type}/commands/<cmd>`
   (see [design/device-as-binaries.md](../design/device-as-binaries.md)).
5. Listen for `control/events` and exit cleanly on shutdown.
6. Drive your `query --json` to populate every field the controller reads:
   `device_label`, `default_device_name`, per-channel `kind`, `channel_label`,
   `default_name`, `modes`, `profiles`, `dof`, `default_control_frequency_hz`,
   `defaults`, `value_limits`, `supported_states`, `supported_commands`,
   `direct_joint_compatibility`. The framework no longer maintains any
   per-driver lookup tables.

## Controller Resolution

`rollio collect` resolves device drivers in this order:

1. In-repo compiled binary at `target/.../rollio-device-<driver_name>`
2. Anywhere else in the controller's directory or workspace `cameras/build`
3. Any executable on `$PATH` whose name starts with `rollio-device-`

That means a new device driver can be installed with `pip install` /
`cargo install` and gets picked up by `rollio setup` automatically (apart
from `rollio-device-pseudo`, which is opt-in via `--sim-pseudo`).
