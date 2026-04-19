"""iceoryx2 service helpers built on the rollio-bus topic naming scheme.

Topic name helpers mirror `rollio-bus/src/lib.rs` exactly so that the
Python device shares services with the Rust controller / visualizer / pairing
modules without any controller-side adapter. We import `iceoryx2` lazily so
unit tests that only touch `types` / `config` / topic-name helpers don't
require the iceoryx2 binary wheel.
"""

from __future__ import annotations

from typing import Any

# ---------------------------------------------------------------------------
# rollio-bus topic helpers (mirror of `rollio-bus/src/lib.rs`)
# ---------------------------------------------------------------------------

CONTROL_EVENTS_SERVICE: str = "control/events"


def channel_state_service_name(bus_root: str, channel_type: str, state_kind: str) -> str:
    return f"{bus_root}/{channel_type}/states/{state_kind}"


def channel_command_service_name(bus_root: str, channel_type: str, command_kind: str) -> str:
    return f"{bus_root}/{channel_type}/commands/{command_kind}"


def channel_mode_info_service_name(bus_root: str, channel_type: str) -> str:
    return f"{bus_root}/{channel_type}/info/mode"


def channel_mode_control_service_name(bus_root: str, channel_type: str) -> str:
    return f"{bus_root}/{channel_type}/control/mode"


# State / command topic suffix vocabulary. Mirrors `RobotStateKind::topic_suffix`
# and `RobotCommandKind::topic_suffix` in `rollio-types/src/config.rs` so the
# device speaks the controller's exact vocabulary. Kept as plain string
# constants because the device only ever publishes / subscribes to a fixed,
# small subset.
STATE_JOINT_POSITION: str = "joint_position"
STATE_JOINT_VELOCITY: str = "joint_velocity"
STATE_JOINT_EFFORT: str = "joint_effort"
STATE_END_EFFECTOR_POSE: str = "end_effector_pose"
STATE_PARALLEL_POSITION: str = "parallel_position"
STATE_PARALLEL_VELOCITY: str = "parallel_velocity"
STATE_PARALLEL_EFFORT: str = "parallel_effort"

COMMAND_JOINT_POSITION: str = "joint_position"
COMMAND_JOINT_MIT: str = "joint_mit"
COMMAND_END_POSE: str = "end_pose"
COMMAND_PARALLEL_POSITION: str = "parallel_position"
COMMAND_PARALLEL_MIT: str = "parallel_mit"


# ---------------------------------------------------------------------------
# Lazy iceoryx2 binding
# ---------------------------------------------------------------------------


def _iox2() -> Any:
    """Import iceoryx2 lazily so the module is importable without the wheel."""
    try:
        import iceoryx2 as iox2
    except Exception as exc:  # pragma: no cover - depends on host install
        raise RuntimeError(
            "iceoryx2 Python bindings are unavailable; install iceoryx2 (the "
            "wheel is shipped as a workspace submodule under "
            "third_party/iceoryx2/iceoryx2-ffi/python)."
        ) from exc
    return iox2


def create_node(name: str | None = None) -> Any:
    """Create an iceoryx2 IPC node.

    Signal handling is left to Python's `signal` module: the runtime installs
    its own SIGINT/SIGTERM handlers per `gravity_compensation.py`, so we do
    not let iceoryx2 install its own.
    """
    iox2 = _iox2()
    builder = iox2.NodeBuilder.new()
    if name:
        builder = builder.name(iox2.NodeName.new(name))
    return builder.create(iox2.ServiceType.Ipc)


def open_or_create_pubsub(
    node: Any,
    service_name: str,
    payload_type: type,
    *,
    max_publishers: int | None = None,
    max_subscribers: int | None = None,
    max_nodes: int | None = None,
) -> Any:
    """Open or create a publish_subscribe service with optional fan-in/out limits.

    iceoryx2 enforces that every process opening the same service must agree
    on the publisher/subscriber/node caps. By default we pass nothing here so
    iceoryx2 picks its built-in defaults (max_publishers=2, max_subscribers=8,
    max_nodes=20 in the bundled config) -- matching every other consumer
    (visualizer, teleop router, episode assembler, the airbot driver's
    state/command services) so a single-publisher state/command service can
    be opened by anyone first.

    Mode services need multiple writers (controller, visualizer, CLI scripts
    for keyboard testing), so callers that open mode services explicitly
    pass `max_publishers=16, max_subscribers=16, max_nodes=16` -- the same
    convention the airbot device uses.
    """
    iox2 = _iox2()
    builder = node.service_builder(iox2.ServiceName.new(service_name)).publish_subscribe(
        payload_type
    )
    if max_publishers is not None:
        builder = builder.max_publishers(max_publishers)
    if max_subscribers is not None:
        builder = builder.max_subscribers(max_subscribers)
    if max_nodes is not None:
        builder = builder.max_nodes(max_nodes)
    return builder.open_or_create()


def make_publisher(service: Any) -> Any:
    return service.publisher_builder().create()


def make_subscriber(service: Any) -> Any:
    return service.subscriber_builder().create()


# ---------------------------------------------------------------------------
# Drain helpers
# ---------------------------------------------------------------------------


def drain_latest(subscriber: Any) -> Any | None:
    """Pop every queued sample, returning a *copy* of the last one or None.

    iceoryx2 samples are zero-copy borrowed memory; once the sample handle is
    dropped, the slot can be recycled. The runtime needs to keep the latest
    payload across iteration boundaries (e.g. last commanded joint target),
    so we copy the ctypes contents into a freshly-allocated value before
    returning.
    """
    import ctypes

    last_copy = None
    while True:
        sample = subscriber.receive()
        if sample is None:
            return last_copy
        ptr = sample.payload()
        # ptr is a `ctypes.POINTER(T)`; dereference and copy the value.
        value = ptr.contents
        type_ = type(value)
        copy = type_()
        ctypes.memmove(ctypes.byref(copy), ctypes.byref(value), ctypes.sizeof(type_))
        last_copy = copy


def drain_all(subscriber: Any) -> list[Any]:
    """Pop every queued sample, returning copies in arrival order.

    Used by the shutdown listener: we want to see *any* `Shutdown` event in
    the queue, not just the last one (the controller may send a burst of
    other events before Shutdown).
    """
    import ctypes

    out: list[Any] = []
    while True:
        sample = subscriber.receive()
        if sample is None:
            return out
        value = sample.payload().contents
        type_ = type(value)
        copy = type_()
        ctypes.memmove(ctypes.byref(copy), ctypes.byref(value), ctypes.sizeof(type_))
        out.append(copy)


__all__ = [
    "COMMAND_END_POSE",
    "COMMAND_JOINT_MIT",
    "COMMAND_JOINT_POSITION",
    "COMMAND_PARALLEL_MIT",
    "COMMAND_PARALLEL_POSITION",
    "CONTROL_EVENTS_SERVICE",
    "STATE_END_EFFECTOR_POSE",
    "STATE_JOINT_EFFORT",
    "STATE_JOINT_POSITION",
    "STATE_JOINT_VELOCITY",
    "STATE_PARALLEL_EFFORT",
    "STATE_PARALLEL_POSITION",
    "STATE_PARALLEL_VELOCITY",
    "channel_command_service_name",
    "channel_mode_control_service_name",
    "channel_mode_info_service_name",
    "channel_state_service_name",
    "create_node",
    "drain_all",
    "drain_latest",
    "make_publisher",
    "make_subscriber",
    "open_or_create_pubsub",
]
