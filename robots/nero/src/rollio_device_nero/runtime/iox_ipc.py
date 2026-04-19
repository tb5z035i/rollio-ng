"""iceoryx2 implementations of `ArmIpc` and `GripperIpc`.

Both classes own a per-thread `iceoryx2.Node` (services are only safe to
use from the thread that created them). They open exactly the services
the per-channel runtime touches; everything else stays on the controller
side. Topic paths are built from `ipc/services.py` so the device shares
naming with the Rust controller.
"""

from __future__ import annotations

import ctypes
from typing import Any

from ..ipc.services import (
    COMMAND_END_POSE,
    COMMAND_JOINT_MIT,
    COMMAND_JOINT_POSITION,
    COMMAND_PARALLEL_MIT,
    COMMAND_PARALLEL_POSITION,
    CONTROL_EVENTS_SERVICE,
    STATE_END_EFFECTOR_POSE,
    STATE_JOINT_EFFORT,
    STATE_JOINT_POSITION,
    STATE_JOINT_VELOCITY,
    STATE_PARALLEL_EFFORT,
    STATE_PARALLEL_POSITION,
    STATE_PARALLEL_VELOCITY,
    channel_command_service_name,
    channel_mode_control_service_name,
    channel_mode_info_service_name,
    channel_state_service_name,
    create_node,
    drain_latest,
    make_publisher,
    make_subscriber,
    open_or_create_pubsub,
)
from ..ipc.types import (
    CONTROL_EVENT_SHUTDOWN,
    ControlEvent,
    DeviceChannelMode,
    JointMitCommand15,
    JointVector15,
    ParallelMitCommand2,
    ParallelVector2,
    Pose7,
)


def _send(publisher: Any, payload: ctypes.Structure) -> None:
    """Loan a sample, copy `payload` into it and send.

    Equivalent to `Publisher.send_copy(payload)` from the iceoryx2
    extensions, but spelled out so the runtime can raise a clean
    `RuntimeError` on the rare iceoryx2 failure modes.
    """
    sample = publisher.loan_uninit()
    sample = sample.write_payload(payload)
    sample.send()


# ---------------------------------------------------------------------------
# Arm IPC
# ---------------------------------------------------------------------------


class ArmIox:
    """`ArmIpc` impl over iceoryx2."""

    def __init__(self, *, bus_root: str, channel_type: str, node_name: str | None = None) -> None:
        self._node = create_node(node_name)
        self._bus_root = bus_root
        self._channel_type = channel_type

        self._mode_in_pub = make_publisher(
            open_or_create_pubsub(
                self._node,
                channel_mode_control_service_name(bus_root, channel_type),
                DeviceChannelMode,
                max_publishers=16,
                max_subscribers=16,
                max_nodes=16,
            )
        )
        self._mode_in_sub = make_subscriber(
            open_or_create_pubsub(
                self._node,
                channel_mode_control_service_name(bus_root, channel_type),
                DeviceChannelMode,
                max_publishers=16,
                max_subscribers=16,
                max_nodes=16,
            )
        )
        self._mode_out_pub = make_publisher(
            open_or_create_pubsub(
                self._node,
                channel_mode_info_service_name(bus_root, channel_type),
                DeviceChannelMode,
                max_publishers=16,
                max_subscribers=16,
                max_nodes=16,
            )
        )

        self._joint_position_cmd_sub = make_subscriber(
            open_or_create_pubsub(
                self._node,
                channel_command_service_name(bus_root, channel_type, COMMAND_JOINT_POSITION),
                JointVector15,
            )
        )
        self._joint_mit_cmd_sub = make_subscriber(
            open_or_create_pubsub(
                self._node,
                channel_command_service_name(bus_root, channel_type, COMMAND_JOINT_MIT),
                JointMitCommand15,
            )
        )
        self._end_pose_cmd_sub = make_subscriber(
            open_or_create_pubsub(
                self._node,
                channel_command_service_name(bus_root, channel_type, COMMAND_END_POSE),
                Pose7,
            )
        )

        self._joint_position_state_pub = make_publisher(
            open_or_create_pubsub(
                self._node,
                channel_state_service_name(bus_root, channel_type, STATE_JOINT_POSITION),
                JointVector15,
            )
        )
        self._joint_velocity_state_pub = make_publisher(
            open_or_create_pubsub(
                self._node,
                channel_state_service_name(bus_root, channel_type, STATE_JOINT_VELOCITY),
                JointVector15,
            )
        )
        self._joint_effort_state_pub = make_publisher(
            open_or_create_pubsub(
                self._node,
                channel_state_service_name(bus_root, channel_type, STATE_JOINT_EFFORT),
                JointVector15,
            )
        )
        self._end_pose_state_pub = make_publisher(
            open_or_create_pubsub(
                self._node,
                channel_state_service_name(bus_root, channel_type, STATE_END_EFFECTOR_POSE),
                Pose7,
            )
        )

        self._control_events_sub = make_subscriber(
            open_or_create_pubsub(self._node, CONTROL_EVENTS_SERVICE, ControlEvent)
        )

        # Suppress mode-control messages we publish ourselves: the runtime
        # writes its own outgoing mode publisher (in case the controller
        # wants to observe what the device is doing) but should not see its
        # own writes echoed back through the input subscriber.
        self._self_mode_pub_used = False

    # ----- ArmIpc -----

    def poll_mode_change(self) -> int | None:
        latest = drain_latest(self._mode_in_sub)
        if latest is None:
            return None
        return int(latest.value)

    def publish_mode(self, mode_value: int) -> None:
        _send(self._mode_out_pub, DeviceChannelMode.of(mode_value))

    def poll_joint_position_command(self) -> JointVector15 | None:
        return drain_latest(self._joint_position_cmd_sub)

    def poll_joint_mit_command(self) -> JointMitCommand15 | None:
        return drain_latest(self._joint_mit_cmd_sub)

    def poll_end_pose_command(self) -> Pose7 | None:
        return drain_latest(self._end_pose_cmd_sub)

    def publish_joint_position(self, msg: JointVector15) -> None:
        _send(self._joint_position_state_pub, msg)

    def publish_joint_velocity(self, msg: JointVector15) -> None:
        _send(self._joint_velocity_state_pub, msg)

    def publish_joint_effort(self, msg: JointVector15) -> None:
        _send(self._joint_effort_state_pub, msg)

    def publish_end_effector_pose(self, msg: Pose7) -> None:
        _send(self._end_pose_state_pub, msg)

    def shutdown_requested(self) -> bool:
        # Reading the control_events subscriber is a non-blocking pop; it
        # may yield several samples in one tick if the controller burst-sent
        # events. We only care about the Shutdown discriminant.
        while True:
            sample = self._control_events_sub.receive()
            if sample is None:
                return False
            event = sample.payload().contents
            if int(event.tag) == CONTROL_EVENT_SHUTDOWN:
                return True


