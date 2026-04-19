"""Rollio device driver for the AGX Nero arm + AGX gripper.

Exposes a `rollio-device-nero` executable that mirrors the iceoryx2 and TOML
contract of the Rust `rollio-device-airbot-play`. The package is laid out so
that pure-Python helpers (config parsing, IPC type layouts, CLI subcommands
that do not require hardware) can be imported without `pyAgxArm`, `pinocchio`
or `iceoryx2` being installed -- those deps are imported lazily by the
`runtime.*` modules and the `gravity` / `ik` helpers.
"""

DRIVER_NAME = "agx-nero"
DEVICE_LABEL = "AGX Nero"
ARM_DOF = 7
GRIPPER_DOF = 1

__all__ = ["ARM_DOF", "DEVICE_LABEL", "DRIVER_NAME", "GRIPPER_DOF"]
