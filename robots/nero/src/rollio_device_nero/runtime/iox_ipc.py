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
    STATE_BUFFER,
    STATE_END_EFFECTOR_POSE,
    STATE_JOINT_EFFORT,
    STATE_JOINT_POSITION,
    STATE_JOINT_VELOCITY,
    STATE_MAX_NODES,
    STATE_MAX_PUBLISHERS,
    STATE_MAX_SUBSCRIBERS,
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
    MitCommandElement,
    ParallelMitCommand2,
    ParallelVector2,
    Pose7,
    SampleHeader,
)


# Helper: every state/command service must request the same caps as the Rust
# producers/consumers. Wrapping the call here keeps the per-service builders
# below short and ensures we don't drift apart from the Rust side over time.
def _open_state_or_command_pubsub(
    node,
    service_name,
    payload_type,
    *,
    user_header_type: type | None = None,
    initial_max_slice_len: int | None = None,
):
    return open_or_create_pubsub(
        node,
        service_name,
        payload_type,
        user_header_type=user_header_type,
        initial_max_slice_len=initial_max_slice_len,
        max_publishers=STATE_MAX_PUBLISHERS,
        max_subscribers=STATE_MAX_SUBSCRIBERS,
        max_nodes=STATE_MAX_NODES,
        subscriber_max_buffer_size=STATE_BUFFER,
        history_size=STATE_BUFFER,
    )


def _slice_type(element_type: type) -> type:
    try:
        import iceoryx2 as iox2
    except Exception as exc:  # pragma: no cover - depends on host install
        raise RuntimeError("iceoryx2 Python bindings are unavailable") from exc
    return iox2.Slice[element_type]


def _open_f64_vector_pubsub(node, service_name, *, initial_max_slice_len: int | None = None):
    return _open_state_or_command_pubsub(
        node,
        service_name,
        _slice_type(ctypes.c_double),
        user_header_type=SampleHeader,
        initial_max_slice_len=initial_max_slice_len,
    )


