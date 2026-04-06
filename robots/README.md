# Robots

In-repo robot drivers live under `robots/`.

## Layout

- `pseudo/`: synthetic Rust robot driver used by CI and smoke tests.
- `airbot_play/`: Python AIRBOT Play driver package.

## Add A New Robot Driver

1. Create a folder under `robots/<driver_name>/`.
2. Expose either:
   - a binary named `rollio-robot-<driver_name>`, or
   - a Python package with `pyproject.toml` and module name `rollio_<driver_name>`.
3. Implement the driver CLI contract:
   - `probe`
   - `validate`
   - `capabilities`
   - `run --config <path>` or `run --config-inline <toml>`
4. Publish state to `robot/{name}/state`.
5. Consume commands from `robot/{name}/command`.
6. Listen for `control/events` and exit cleanly on shutdown.
7. Keep driver-specific config parsing inside the driver. Shared config now preserves
   extra device keys so new drivers do not need to register every option in
   `rollio-types`.

## Controller Resolution

`rollio collect` resolves robot drivers in this order:

1. In-repo compiled binary at `target/.../rollio-robot-<driver_name>`
2. In-repo Python package at `robots/<driver_name>/`
3. External binary on `PATH`

That means a new robot driver can be added without editing controller dispatch as
long as it follows the naming convention and the iceoryx2 bus contract.
