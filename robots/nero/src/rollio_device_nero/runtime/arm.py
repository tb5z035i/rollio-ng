"""Arm channel control loop for the AGX Nero device.

The loop is structured around two protocols (`ArmBackend` and `ArmIpc`) so
the four mode behaviours can be unit-tested with in-memory fakes (no
`pyAgxArm`, no `iceoryx2`). The "real" wiring is in `runtime/device.py`.

Per-tick contract (executes at `CONTROL_FREQUENCY_HZ`, currently 250 Hz):

  1. Drain the `control/mode` subscriber; on a transition, run any
     mode-entry book-keeping (e.g. `Disabled` snapshots `q_start`).
  2. Publish the current mode to `info/mode`.
  3. Read `q_meas`, `qd_meas`, `tau_meas`. Skip the tick if `q_meas` is
     unavailable yet (CAN reader still warming up).
  4. Compute `g(q_meas)` via Pinocchio RNEA (clipped to per-joint TAU_MAX).
  5. Compute `(p_des, v_des, kp, kd)` per mode:
        * Disabled: linear ramp `q_start -> DISABLED_HOLD_Q` over
          RAMP_DURATION_S, then hold there. `DISABLED_HOLD_Q` parks the
          arm in a safe, kinematically clear "stand-up" pose
          ([0, 0, 0, pi/2, 0, 0, 0]) rather than fully stretched.
        * Identifying / FreeDrive: `0, 0, 0, FREE_DRIVE_KD`.
        * CommandFollowing: from latest joint_position / joint_mit / end_pose
          command (with IK for end_pose); fall back to `q_meas`-tracking if
          no fresh command in the queue.
  6. Send per-joint `move_mit(i+1, p_des[i], v_des[i], kp, kd, ff[i])`.
  7. Publish state topics from `q_meas`, `qd_meas`, `tau_meas`,
     `Pinocchio FK(q_meas)`.
"""

from __future__ import annotations

import math
import time
from collections.abc import Callable
from dataclasses import dataclass, field
from typing import Protocol

import numpy as np

from .. import ARM_DOF
from ..airbot_aligned_pose import (
    apply_command_pose_fix,
    apply_publish_pose_fix,
)
from ..config import (
    CONTROL_FREQUENCY_HZ,
    DEFAULT_FREE_DRIVE_KD,
    DEFAULT_IDENTIFYING_KD,
    DEFAULT_TRACKING_KD,
    DEFAULT_TRACKING_KP,
    MIN_ACHIEVED_FREQUENCY_RATIO,
    ArmChannelConfig,
)
from ..gravity import NeroModel
from .rate_monitor import RateMonitor
from ..ipc.types import (
    DEVICE_CHANNEL_MODE_COMMAND_FOLLOWING,
    DEVICE_CHANNEL_MODE_DISABLED,
    DEVICE_CHANNEL_MODE_FREE_DRIVE,
    DEVICE_CHANNEL_MODE_IDENTIFYING,
    JointMitCommand15,
    JointVector15,
    Pose7,
)

# Smooth-ramp constants for the `Disabled` mode entry. The arm is driven
# from its current pose to `DISABLED_HOLD_Q` over RAMP_DURATION_S seconds
# with PD tracking + gravity feed-forward, then held at that target
# indefinitely. Lifted directly from `home_on_exit_mit` in
# `external/reference/nero-demo/gravity_compensation.py`, with the hold
# target generalised so we can park the arm in a kinematically clear
# pose rather than fully stretched.
RAMP_DURATION_S: float = 3.0
RAMP_KP: float = DEFAULT_TRACKING_KP
RAMP_KD: float = DEFAULT_TRACKING_KD

