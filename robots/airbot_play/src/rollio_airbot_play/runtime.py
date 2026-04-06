from __future__ import annotations

import time
from collections.abc import Callable
from dataclasses import dataclass

from .backend import AirbotBackend
from .config import AirbotRuntimeConfig
from .gravity import GravityModel, compute_gravity_torques, load_gravity_model
from .ipc import RollioIpc
from .messages import ControlEventTag, JointStateSnapshot, command_targets


@dataclass(slots=True)
class RuntimeResult:
    running: bool
    mode: str


class AirbotRuntime:
    def __init__(
        self,
        *,
        config: AirbotRuntimeConfig,
        backend: AirbotBackend,
        ipc: RollioIpc,
        gravity_model_loader: Callable[[object], GravityModel] = load_gravity_model,
        monotonic_time: Callable[[], float] = time.monotonic,
        sleep: Callable[[float], None] = time.sleep,
    ) -> None:
        self._config = config
        self._backend = backend
        self._ipc = ipc
        self._gravity_model_loader = gravity_model_loader
        self._monotonic_time = monotonic_time
        self._sleep = sleep
        self._mode = config.mode
        self._gravity_model: GravityModel | None = None
        self._latest_joint_targets = [0.0] * config.dof

        if self._mode == "free-drive":
            self._gravity_model = self._load_gravity_model()

    @property
    def mode(self) -> str:
        return self._mode

    def run(self) -> None:
        next_tick = self._monotonic_time()
        while self.step_once().running:
            next_tick += self._config.control_period_s
            sleep_s = max(0.0, next_tick - self._monotonic_time())
            self._sleep(sleep_s)

    def step_once(self) -> RuntimeResult:
        for event in self._ipc.poll_control_events():
            tag = ControlEventTag(int(event.tag))
            if tag is ControlEventTag.SHUTDOWN:
                return RuntimeResult(running=False, mode=self._mode)
            if tag is ControlEventTag.MODE_SWITCH:
                self._set_mode(
                    "free-drive" if int(event.payload.target_mode) == 0 else "command-following"
                )

        if self._mode == "command-following":
            command = self._ipc.poll_latest_command()
            if command is not None:
                self._latest_joint_targets = command_targets(command, self._config.dof)[
                    : self._config.dof
                ]
                self._backend.send_joint_targets(self._latest_joint_targets)

        snapshot = self._backend.read_state()
        if self._mode == "free-drive":
            assert self._gravity_model is not None
            torques = compute_gravity_torques(
                self._gravity_model,
                snapshot.positions[: self._config.dof],
                self._config.gravity_comp_torque_scales,
            )
            self._backend.send_gravity_compensation(
                torques,
                kp=self._config.mit_kp,
                kd=self._config.mit_kd,
            )

        self._ipc.publish_state(
            timestamp_ns=time.time_ns(),
            dof=self._config.dof,
            snapshot=JointStateSnapshot(
                positions=snapshot.positions[: self._config.dof],
                velocities=snapshot.velocities[: self._config.dof],
                efforts=snapshot.efforts[: self._config.dof],
            ),
        )
        return RuntimeResult(running=True, mode=self._mode)

    def close(self) -> None:
        self._ipc.close()
        self._backend.close()

    def _load_gravity_model(self) -> GravityModel:
        assert self._config.model_path is not None
        return self._gravity_model_loader(self._config.model_path)

    def _set_mode(self, mode: str) -> None:
        self._mode = mode
        if mode == "free-drive" and self._gravity_model is None:
            self._gravity_model = self._load_gravity_model()
