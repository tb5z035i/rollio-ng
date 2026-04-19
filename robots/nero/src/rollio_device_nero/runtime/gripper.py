"""Gripper channel control loop for the AGX Nero device.

Mirrors the AIRBOT G2 contract on the iceoryx2 wire (`parallel_position`
/ `parallel_velocity` / `parallel_effort` states + `parallel_position` /
`parallel_mit` commands), but talks to the AGX vendor gripper through
`pyAgxArm`'s `init_effector("agx_gripper")` driver.

Modes:

  * Disabled: stop sending commands; never call `disable_gripper()` so the
    gripper keeps its grip on whatever it is holding.
  * Identifying: open/close sine pattern between 0 and `MAX_WIDTH_M` at
    `IDENTIFY_PERIOD_S`-second period -- mirrors `identifying_g2_command`
    in `robots/airbot_play_rust/src/bin/device.rs`.
  * FreeDrive: hold (no commands).
  * CommandFollowing: drain `parallel_position` / `parallel_mit` and forward
    `value=command.position[0]` to `move_gripper_m(value, force)` with
    `force = command.kp[0]` if non-zero, else `config.default_force_n`.
"""

from __future__ import annotations

import time
from collections.abc import Callable
from dataclasses import dataclass
from typing import Protocol

from .. import GRIPPER_DOF
from ..config import (
    CONTROL_FREQUENCY_HZ,
    MIN_ACHIEVED_FREQUENCY_RATIO,
    GripperChannelConfig,
)
from ..ipc.types import (
    DEVICE_CHANNEL_MODE_COMMAND_FOLLOWING,
    DEVICE_CHANNEL_MODE_DISABLED,
    DEVICE_CHANNEL_MODE_FREE_DRIVE,
    DEVICE_CHANNEL_MODE_IDENTIFYING,
    ParallelMitCommand2,
    ParallelVector2,
)
from .rate_monitor import RateMonitor

MAX_WIDTH_M: float = 0.07
IDENTIFY_PERIOD_S: float = 2.0  # 0..max..0 cycle -- matches airbot G2.


# ---------------------------------------------------------------------------
# Protocols (so the loop can be unit-tested with fakes)
# ---------------------------------------------------------------------------


class GripperBackend(Protocol):
    """Subset of the pyAgxArm AGX gripper effector API used by the gripper runtime."""

    def get_gripper_position_m(self) -> float | None: ...

    def get_gripper_velocity_m_per_s(self) -> float | None: ...

    def get_gripper_effort_n(self) -> float | None: ...

    def move_gripper_m(self, value: float, force: float) -> None: ...


class GripperIpc(Protocol):
    """Subset of iceoryx2 traffic used by the gripper runtime."""

    def poll_mode_change(self) -> int | None: ...

    def publish_mode(self, mode_value: int) -> None: ...

    def poll_parallel_position_command(self) -> ParallelVector2 | None: ...

    def poll_parallel_mit_command(self) -> ParallelMitCommand2 | None: ...

    def publish_parallel_position(self, msg: ParallelVector2) -> None: ...

    def publish_parallel_velocity(self, msg: ParallelVector2) -> None: ...

    def publish_parallel_effort(self, msg: ParallelVector2) -> None: ...

    def shutdown_requested(self) -> bool: ...


# ---------------------------------------------------------------------------
# Mode mapping
# ---------------------------------------------------------------------------

_MODE_BY_NAME: dict[str, int] = {
    "free-drive": DEVICE_CHANNEL_MODE_FREE_DRIVE,
    "command-following": DEVICE_CHANNEL_MODE_COMMAND_FOLLOWING,
    "identifying": DEVICE_CHANNEL_MODE_IDENTIFYING,
    "disabled": DEVICE_CHANNEL_MODE_DISABLED,
}


def mode_value_for_config(config: GripperChannelConfig) -> int:
    return _MODE_BY_NAME[config.mode]


# ---------------------------------------------------------------------------
# Identifying pattern
# ---------------------------------------------------------------------------


def identify_target(elapsed_s: float, max_width_m: float = MAX_WIDTH_M) -> float:
    """Triangle-wave open/close pattern, period = IDENTIFY_PERIOD_S.

    Matches the shape of `identifying_g2_command` in the airbot device:
    `phase < 1.0 -> 0.07 * phase`, `phase >= 1.0 -> 0.07 * (2.0 - phase)`.
    Using a triangle wave (vs a sine) gives a constant-velocity stroke
    that is easier to eyeball during setup.
    """
    half_period = IDENTIFY_PERIOD_S * 0.5
    if half_period <= 0.0:
        return 0.0
    phase = (elapsed_s % IDENTIFY_PERIOD_S) / half_period  # 0..2
    if phase < 1.0:
        return max_width_m * phase
    return max_width_m * (2.0 - phase)


# ---------------------------------------------------------------------------
# Controller
# ---------------------------------------------------------------------------


@dataclass(slots=True)
class GripperTickResult:
    mode_value: int
    sent_target_m: float | None
    sent_force_n: float | None
    published_states: list[str]


