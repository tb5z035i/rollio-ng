from __future__ import annotations

from typing import Protocol

from .messages import (
    ControlEvent,
    JointStateSnapshot,
    RobotCommand,
    RobotState,
    build_robot_state_message,
)

CONTROL_EVENTS_SERVICE = "control/events"


def robot_state_service_name(device_name: str) -> str:
    return f"robot/{device_name}/state"


def robot_command_service_name(device_name: str) -> str:
    return f"robot/{device_name}/command"


class RollioIpc(Protocol):
    def poll_control_events(self) -> list[ControlEvent]: ...

    def poll_latest_command(self) -> RobotCommand | None: ...

    def publish_state(
        self,
        *,
        timestamp_ns: int,
        dof: int,
        snapshot: JointStateSnapshot,
    ) -> None: ...

    def close(self) -> None: ...


class Iceoryx2IpcAdapter:
    def __init__(self, device_name: str) -> None:
        try:
            import iceoryx2 as iox2
        except Exception as exc:  # pragma: no cover - depends on optional host install
            raise RuntimeError(
                "iceoryx2 Python bindings are unavailable; install iceoryx2 for runtime IPC"
            ) from exc

        self._iox2 = iox2
        self._node = iox2.NodeBuilder.new().create(iox2.ServiceType.Ipc)

        state_service = (
            self._node.service_builder(iox2.ServiceName.new(robot_state_service_name(device_name)))
            .publish_subscribe(RobotState)
            .open_or_create()
        )
        command_service = (
            self._node.service_builder(
                iox2.ServiceName.new(robot_command_service_name(device_name))
            )
            .publish_subscribe(RobotCommand)
            .open_or_create()
        )
        control_service = (
            self._node.service_builder(iox2.ServiceName.new(CONTROL_EVENTS_SERVICE))
            .publish_subscribe(ControlEvent)
            .open_or_create()
        )

        self._state_publisher = state_service.publisher_builder().create()
        self._command_subscriber = command_service.subscriber_builder().create()
        self._control_subscriber = control_service.subscriber_builder().create()

    def poll_control_events(self) -> list[ControlEvent]:
        events: list[ControlEvent] = []
        while True:
            sample = self._control_subscriber.receive()
            if sample is None:
                break
            events.append(sample.payload().contents)
        return events

    def poll_latest_command(self) -> RobotCommand | None:
        latest: RobotCommand | None = None
        while True:
            sample = self._command_subscriber.receive()
            if sample is None:
                break
            latest = sample.payload().contents
        return latest

    def publish_state(
        self,
        *,
        timestamp_ns: int,
        dof: int,
        snapshot: JointStateSnapshot,
    ) -> None:
        message = build_robot_state_message(
            timestamp_ns=timestamp_ns,
            dof=dof,
            snapshot=snapshot,
        )
        sample = self._state_publisher.loan_uninit()
        sample = sample.write_payload(message)
        sample.send()

    def close(self) -> None:
        return None