# Joint vector that the `Disabled` mode rolls to and holds at. Per the
# operator spec this is `[0, 0, 0, pi/2, 0, 0, 0]` -- joint 4 is bent
# 90 deg so the elbow is up and the forearm is folded over the shoulder,
# keeping the wrist + gripper away from the workspace and out of any
# kinematic singularity. All other joints sit at zero. The motors stay
# energised at this hold; the arm does NOT power down.
DISABLED_HOLD_Q: np.ndarray = np.array(
    [0.0, 0.0, 0.0, math.pi / 2.0, 0.0, 0.0, math.pi / 2.0],
    dtype=float,
)

# Default settle time held *after* the ramp completes during the
# `home_on_exit` shutdown phase. With the ramp duration above we wait
# `RAMP_DURATION_S + HOMING_SETTLE_S` seconds before the run loop returns,
# matching the `--exit-settle` default in
# `external/reference/nero-demo/gravity_compensation.py`.
HOMING_SETTLE_S: float = 1.0
HOMING_FEEDBACK_WAIT_S: float = 0.5

# Hard per-tick safety bound on the per-joint delta between the commanded
# target and the current feedback position while in `CommandFollowing`.
# Mirrors the AIRBOT Play `MAX_COMMAND_JOINT_DELTA_RAD` (see
# `third_party/airbot-play-rust/src/arm/play.rs`): any oversized request
# is clipped to within this many radians of the present joint angle so an
# upstream glitch (a stale teleop snapshot, a corrupted IK seed, etc.)
# cannot snap the arm. 5 deg is loose enough that intentional teleop
# motion is never clipped at 100 Hz control (~8.7 rad/s slew cap) but
# tight enough to catch obvious outliers.
MAX_COMMAND_JOINT_DELTA_RAD: float = math.pi / 12.0


# ---------------------------------------------------------------------------
# Protocols (so runtime can be unit-tested without pyAgxArm / iceoryx2)
# ---------------------------------------------------------------------------


class ArmBackend(Protocol):
    """Subset of the pyAgxArm Nero driver API used by the arm runtime."""

    def get_joint_angles_array(self) -> np.ndarray | None: ...

    def get_joint_velocities_array(self) -> np.ndarray | None: ...

    def get_joint_efforts_array(self) -> np.ndarray | None: ...

    def move_mit(
        self,
        joint_index: int,
        p_des: float,
        v_des: float,
        kp: float,
        kd: float,
        t_ff: float,
    ) -> None: ...


class ArmIpc(Protocol):
    """Subset of iceoryx2 traffic used by the arm runtime."""

    def poll_mode_change(self) -> int | None: ...

    def publish_mode(self, mode_value: int) -> None: ...

    def poll_joint_position_command(self) -> JointVector15 | None: ...

    def poll_joint_mit_command(self) -> JointMitCommand15 | None: ...

    def poll_end_pose_command(self) -> Pose7 | None: ...

    def publish_joint_position(self, msg: JointVector15) -> None: ...

    def publish_joint_velocity(self, msg: JointVector15) -> None: ...

    def publish_joint_effort(self, msg: JointVector15) -> None: ...

    def publish_end_effector_pose(self, msg: Pose7) -> None: ...

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


def mode_value_for_config(config: ArmChannelConfig) -> int:
    return _MODE_BY_NAME[config.mode]


# ---------------------------------------------------------------------------
# Controller
# ---------------------------------------------------------------------------


@dataclass(slots=True)
class _DisabledRamp:
    """State for the `Disabled` mode's smooth ramp + hold transition.

    Linearly interpolates each joint from `q_start` to `q_end` over
    `duration_s` seconds, then holds at `q_end`. `q_end` defaults to
    [`DISABLED_HOLD_Q`] (the operator-spec parking pose).
    """

    q_start: np.ndarray
    started_at: float
    duration_s: float = RAMP_DURATION_S
    q_end: np.ndarray = field(default_factory=lambda: DISABLED_HOLD_Q.copy())

    def desired(self, now: float) -> tuple[np.ndarray, np.ndarray]:
        """Return (p_des, v_des) at `now` along the `q_start -> q_end` ramp."""
        elapsed = max(0.0, now - self.started_at)
        if self.duration_s <= 0.0 or elapsed >= self.duration_s:
            return self.q_end.copy(), np.zeros_like(self.q_end)
        alpha = elapsed / self.duration_s
        p_des = self.q_start + (self.q_end - self.q_start) * alpha
        v_des = (self.q_end - self.q_start) / self.duration_s
        return p_des, v_des


