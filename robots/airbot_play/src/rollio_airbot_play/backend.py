from __future__ import annotations

from contextlib import suppress
from dataclasses import dataclass
from typing import Any, Protocol

from .can_transport import (
    is_python_can_available,
    query_airbot_serial,
    scan_can_interfaces,
)
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

    def send_gravity_compensation(self, torques: list[float]) -> None: ...

    def close(self) -> None: ...


def probe_devices() -> list[ProbeDevice]:
    if not is_python_can_available():
        return []

    devices: list[ProbeDevice] = []
    seen_serials: set[str] = set()
    for interface in scan_can_interfaces():
        serial_number = query_airbot_serial(interface, timeout=0.5)
        if serial_number is None or serial_number in seen_serials:
            continue
        devices.append(
            ProbeDevice(
                device_id=build_probe_id(serial_number),
                interface=interface,
                product_variant="play-e2",
            )
        )
        seen_serials.add(serial_number)

    return devices


def capabilities_for_probe_id(device_id: str) -> dict[str, Any]:
    device = require_probe_device(device_id)

    return {
        "id": device.device_id,
        "driver": "airbot-play",
        "dof": 6,
        "supported_modes": ["free-drive", "command-following"],
        "transport": "can",
        "interface": device.interface,
        "product_variant": device.product_variant,
        "serial_number": device.device_id,
    }


def validate_probe_id(device_id: str) -> None:
    require_probe_device(device_id)


def require_probe_device(device_id: str) -> ProbeDevice:
    normalized_device_id = parse_probe_id(device_id)
    devices = probe_devices()
    for device in devices:
        if device.device_id == normalized_device_id:
            return device

    if not devices:
        raise RuntimeError("no AIRBOT devices with readable serial numbers were detected")

    raise RuntimeError(f"unknown AIRBOT device id: {device_id}")


def build_probe_id(serial_number: str) -> str:
    normalized = str(serial_number).strip()
    if not normalized:
        raise RuntimeError("AIRBOT serial number must not be empty")
    return normalized


def parse_probe_id(device_id: str) -> str:
    normalized = str(device_id).strip()
    if not normalized or normalized.startswith("airbot-play@"):
        raise RuntimeError(f"invalid AIRBOT probe id: {device_id}")
    return normalized


class VendorAirbotBackend:
    def __init__(self, config: AirbotRuntimeConfig) -> None:
        self._config = config
        self._ah = _load_vendor_module()
        self._executor = self._ah.create_asio_executor(1)
        self._io_context = self._executor.get_io_context()
        self._arm = self._create_arm()
        self._active_control_mode: str | None = None
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

    def send_gravity_compensation(self, torques: list[float]) -> None:
        self._set_control_mode("free-drive")
        zeros = [0.0] * self._config.dof
        self._arm.mit(
            zeros,
            zeros,
            torques[: self._config.dof],
            zeros,
            zeros,
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
        if mode == self._active_control_mode:
            return
        control_mode = (
            self._ah.MotorControlMode.MIT if mode == "free-drive" else self._ah.MotorControlMode.PVT
        )
        # The vendor bindings can warn or return a falsey status for redundant mode writes,
        # so track the last requested mode and avoid re-sending it every control tick.
        self._arm.set_param("arm.control_mode", control_mode)
        self._active_control_mode = mode


def _load_vendor_module() -> Any:
    try:
        import airbot_hardware_py as ah
    except Exception as exc:  # pragma: no cover - import outcome depends on host setup
        raise BackendUnavailableError(
            "AIRBOT Python bindings are unavailable; install airbot_hardware_py for hardware access"
        ) from exc

    return ah