class GripperController:
    def __init__(
        self,
        *,
        backend: GripperBackend,
        ipc: GripperIpc,
        config: GripperChannelConfig,
        clock: Callable[[], float] = time.monotonic,
    ) -> None:
        self._backend = backend
        self._ipc = ipc
        self._config = config
        self._clock = clock
        self._mode_value: int = mode_value_for_config(config)
        self._identify_started_at: float | None = (
            self._clock() if self._mode_value == DEVICE_CHANNEL_MODE_IDENTIFYING else None
        )

    @property
    def mode_value(self) -> int:
        return self._mode_value

    def step(self) -> GripperTickResult:
        published: list[str] = []
        sent_target: float | None = None
        sent_force: float | None = None

        new_mode = self._ipc.poll_mode_change()
        if new_mode is not None and new_mode != self._mode_value:
            self._on_mode_change(new_mode)

        self._ipc.publish_mode(self._mode_value)

        # Identifying: emit the open/close pattern.
        if self._mode_value == DEVICE_CHANNEL_MODE_IDENTIFYING:
            assert self._identify_started_at is not None
            elapsed = max(0.0, self._clock() - self._identify_started_at)
            target = identify_target(elapsed)
            force = self._config.default_force_n
            self._backend.move_gripper_m(target, force)
            sent_target, sent_force = target, force

        # CommandFollowing: forward the latest queued command.
        elif self._mode_value == DEVICE_CHANNEL_MODE_COMMAND_FOLLOWING:
            mit = self._ipc.poll_parallel_mit_command()
            if mit is not None and int(mit.len) >= 1:
                target = float(mit.position[0])
                # Per airbot's G2 contract, kp slot is overloaded as the
                # commanded force (N). Fall back to the channel default
                # when the controller leaves it at zero.
                force_msg = float(mit.kp[0])
                force = force_msg if force_msg > 0.0 else self._config.default_force_n
                target = _clip_width(target)
                self._backend.move_gripper_m(target, force)
                sent_target, sent_force = target, force
            else:
                pos = self._ipc.poll_parallel_position_command()
                if pos is not None and int(pos.len) >= 1:
                    target = _clip_width(float(pos.values[0]))
                    force = self._config.default_force_n
                    self._backend.move_gripper_m(target, force)
                    sent_target, sent_force = target, force

        # Disabled / FreeDrive: drain the queues so the controller's old
        # commands don't pile up in shared memory, but do not actuate.
        else:
            _ = self._ipc.poll_parallel_mit_command()
            _ = self._ipc.poll_parallel_position_command()

        published.extend(self._publish_states())

        return GripperTickResult(
            mode_value=self._mode_value,
            sent_target_m=sent_target,
            sent_force_n=sent_force,
            published_states=published,
        )

    def run(self, stop_check: Callable[[], bool]) -> None:
        period = 1.0 / CONTROL_FREQUENCY_HZ
        next_tick = self._clock()
        rate_monitor = RateMonitor(
            target_hz=CONTROL_FREQUENCY_HZ,
            min_ratio=MIN_ACHIEVED_FREQUENCY_RATIO,
            label="gripper",
            clock=self._clock,
        )
        while not stop_check() and not self._ipc.shutdown_requested():
            self.step()
            rate_monitor.record_tick()
            next_tick += period
            sleep_s = next_tick - self._clock()
            if sleep_s > 0:
                time.sleep(sleep_s)
            else:
                # We're behind. Realign so we don't spin trying to catch up.
                # The RateMonitor will surface a warning if this happens
                # consistently over a 5 s window.
                next_tick = self._clock()

    # ----- internals -----

    def _on_mode_change(self, next_mode: int) -> None:
        if next_mode == DEVICE_CHANNEL_MODE_IDENTIFYING:
            self._identify_started_at = self._clock()
        elif self._mode_value == DEVICE_CHANNEL_MODE_IDENTIFYING:
            self._identify_started_at = None
        self._mode_value = next_mode

    def _publish_states(self) -> list[str]:
        published: list[str] = []
        timestamp_ms = _unix_ms()
        publish_states = self._config.publish_states or [
            "parallel_position",
            "parallel_velocity",
            "parallel_effort",
        ]

        if "parallel_position" in publish_states:
            value = self._backend.get_gripper_position_m()
            if value is not None:
                self._ipc.publish_parallel_position(
                    ParallelVector2.from_values(timestamp_ms, [float(value)])
                )
                published.append("parallel_position")

        if "parallel_velocity" in publish_states:
            value = self._backend.get_gripper_velocity_m_per_s()
            if value is not None:
                self._ipc.publish_parallel_velocity(
                    ParallelVector2.from_values(timestamp_ms, [float(value)])
                )
                published.append("parallel_velocity")

        if "parallel_effort" in publish_states:
            value = self._backend.get_gripper_effort_n()
            if value is not None:
                self._ipc.publish_parallel_effort(
                    ParallelVector2.from_values(timestamp_ms, [float(value)])
                )
                published.append("parallel_effort")

        return published


def _clip_width(value: float) -> float:
    if value < 0.0:
        return 0.0
    if value > MAX_WIDTH_M:
        return MAX_WIDTH_M
    return value


def _unix_ms() -> int:
    return int(time.time() * 1000.0) & 0xFFFFFFFFFFFFFFFF


__all__ = [
    "GRIPPER_DOF",
    "IDENTIFY_PERIOD_S",
    "MAX_WIDTH_M",
    "GripperBackend",
    "GripperController",
    "GripperIpc",
    "GripperTickResult",
    "identify_target",
    "mode_value_for_config",
]