@dataclass(slots=True)
class ArmTickResult:
    """Per-tick observability output (used by tests)."""

    mode_value: int
    sent_targets: list[tuple[int, float, float, float, float, float]]
    published_states: list[str]


class ArmController:
    """Per-tick control loop for the AGX Nero arm channel.

    Owns the desired mode + the per-mode entry book-keeping. Does NOT own
    the IPC node itself (the device-level orchestrator does); the
    controller talks to whatever object satisfies `ArmIpc`. Likewise, IK is
    pluggable so tests can inject a deterministic IK without pinocchio.
    """

    def __init__(
        self,
        *,
        backend: ArmBackend,
        ipc: ArmIpc,
        model: NeroModel,
        config: ArmChannelConfig,
        ik_solver: Callable[..., tuple[np.ndarray, bool, float]] | None = None,
        clock: Callable[[], float] = time.monotonic,
    ) -> None:
        self._backend = backend
        self._ipc = ipc
        self._model = model
        self._config = config
        self._clock = clock
        self._mode_value: int = mode_value_for_config(config)
        self._disabled_ramp: _DisabledRamp | None = None
        self._latest_joint_target: np.ndarray | None = None
        # Last clamped `p_des` actually sent to the motors. Used by the
        # safety clamp (`_clamp_p_des_to_max_joint_delta`) as the
        # reference point so a stale host-side `q_meas` cannot leak its
        # ~60 Hz update quantisation into the 250 Hz `p_des` stream. See
        # `_clamp_p_des_to_max_joint_delta` for the full rationale.
        # Initialised to `None`; the clamp falls back to `q_meas` on the
        # very first tick after entering `CommandFollowing` so the arm
        # never receives a `p_des` step larger than the clamp from
        # whatever pose it happens to be in at mode entry.
        self._last_sent_p_des: np.ndarray | None = None
        # Most recent CLIK output, used as the IK seed for the next
        # cartesian command. Distinct from `_latest_joint_target` (which
        # is also set by joint_position / joint_mit commands) so the
        # "first IK call after entering CommandFollowing" path
        # deterministically falls back to live feedback even if the
        # operator was previously commanding raw joints.
        self._latest_ik_target: np.ndarray | None = None

        if ik_solver is None:
            from ..ik import solve as default_ik

            self._ik = default_ik
        else:
            self._ik = ik_solver

    # ----- public surface -----

    @property
    def mode_value(self) -> int:
        return self._mode_value

    def step(self, *, accept_mode_changes: bool = True) -> ArmTickResult:
        """Execute one control tick and return what was emitted (for tests).

        When `accept_mode_changes=False`, incoming `control/mode` messages
        are not consulted -- used during the `home_on_exit` shutdown phase
        so a stray late-arriving mode-switch from the controller cannot
        abort the homing ramp.
        """
        sent: list[tuple[int, float, float, float, float, float]] = []
        published: list[str] = []

        if accept_mode_changes:
            new_mode = self._ipc.poll_mode_change()
            if new_mode is not None and new_mode != self._mode_value:
                self._on_mode_change(new_mode)

        self._ipc.publish_mode(self._mode_value)

        q_meas = self._backend.get_joint_angles_array()
        if q_meas is None:
            return ArmTickResult(
                mode_value=self._mode_value,
                sent_targets=sent,
                published_states=published,
            )

        q_meas = np.asarray(q_meas, dtype=float)[:ARM_DOF]
        if q_meas.shape != (ARM_DOF,):
            return ArmTickResult(
                mode_value=self._mode_value,
                sent_targets=sent,
                published_states=published,
            )

        # Defensive: lazily snapshot q_start the first time we hit a
        # disabled-mode tick. Avoids crashing if the controller starts in
        # Disabled before we ever observed q_meas.
        if self._mode_value == DEVICE_CHANNEL_MODE_DISABLED and self._disabled_ramp is None:
            self._disabled_ramp = _DisabledRamp(
                q_start=q_meas.copy(), started_at=self._clock()
            )

        ff = self._model.gravity_torques_clipped(q_meas)

        p_des, v_des, kp, kd = self._desired(q_meas)

        for i in range(ARM_DOF):
            self._backend.move_mit(
                joint_index=i + 1,
                p_des=float(p_des[i]),
                v_des=float(v_des[i]),
                kp=float(kp),
                kd=float(kd),
                t_ff=float(ff[i]),
            )
            sent.append(
                (
                    i + 1,
                    float(p_des[i]),
                    float(v_des[i]),
                    float(kp),
                    float(kd),
                    float(ff[i]),
                )
            )

        published.extend(self._publish_states(q_meas))

        return ArmTickResult(
            mode_value=self._mode_value,
            sent_targets=sent,
            published_states=published,
        )

    def run(
        self,
        stop_check: Callable[[], bool],
        *,
        home_on_exit: bool = True,
        homing_settle_s: float = HOMING_SETTLE_S,
    ) -> None:
        """Block until `stop_check()` is True, ticking at the configured rate.

        When `home_on_exit=True` (the default), the shutdown sequence forces
        the controller into Disabled mode (which snapshots `q_start = q_meas`
        and starts the linear ramp toward `DISABLED_HOLD_Q`) and keeps
        ticking for `RAMP_DURATION_S + homing_settle_s` so the ramp can
        complete and a small settle period is held at the target. The
        motors are NEVER disabled -- they keep holding the parking pose
        with kp=10, kd=0.5 until the orchestrator disconnects the CAN
        socket.
        """
        period = 1.0 / CONTROL_FREQUENCY_HZ
        next_tick = self._clock()
        rate_monitor = RateMonitor(
            target_hz=CONTROL_FREQUENCY_HZ,
            min_ratio=MIN_ACHIEVED_FREQUENCY_RATIO,
            label="arm",
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

        if home_on_exit:
            self._home_to_disabled_hold(settle_s=homing_settle_s)

    def _home_to_disabled_hold(self, *, settle_s: float = HOMING_SETTLE_S) -> None:
        """Drive the arm to `DISABLED_HOLD_Q` before returning from `run`.

        Force the controller into Disabled mode (so `_DisabledRamp` snapshots
        `q_start = q_meas` and starts a linear ramp to `DISABLED_HOLD_Q`),
        then keep emitting MIT commands for `RAMP_DURATION_S + settle_s`
        seconds. Mode-change polling is disabled during this phase so a
        stray late mode-switch from the controller cannot abort the homing.

        If `q_meas` is unavailable when this method is called (e.g. the CAN
        reader hasn't produced a frame yet), poll briefly before snapshotting
        so the ramp starts from the actual pose rather than zero.
        """
        # Wait briefly for fresh joint feedback so the ramp begins from the
        # arm's actual pose, not the (potentially-stale) zero default.
        deadline = self._clock() + HOMING_FEEDBACK_WAIT_S
        while (
            self._backend.get_joint_angles_array() is None
            and self._clock() < deadline
        ):
            time.sleep(0.01)

        # Force-enter Disabled. _on_mode_change clears any prior state and
        # resnapshots q_start from the latest q_meas (or zero if still None).
        if self._mode_value != DEVICE_CHANNEL_MODE_DISABLED:
            self._on_mode_change(DEVICE_CHANNEL_MODE_DISABLED)
        else:
            # Already in Disabled -- e.g. the operator parked the arm there
            # before quitting. Reset the ramp so we start from the current
            # pose, not whatever q_start was captured on entry.
            self._disabled_ramp = None

        period = 1.0 / CONTROL_FREQUENCY_HZ
        homing_deadline = self._clock() + RAMP_DURATION_S + max(0.0, settle_s)
        next_tick = self._clock()
        while self._clock() < homing_deadline:
            self.step(accept_mode_changes=False)
            next_tick += period
            sleep_s = next_tick - self._clock()
            if sleep_s > 0:
                time.sleep(sleep_s)
            else:
                next_tick = self._clock()

    # ----- internals -----

    def _on_mode_change(self, next_mode: int) -> None:
        # Leaving Disabled clears the ramp snapshot so a future re-entry
        # captures the new q_start.
        if self._mode_value == DEVICE_CHANNEL_MODE_DISABLED:
            self._disabled_ramp = None

        if next_mode == DEVICE_CHANNEL_MODE_DISABLED:
            q_meas = self._backend.get_joint_angles_array()
            q_start = (
                np.asarray(q_meas, dtype=float)[:ARM_DOF]
                if q_meas is not None
                else np.zeros(ARM_DOF)
            )
            self._disabled_ramp = _DisabledRamp(q_start=q_start.copy(), started_at=self._clock())

        # Entering CommandFollowing without a fresh command would otherwise
        # send a stale joint target (or zero) on the first tick; clear so
        # the fallback path ("track q_meas") engages until a real command
        # arrives. The CLIK seed cache is cleared with the same logic so
        # the very first cartesian command after enabling seeds from
        # live feedback rather than from a stale CLIK output.
        if next_mode == DEVICE_CHANNEL_MODE_COMMAND_FOLLOWING:
            self._latest_joint_target = None
            self._latest_ik_target = None
            # First clamp after re-entering CF should anchor at q_meas
            # (we don't know where the arm is until that first tick),
            # not at whatever stale p_des was sent before leaving CF.
            self._last_sent_p_des = None

        self._mode_value = next_mode

    def _desired(self, q_meas: np.ndarray) -> tuple[np.ndarray, np.ndarray, float, float]:
        if self._mode_value == DEVICE_CHANNEL_MODE_DISABLED:
            assert self._disabled_ramp is not None
            p_des, v_des = self._disabled_ramp.desired(self._clock())
            return p_des, v_des, RAMP_KP, RAMP_KD

        if self._mode_value == DEVICE_CHANNEL_MODE_FREE_DRIVE:
            # Truly floating arm: no PD, only gravity feed-forward. The
            # operator can move it by hand without fighting MIT damping.
            return (
                np.zeros(ARM_DOF),
                np.zeros(ARM_DOF),
                0.0,
                DEFAULT_FREE_DRIVE_KD,
            )

        if self._mode_value == DEVICE_CHANNEL_MODE_IDENTIFYING:
            # Same control shape as FreeDrive (kp=0, kd=0, ff=g(q)). Only
            # the reported mode differs so the rollio setup wizard can
            # highlight this state independently.
            return (
                np.zeros(ARM_DOF),
                np.zeros(ARM_DOF),
                0.0,
                DEFAULT_IDENTIFYING_KD,
            )

        # CommandFollowing
        p_des, v_des, kp, kd = self._desired_command_following(q_meas)
        # Hard per-tick safety bound -- any inbound command (joint
        # position / joint MIT / cartesian-via-IK) and the held last
        # target are clipped to within `MAX_COMMAND_JOINT_DELTA_RAD`
        # per tick. The reference point for the clamp is the previously
        # *sent* p_des rather than the live q_meas: NERO's host-side
        # CAN feedback only updates at ~60 Hz, so anchoring the clamp
        # to q_meas aliases that quantisation into the 250 Hz p_des
        # stream (we measured up to 26 mrad p_des steps in CF, vs
        # 2.7 mrad uniform steps in Disabled mode -- the only
        # difference being Disabled's p_des is a pure ramp independent
        # of q_meas). Clamping against the prior p_des still bounds
        # the worst-case per-tick joint velocity at the same value
        # (~22 rad/s slew cap), preserving the original safety
        # invariant. We keep `_latest_joint_target` as the *unclamped*
        # IK output so the held-target fallback keeps walking toward
        # the original goal one tick at a time.
        ref = self._last_sent_p_des if self._last_sent_p_des is not None else q_meas
        p_des = _clamp_p_des_to_max_joint_delta(p_des, ref)
        self._last_sent_p_des = p_des
        return p_des, v_des, kp, kd

    def _desired_command_following(
        self, q_meas: np.ndarray
    ) -> tuple[np.ndarray, np.ndarray, float, float]:
        kp = DEFAULT_TRACKING_KP
        kd = DEFAULT_TRACKING_KD

        # Try sources in priority order: end_pose (cartesian) > joint_mit > joint_position.
        end_pose = self._ipc.poll_end_pose_command()
        if end_pose is not None:
            # Cartesian commands arrive in the AIRBOT-aligned reporting
            # frame (see `airbot_aligned_pose`). Convert position +
            # orientation back to Nero's native base/TCP frame before IK
            # so the solver and the published reports stay in sync.
            target7 = apply_command_pose_fix(
                [float(end_pose.values[i]) for i in range(7)]
            )
            # Seed CLIK from the *previous CLIK output*, falling back
            # to live feedback only on the first IK call (no previous
            # CLIK output yet). Nero is 7-DOF (one redundant joint) so
            # the damped pseudo-inverse converges to whichever
            # null-space configuration is closest to the seed; using
            # live feedback every tick closes a positive-feedback loop
            # with the joint controller (small feedback drift ->
            # different null-space branch -> arm jerks toward it ->
            # more drift) that shows up as visible oscillation when
            # the cartesian target is near-static. The previous CLIK
            # output is stable for a stable pose and so picks the same
            # configuration every cycle. Mirrors the AIRBOT Play seed
            # policy in `third_party/airbot-play-rust/src/arm/play.rs`.
            #
            # Default fallback: if `_latest_ik_target` is None (the
            # very first cartesian command after entering
            # CommandFollowing), seed with `q_meas` -- the current
            # joint positions -- rather than with whatever stale
            # joint target may be cached from a previous joint-mode
            # command. `_on_mode_change` clears `_latest_ik_target`
            # on entry so the fallback engages cleanly across mode
            # transitions.
            ik_seed = (
                self._latest_ik_target
                if self._latest_ik_target is not None
                else q_meas
            )
            # Pass `q_meas` as the null-space anchor: with Nero's 7-DOF
            # arm, the bare damped pseudo-inverse warm-started from the
            # previous IK output drifts along the null space tick-to-tick
            # (elbow swings dozens of degrees while the EE barely moves).
            # Anchoring to the live joint positions collapses that
            # null-space freedom onto a single stable configuration so
            # `q_target` walks smoothly with the cartesian target instead
            # of wandering through redundant configurations.
            q_target, _conv, _err = self._ik(
                self._model, target7, q0=ik_seed, q_anchor=q_meas
            )
            self._latest_ik_target = q_target
            self._latest_joint_target = q_target
            return q_target, np.zeros(ARM_DOF), kp, kd

        mit = self._ipc.poll_joint_mit_command()
        if mit is not None:
            n = min(int(mit.len), ARM_DOF)
            p_des = np.array(
                [float(mit.position[i]) for i in range(ARM_DOF)] if n == ARM_DOF
                else (
                    [float(mit.position[i]) for i in range(n)] + list(q_meas[n:])
                ),
                dtype=float,
            )
            v_des = np.array(
                [float(mit.velocity[i]) for i in range(ARM_DOF)] if n == ARM_DOF
                else [float(mit.velocity[i]) for i in range(n)] + [0.0] * (ARM_DOF - n),
                dtype=float,
            )
            # Honour per-message kp/kd if non-zero, else fall back to defaults.
            kp_msg = float(mit.kp[0]) if n > 0 else 0.0
            kd_msg = float(mit.kd[0]) if n > 0 else 0.0
            self._latest_joint_target = p_des
            return (
                p_des,
                v_des,
                kp_msg if kp_msg > 0.0 else kp,
                kd_msg if kd_msg > 0.0 else kd,
            )

        joint_pos = self._ipc.poll_joint_position_command()
        if joint_pos is not None:
            n = min(int(joint_pos.len), ARM_DOF)
            p_des = np.array(
                [float(joint_pos.values[i]) for i in range(n)]
                + list(q_meas[n:]),
                dtype=float,
            )
            self._latest_joint_target = p_des
            return p_des, np.zeros(ARM_DOF), kp, kd

        # No fresh command this tick: reuse the last joint target if we had
        # one, else hold at the current measured pose.
        p_des = (
            self._latest_joint_target
            if self._latest_joint_target is not None
            else q_meas.copy()
        )
        return p_des, np.zeros(ARM_DOF), kp, kd

    def _publish_states(self, q_meas: np.ndarray) -> list[str]:
        published: list[str] = []
        timestamp_ms = _unix_ms()
        publish_states = self._config.publish_states or [
            "joint_position",
            "joint_velocity",
            "joint_effort",
            "end_effector_pose",
        ]

        if "joint_position" in publish_states:
            self._ipc.publish_joint_position(
                JointVector15.from_values(timestamp_ms, list(q_meas))
            )
            published.append("joint_position")

        if "joint_velocity" in publish_states:
            qd = self._backend.get_joint_velocities_array()
            if qd is not None:
                self._ipc.publish_joint_velocity(
                    JointVector15.from_values(
                        timestamp_ms, [float(v) for v in np.asarray(qd)[:ARM_DOF]]
                    )
                )
                published.append("joint_velocity")

        if "joint_effort" in publish_states:
            tau = self._backend.get_joint_efforts_array()
            if tau is not None:
                self._ipc.publish_joint_effort(
                    JointVector15.from_values(
                        timestamp_ms, [float(t) for t in np.asarray(tau)[:ARM_DOF]]
                    )
                )
                published.append("joint_effort")

        if "end_effector_pose" in publish_states:
            # Translate the native Nero pose (position + orientation)
            # into the AIRBOT-aligned reporting frame. The Nero base is
            # mounted 180 degrees rotated about z relative to AIRBOT,
            # so position needs the same `q_base` rotation that
            # orientation gets -- otherwise a Cartesian teleop follower
            # sees x/y mirrored.
            pose = apply_publish_pose_fix(self._model.end_effector_pose7(q_meas))
            self._ipc.publish_end_effector_pose(Pose7.from_values(timestamp_ms, pose))
            published.append("end_effector_pose")

        return published


def _unix_ms() -> int:
    return int(time.time() * 1000.0) & 0xFFFFFFFFFFFFFFFF


def _clamp_p_des_to_max_joint_delta(
    p_des: np.ndarray, q_meas: np.ndarray
) -> np.ndarray:
    """Clamp each `p_des[i]` to within `MAX_COMMAND_JOINT_DELTA_RAD` of `q_meas[i]`.

    Returns a fresh ndarray so the controller's `_latest_joint_target`
    cache (which keeps the *unclamped* goal) is not mutated -- a held
    target keeps walking toward the original goal one tick at a time
    rather than stalling at the first clamp.
    """
    delta = np.clip(
        p_des - q_meas,
        -MAX_COMMAND_JOINT_DELTA_RAD,
        MAX_COMMAND_JOINT_DELTA_RAD,
    )
    return q_meas + delta


__all__ = [
    "RAMP_DURATION_S",
    "RAMP_KP",
    "RAMP_KD",
    "HOMING_SETTLE_S",
    "HOMING_FEEDBACK_WAIT_S",
    "MAX_COMMAND_JOINT_DELTA_RAD",
    "DISABLED_HOLD_Q",
    "ArmBackend",
    "ArmIpc",
    "ArmController",
    "ArmTickResult",
    "mode_value_for_config",
]
