from __future__ import annotations

import socket
from contextlib import suppress
from dataclasses import dataclass
from typing import Any, Protocol

from .config import AirbotRuntimeConfig
from .messages import JointStateSnapshot


class BackendUnavailableError(RuntimeError):
    """Raised when the AIRBOT vendor bindings are unavailable."""


@dataclass(slots=True)
class ProbeDevice:
    device_id: str
    interface: str
    product_variant: str
    driver: str = "airbot-play"


class AirbotBackend(Protocol):
    def read_state(self) -> JointStateSnapshot: ...

    def send_joint_targets(self, joint_targets: list[float]) -> None: ...

    def send_gravity_compensation(
        self,
        torques: list[float],
        *,
        kp: list[float],
        kd: list[float],
    ) -> None: ...

    def close(self) -> None: ...


def list_can_interfaces() -> list[str]:
    return [name for _index, name in socket.if_nameindex() if name.startswith("can")]


def probe_devices() -> list[ProbeDevice]:
    try:
        _load_vendor_module()
    except BackendUnavailableError:
        return []

    return [
        ProbeDevice(
            device_id=build_probe_id(interface),
            interface=interface,
            product_variant="play-e2",
        )
        for interface in list_can_interfaces()
    ]


def capabilities_for_probe_id(device_id: str) -> dict[str, Any]:
    validate_probe_id(device_id)
    interface = parse_probe_id(device_id)
    if interface not in list_can_interfaces():
        raise RuntimeError(f"unknown AIRBOT interface for id: {device_id}")

    return {
        "id": device_id,
        "driver": "airbot-play",
        "dof": 6,
        "supported_modes": ["free-drive", "command-following"],
        "transport": "can",
        "interface": interface,
        "product_variant": "play-e2",
    }


def validate_probe_id(device_id: str) -> None:
    known_ids = {device.device_id for device in probe_devices()}
    if device_id not in known_ids:
        raise RuntimeError(f"unknown AIRBOT device id: {device_id}")


def build_probe_id(interface: str) -> str:
    return f"airbot-play@{interface}"


def parse_probe_id(device_id: str) -> str:
    prefix = "airbot-play@"
    if not device_id.startswith(prefix):
        raise RuntimeError(f"invalid AIRBOT probe id: {device_id}")
    interface = device_id[len(prefix) :]
    if not interface:
        raise RuntimeError(f"invalid AIRBOT probe id: {device_id}")
    return interface


class VendorAirbotBackend:
    def __init__(self, config: AirbotRuntimeConfig) -> None:
        self._config = config
        self._ah = _load_vendor_module()
        self._executor = self._ah.create_asio_executor(1)
        self._io_context = self._executor.get_io_context()
        self._arm = self._create_arm()
        if not self._arm.init(self._io_context, config.interface, int(config.control_frequency_hz)):
            raise RuntimeError(f"failed to initialize AIRBOT Play on interface {config.interface}")
        self._arm.enable()
        self._set_control_mode(config.mode)

    def read_state(self) -> JointStateSnapshot:
        state = self._arm.state()
        if not getattr(state, "is_valid", False):
            raise RuntimeError("AIRBOT state is invalid")
        return JointStateSnapshot(
            positions=[float(value) for value in state.pos[: self._config.dof]],
            velocities=[float(value) for value in state.vel[: self._config.dof]],
            efforts=[float(value) for value in state.eff[: self._config.dof]],
        )

    def send_joint_targets(self, joint_targets: list[float]) -> None:
        self._set_control_mode("command-following")
        velocities = [0.5] * self._config.dof
        accelerations = [10.0] * self._config.dof
        self._arm.pvt(joint_targets[: self._config.dof], velocities, accelerations)

    def send_gravity_compensation(
        self,
        torques: list[float],
        *,
        kp: list[float],
        kd: list[float],
    ) -> None:
        self._set_control_mode("free-drive")
        zeros = [0.0] * self._config.dof
        self._arm.mit(
            zeros,
            zeros,
            torques[: self._config.dof],
            kp[: self._config.dof],
            kd[: self._config.dof],
        )

    def close(self) -> None:
        with suppress(Exception):
            self._arm.disable()
        with suppress(Exception):
            self._arm.uninit()

    def _create_arm(self) -> Any:
        return self._ah.Play.create(
            self._ah.MotorType.OD,
            self._ah.MotorType.OD,
            self._ah.MotorType.OD,
            self._ah.MotorType.DM,
            self._ah.MotorType.DM,
            self._ah.MotorType.DM,
            self._ah.EEFType.NA,
            self._ah.MotorType.NA,
        )

    def _set_control_mode(self, mode: str) -> None:
        control_mode = (
            self._ah.MotorControlMode.MIT if mode == "free-drive" else self._ah.MotorControlMode.PVT
        )
        self._arm.set_param("arm.control_mode", control_mode)


def _load_vendor_module() -> Any:
    try:
        import airbot_hardware_py as ah
    except Exception as exc:  # pragma: no cover - import outcome depends on host setup
        raise BackendUnavailableError(
            "AIRBOT Python bindings are unavailable; install airbot_hardware_py for hardware access"
        ) from exc

    return ah
