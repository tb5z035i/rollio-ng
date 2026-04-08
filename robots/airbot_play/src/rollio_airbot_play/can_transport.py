from __future__ import annotations

import time
from collections.abc import Callable
from pathlib import Path

AIRBOT_BROADCAST_ID = 0x000
AIRBOT_RESPONSE_ID = 0x100
AIRBOT_SERIAL_CMD = 0x04


def is_python_can_available() -> bool:
    try:
        import can  # pylint: disable=import-outside-toplevel,unused-import
    except ImportError:
        return False

    return True


def scan_can_interfaces() -> list[str]:
    interfaces: list[str] = []
    net_path = Path("/sys/class/net")

    if not net_path.exists():
        return interfaces

    for iface_path in net_path.iterdir():
        type_path = iface_path / "type"
        if not type_path.exists():
            continue
        try:
            iface_type = type_path.read_text(encoding="utf-8").strip()
        except OSError:
            continue
        if iface_type == "280":
            interfaces.append(iface_path.name)

    return sorted(interfaces)


def is_can_interface_up(interface: str) -> bool:
    flags_path = Path(f"/sys/class/net/{interface}/flags")
    if not flags_path.exists():
        return False

    try:
        flags = int(flags_path.read_text(encoding="utf-8").strip(), 16)
    except (OSError, ValueError):
        return False

    return bool(flags & 0x1)


class CANBus:
    def __init__(self, interface: str, *, bustype: str = "socketcan") -> None:
        try:
            import can  # pylint: disable=import-outside-toplevel
        except ImportError as exc:
            raise ImportError(
                "python-can is required for AIRBOT CAN probing. Install with: pip install python-can"
            ) from exc

        self._can = can
        self._interface = interface
        self._bustype = bustype
        self._bus = None

    def __enter__(self) -> CANBus:
        self.open()
        return self

    def __exit__(self, exc_type, exc_val, exc_tb) -> None:
        self.close()

    def open(self) -> None:
        if self._bus is not None:
            return

        self._bus = self._can.interface.Bus(
            channel=self._interface,
            interface=self._bustype,
        )

    def close(self) -> None:
        if self._bus is not None:
            self._bus.shutdown()
            self._bus = None

    def send(self, arbitration_id: int, data: bytes | list[int]) -> bool:
        if self._bus is None:
            return False
        if isinstance(data, list):
            data = bytes(data)

        msg = self._can.Message(
            arbitration_id=arbitration_id,
            data=data,
            is_extended_id=False,
        )
        try:
            self._bus.send(msg)
        except self._can.CanError:
            return False

        return True

    def recv(self, *, timeout: float = 1.0) -> tuple[int, bytes] | None:
        if self._bus is None:
            return None

        try:
            msg = self._bus.recv(timeout=timeout)
        except self._can.CanError:
            return None

        if msg is None:
            return None
        return (msg.arbitration_id, bytes(msg.data))

    def recv_all(self, *, timeout: float = 0.5, max_messages: int = 100) -> list[tuple[int, bytes]]:
        messages: list[tuple[int, bytes]] = []
        for _ in range(max_messages):
            message = self.recv(timeout=timeout)
            if message is None:
                break
            messages.append(message)
        return messages


def query_airbot_serial(interface: str, timeout: float = 1.0) -> str | None:
    responses = _collect_airbot_frames(
        interface,
        request_arb_id=AIRBOT_BROADCAST_ID,
        request_payload=bytes([AIRBOT_SERIAL_CMD]),
        timeout=timeout,
        recv_timeout=0.1,
        accept_frame=lambda arb_id, data: (
            arb_id == AIRBOT_RESPONSE_ID and len(data) >= 2 and data[0] == AIRBOT_SERIAL_CMD
        ),
        stop_when=lambda frames: len(frames) >= 4,
    )
    if not responses:
        return None

    ordered_responses = sorted(
        ((data[1], data) for _, data in responses),
        key=lambda item: item[0],
    )
    serial_parts = [
        data[2:].decode("ascii", errors="ignore") for _, data in ordered_responses if len(data) > 2
    ]
    serial = "".join(serial_parts).strip("\x00").strip()
    return serial or None


def _collect_airbot_frames(
    interface: str,
    *,
    request_arb_id: int,
    request_payload: bytes,
    timeout: float,
    recv_timeout: float,
    accept_frame: Callable[[int, bytes], bool],
    stop_when: Callable[[list[tuple[int, bytes]]], bool] | None = None,
) -> list[tuple[int, bytes]] | None:
    if not is_can_interface_up(interface):
        return None

    try:
        with CANBus(interface) as bus:
            bus.recv_all(timeout=0.1, max_messages=50)
            if not bus.send(request_arb_id, request_payload):
                return None

            responses: list[tuple[int, bytes]] = []
            deadline = time.time() + timeout
            while time.time() < deadline:
                frame = bus.recv(timeout=recv_timeout)
                if frame is None:
                    if stop_when is not None and stop_when(responses):
                        break
                    continue

                arb_id, data = frame
                if not accept_frame(arb_id, data):
                    continue

                responses.append(frame)
                if stop_when is not None and stop_when(responses):
                    break

            return responses
    except (ImportError, OSError, RuntimeError, ValueError, TypeError):
        return None


__all__ = [
    "CANBus",
    "is_can_interface_up",
    "is_python_can_available",
    "query_airbot_serial",
    "scan_can_interfaces",
]
