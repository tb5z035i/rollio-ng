"""Device-level orchestrator: spawn arm + gripper threads and join cleanly.

Runs only when `rollio-device-nero run --config ...` is invoked. Handles
SIGINT / SIGTERM / SIGHUP via the same "set a flag, then mask further
signals during shutdown" pattern from
`external/reference/nero-demo/gravity_compensation.py` so a stray Ctrl+C
during the disable-mode ramp cannot orphan the arm in mid-air.
"""

from __future__ import annotations

import signal
import sys
import threading
from contextlib import suppress

from ..config import RuntimeConfig
from ..gravity import NeroModel
from .agx_backend import (
    AgxArmBackend,
    AgxGripperBackend,
    create_robot,
    enable_robot,
    init_gripper,
)
from .arm import ArmController
from .gripper import GripperController
from .iox_ipc import ArmIox, GripperIox

_SHUTDOWN_SIGNALS: list[int] = [
    sig
    for sig in (
        getattr(signal, "SIGINT", None),
        getattr(signal, "SIGTERM", None),
        getattr(signal, "SIGHUP", None),
    )
    if sig is not None
]


def install_shutdown_handler(stop: threading.Event) -> None:
    """Convert SIGINT/SIGTERM/SIGHUP into a single graceful shutdown flag.

    Mirrors `install_shutdown_handler` from
    `external/reference/nero-demo/gravity_compensation.py`. The first
    signal sets `stop`; subsequent signals are no-ops by this handler. To
    force-quit a wedged process, send SIGKILL.
    """

    def handler(signum: int, _frame: object) -> None:
        if stop.is_set():
            return
        try:
            name = signal.Signals(signum).name
        except ValueError:
            name = str(signum)
        print(
            f"[{name}] shutdown requested; arm will home to all-zero "
            "positions before exit (~4 s). Further Ctrl+C is ignored "
            "during homing -- use `kill -9 <pid>` to force-quit.",
            flush=True,
            file=sys.stderr,
        )
        stop.set()

    for sig in _SHUTDOWN_SIGNALS:
        with suppress(OSError, ValueError):
            signal.signal(sig, handler)


def mask_shutdown_signals() -> None:
    """Mask SIGINT/SIGTERM/SIGHUP so the shutdown sequence cannot be aborted."""
    for sig in _SHUTDOWN_SIGNALS:
        with suppress(OSError, ValueError):
            signal.signal(sig, signal.SIG_IGN)


def run_device(config: RuntimeConfig) -> int:
    """Block until shutdown. Returns the process exit code (0 on success).

    The orchestrator owns:

      * one `pyAgxArm` Nero robot (shared CAN socket / read thread);
      * one Pinocchio `NeroModel` (with or without gripper inertia depending on
        whether the gripper channel is configured);
      * one `ArmIox` and/or `GripperIox` (each with its own iceoryx2 node);
      * the per-channel `ArmController` / `GripperController` loops, each in
        its own OS thread (mirrors the airbot Rust device's two-thread model).

    A single `threading.Event` `stop` flag is set by SIGINT/SIGTERM/SIGHUP
    handlers; both controllers poll it via the `stop_check` callable they
    receive in `run()`. Once both threads have joined, we never call
    `robot.disable()`: the arm's `Disabled` mode already drove it to a
    safe pose with motors energised, and `disable()` would let it drop.
    """
    stop = threading.Event()

    robot = create_robot(config.interface)
    if not enable_robot(robot):
        print(
            "rollio-device-nero: warning -- arm did not report enabled within "
            "the retry budget; proceeding anyway (Disabled mode can still hold "
            "the arm at zero).",
            file=sys.stderr,
        )

    model: NeroModel | None = None
    arm_controller: ArmController | None = None
    if config.arm is not None:
        # TEMPORARY: assume the gripper is always physically mounted on the
        # AGX Nero, so the gravity feedforward includes the gripper inertia
        # whether or not the gripper channel is enabled in the runtime config.
        # Otherwise, when the controller spawns the device with only the arm
        # channel (e.g. wizard's per-channel Identify preview), the gravity
        # model underestimates true torque and the arm visibly sags.
        # Eventually the device should probe whether the gripper is wired up
        # (mirroring airbot-play's end-effector detection) and pick the
        # right model accordingly.
        model = NeroModel(with_gripper=True)
        arm_controller = ArmController(
            backend=AgxArmBackend(robot),
            ipc=ArmIox(bus_root=config.bus_root, channel_type=config.arm.channel_type),
            model=model,
            config=config.arm,
        )

    gripper_controller: GripperController | None = None
    if config.gripper is not None:
        gripper_driver = init_gripper(robot)
        gripper_controller = GripperController(
            backend=AgxGripperBackend(gripper_driver),
            ipc=GripperIox(bus_root=config.bus_root, channel_type=config.gripper.channel_type),
            config=config.gripper,
        )

    # Install signal handlers AFTER hardware bring-up so a Ctrl+C during
    # connect/enable still cancels cheaply (no shutdown sequence is
    # meaningful at that point).
    install_shutdown_handler(stop)

    threads: list[threading.Thread] = []

    if arm_controller is not None:
        arm_thread = threading.Thread(
            target=_run_safe,
            args=(arm_controller.run, stop),
            name="rollio-nero-arm",
            daemon=False,
        )
        threads.append(arm_thread)
        arm_thread.start()

    if gripper_controller is not None:
        gripper_thread = threading.Thread(
            target=_run_safe,
            args=(gripper_controller.run, stop),
            name="rollio-nero-gripper",
            daemon=False,
        )
        threads.append(gripper_thread)
        gripper_thread.start()

    try:
        for thread in threads:
            thread.join()
    finally:
        # Mask further signals so a Ctrl+C during the post-loop teardown
        # can't kill the process before disconnect() runs.
        mask_shutdown_signals()
        with suppress(Exception):
            robot.disconnect()

    return 0


def _run_safe(target_run, stop: threading.Event) -> None:
    """Wrap a controller's `run(stop_check)` so an exception flips `stop`.

    Without this, an unhandled exception in the arm thread would silently
    leave the gripper thread spinning until SIGTERM. The wrapper reraises
    after flipping the flag so the parent's `thread.join()` still surfaces
    the traceback.
    """
    try:
        target_run(stop_check=stop.is_set)
    except Exception:
        stop.set()
        raise


__all__ = ["run_device", "install_shutdown_handler", "mask_shutdown_signals"]