# ---------------------------------------------------------------------------
# Gripper IPC
# ---------------------------------------------------------------------------


class GripperIox:
    """`GripperIpc` impl over iceoryx2."""

    def __init__(self, *, bus_root: str, channel_type: str, node_name: str | None = None) -> None:
        self._node = create_node(node_name)

        self._mode_in_sub = make_subscriber(
            open_or_create_pubsub(
                self._node,
                channel_mode_control_service_name(bus_root, channel_type),
                DeviceChannelMode,
                max_publishers=16,
                max_subscribers=16,
                max_nodes=16,
            )
        )
        self._mode_out_pub = make_publisher(
            open_or_create_pubsub(
                self._node,
                channel_mode_info_service_name(bus_root, channel_type),
                DeviceChannelMode,
                max_publishers=16,
                max_subscribers=16,
                max_nodes=16,
            )
        )

        self._parallel_position_cmd_sub = make_subscriber(
            open_or_create_pubsub(
                self._node,
                channel_command_service_name(bus_root, channel_type, COMMAND_PARALLEL_POSITION),
                ParallelVector2,
            )
        )
        self._parallel_mit_cmd_sub = make_subscriber(
            open_or_create_pubsub(
                self._node,
                channel_command_service_name(bus_root, channel_type, COMMAND_PARALLEL_MIT),
                ParallelMitCommand2,
            )
        )

        self._parallel_position_state_pub = make_publisher(
            open_or_create_pubsub(
                self._node,
                channel_state_service_name(bus_root, channel_type, STATE_PARALLEL_POSITION),
                ParallelVector2,
            )
        )
        self._parallel_velocity_state_pub = make_publisher(
            open_or_create_pubsub(
                self._node,
                channel_state_service_name(bus_root, channel_type, STATE_PARALLEL_VELOCITY),
                ParallelVector2,
            )
        )
        self._parallel_effort_state_pub = make_publisher(
            open_or_create_pubsub(
                self._node,
                channel_state_service_name(bus_root, channel_type, STATE_PARALLEL_EFFORT),
                ParallelVector2,
            )
        )

        self._control_events_sub = make_subscriber(
            open_or_create_pubsub(self._node, CONTROL_EVENTS_SERVICE, ControlEvent)
        )

    # ----- GripperIpc -----

    def poll_mode_change(self) -> int | None:
        latest = drain_latest(self._mode_in_sub)
        if latest is None:
            return None
        return int(latest.value)

    def publish_mode(self, mode_value: int) -> None:
        _send(self._mode_out_pub, DeviceChannelMode.of(mode_value))

    def poll_parallel_position_command(self) -> ParallelVector2 | None:
        return drain_latest(self._parallel_position_cmd_sub)

    def poll_parallel_mit_command(self) -> ParallelMitCommand2 | None:
        return drain_latest(self._parallel_mit_cmd_sub)

    def publish_parallel_position(self, msg: ParallelVector2) -> None:
        _send(self._parallel_position_state_pub, msg)

    def publish_parallel_velocity(self, msg: ParallelVector2) -> None:
        _send(self._parallel_velocity_state_pub, msg)

    def publish_parallel_effort(self, msg: ParallelVector2) -> None:
        _send(self._parallel_effort_state_pub, msg)

    def shutdown_requested(self) -> bool:
        while True:
            sample = self._control_events_sub.receive()
            if sample is None:
                return False
            event = sample.payload().contents
            if int(event.tag) == CONTROL_EVENT_SHUTDOWN:
                return True


__all__ = ["ArmIox", "GripperIox"]