def _open_mit_vector_pubsub(node, service_name, *, initial_max_slice_len: int | None = None):
    return _open_state_or_command_pubsub(
        node,
        service_name,
        _slice_type(MitCommandElement),
        user_header_type=SampleHeader,
        initial_max_slice_len=initial_max_slice_len,
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


def _set_sample_header(sample: Any, timestamp_us: int) -> None:
    header = SampleHeader.of(timestamp_us)
    target = sample.user_header_mut()
    if hasattr(target, "contents"):
        ctypes.memmove(ctypes.byref(target.contents), ctypes.byref(header), ctypes.sizeof(header))
    else:
        target.timestamp_us = header.timestamp_us


def _sample_timestamp(sample: Any) -> int:
    header = sample.user_header()
    if hasattr(header, "contents"):
        header = header.contents
    return int(header.timestamp_us)


def _payload_values(sample: Any) -> list[Any]:
    payload = sample.payload()
    return [payload[i] for i in range(len(payload))]


def _send_f64_vector(publisher: Any, timestamp_us: int, values: list[float]) -> None:
    sample = publisher.loan_slice_uninit(len(values))
    _set_sample_header(sample, timestamp_us)
    array_type = ctypes.c_double * len(values)
    sample = sample.write_from_slice(array_type(*[float(value) for value in values]))
    sample.send()


def _send_mit_vector(publisher: Any, timestamp_us: int, elements: list[MitCommandElement]) -> None:
    sample = publisher.loan_slice_uninit(len(elements))
    _set_sample_header(sample, timestamp_us)
    array_type = MitCommandElement * len(elements)
    sample = sample.write_from_slice(array_type(*elements))
    sample.send()


def _drain_latest_f64_vector(subscriber: Any, factory: type, max_len: int) -> Any | None:
    latest = None
    while True:
        sample = subscriber.receive()
        if sample is None:
            return latest
        values = [float(value) for value in _payload_values(sample)]
        if len(values) > max_len:
            continue
        latest = factory.from_values(_sample_timestamp(sample), values)


def _drain_latest_mit_vector(subscriber: Any, factory: type, max_len: int) -> Any | None:
    latest = None
    while True:
        sample = subscriber.receive()
        if sample is None:
            return latest
        elements = _payload_values(sample)
        if len(elements) > max_len:
            continue
        msg = factory()
        msg.timestamp_us = _sample_timestamp(sample)
        msg.len = len(elements)
        for index, element in enumerate(elements):
            msg.position[index] = float(element.position)
            msg.velocity[index] = float(element.velocity)
            msg.effort[index] = float(element.effort)
            msg.kp[index] = float(element.kp)
            msg.kd[index] = float(element.kd)
        latest = msg


def _mit_elements_from_command(msg: JointMitCommand15 | ParallelMitCommand2) -> list[MitCommandElement]:
    out: list[MitCommandElement] = []
    for index in range(int(msg.len)):
        element = MitCommandElement()
        element.position = float(msg.position[index])
        element.velocity = float(msg.velocity[index])
        element.effort = float(msg.effort[index])
        element.kp = float(msg.kp[index])
        element.kd = float(msg.kd[index])
        out.append(element)
    return out


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
            _open_f64_vector_pubsub(
                self._node,
                channel_command_service_name(bus_root, channel_type, COMMAND_JOINT_POSITION),
            )
        )
        self._joint_mit_cmd_sub = make_subscriber(
            _open_mit_vector_pubsub(
                self._node,
                channel_command_service_name(bus_root, channel_type, COMMAND_JOINT_MIT),
            )
        )
        self._end_pose_cmd_sub = make_subscriber(
            _open_f64_vector_pubsub(
                self._node,
                channel_command_service_name(bus_root, channel_type, COMMAND_END_POSE),
            )
        )

        self._joint_position_state_pub = make_publisher(
            _open_f64_vector_pubsub(
                self._node,
                channel_state_service_name(bus_root, channel_type, STATE_JOINT_POSITION),
                initial_max_slice_len=15,
            )
        )
        self._joint_velocity_state_pub = make_publisher(
            _open_f64_vector_pubsub(
                self._node,
                channel_state_service_name(bus_root, channel_type, STATE_JOINT_VELOCITY),
                initial_max_slice_len=15,
            )
        )
        self._joint_effort_state_pub = make_publisher(
            _open_f64_vector_pubsub(
                self._node,
                channel_state_service_name(bus_root, channel_type, STATE_JOINT_EFFORT),
                initial_max_slice_len=15,
            )
        )
        self._end_pose_state_pub = make_publisher(
            _open_f64_vector_pubsub(
                self._node,
                channel_state_service_name(bus_root, channel_type, STATE_END_EFFECTOR_POSE),
                initial_max_slice_len=7,
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
        return _drain_latest_f64_vector(self._joint_position_cmd_sub, JointVector15, 15)

    def poll_joint_mit_command(self) -> JointMitCommand15 | None:
        return _drain_latest_mit_vector(self._joint_mit_cmd_sub, JointMitCommand15, 15)

    def poll_end_pose_command(self) -> Pose7 | None:
        return _drain_latest_f64_vector(self._end_pose_cmd_sub, Pose7, 7)

    def publish_joint_position(self, msg: JointVector15) -> None:
        _send_f64_vector(
            self._joint_position_state_pub,
            int(msg.timestamp_us),
            [float(msg.values[i]) for i in range(int(msg.len))],
        )

    def publish_joint_velocity(self, msg: JointVector15) -> None:
        _send_f64_vector(
            self._joint_velocity_state_pub,
            int(msg.timestamp_us),
            [float(msg.values[i]) for i in range(int(msg.len))],
        )

    def publish_joint_effort(self, msg: JointVector15) -> None:
        _send_f64_vector(
            self._joint_effort_state_pub,
            int(msg.timestamp_us),
            [float(msg.values[i]) for i in range(int(msg.len))],
        )

    def publish_end_effector_pose(self, msg: Pose7) -> None:
        _send_f64_vector(
            self._end_pose_state_pub,
            int(msg.timestamp_us),
            [float(msg.values[i]) for i in range(7)],
        )

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
            _open_f64_vector_pubsub(
                self._node,
                channel_command_service_name(bus_root, channel_type, COMMAND_PARALLEL_POSITION),
            )
        )
        self._parallel_mit_cmd_sub = make_subscriber(
            _open_mit_vector_pubsub(
                self._node,
                channel_command_service_name(bus_root, channel_type, COMMAND_PARALLEL_MIT),
            )
        )

        self._parallel_position_state_pub = make_publisher(
            _open_f64_vector_pubsub(
                self._node,
                channel_state_service_name(bus_root, channel_type, STATE_PARALLEL_POSITION),
                initial_max_slice_len=2,
            )
        )
        self._parallel_velocity_state_pub = make_publisher(
            _open_f64_vector_pubsub(
                self._node,
                channel_state_service_name(bus_root, channel_type, STATE_PARALLEL_VELOCITY),
                initial_max_slice_len=2,
            )
        )
        self._parallel_effort_state_pub = make_publisher(
            _open_f64_vector_pubsub(
                self._node,
                channel_state_service_name(bus_root, channel_type, STATE_PARALLEL_EFFORT),
                initial_max_slice_len=2,
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
        return _drain_latest_f64_vector(self._parallel_position_cmd_sub, ParallelVector2, 2)

    def poll_parallel_mit_command(self) -> ParallelMitCommand2 | None:
        return _drain_latest_mit_vector(self._parallel_mit_cmd_sub, ParallelMitCommand2, 2)

    def publish_parallel_position(self, msg: ParallelVector2) -> None:
        _send_f64_vector(
            self._parallel_position_state_pub,
            int(msg.timestamp_us),
            [float(msg.values[i]) for i in range(int(msg.len))],
        )

    def publish_parallel_velocity(self, msg: ParallelVector2) -> None:
        _send_f64_vector(
            self._parallel_velocity_state_pub,
            int(msg.timestamp_us),
            [float(msg.values[i]) for i in range(int(msg.len))],
        )

    def publish_parallel_effort(self, msg: ParallelVector2) -> None:
        _send_f64_vector(
            self._parallel_effort_state_pub,
            int(msg.timestamp_us),
            [float(msg.values[i]) for i in range(int(msg.len))],
        )

    def shutdown_requested(self) -> bool:
        while True:
            sample = self._control_events_sub.receive()
            if sample is None:
                return False
            event = sample.payload().contents
            if int(event.tag) == CONTROL_EVENT_SHUTDOWN:
                return True


__all__ = ["ArmIox", "GripperIox"]
