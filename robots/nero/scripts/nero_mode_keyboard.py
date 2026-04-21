#!/usr/bin/env -S uv run --script
# /// script
# dependencies = [
#   "maturin>=1.8",
# ]
# ///

"""Keyboard test UI for a running `rollio-device-nero` process.

Talks directly to the AGX Nero channel-local iceoryx2 services:

  - `{bus_root}/{channel}/control/mode`
  - `{bus_root}/{channel}/info/mode`
  - `{bus_root}/{channel}/states/*`

Default channel set is `arm` + `gripper`, matching the two channels the
device opens for the AGX Nero arm + AGX gripper combo.

Two ways to run:

  # Fast path: reuse the device project's already-built iceoryx2 wheel.
  cd robots/nero && uv run python scripts/nero_mode_keyboard.py --bus-root agx_nero

  # Standalone path: PEP 723 ephemeral env. The first run builds iceoryx2
  # via `maturin develop` (cached in `~/.cache/uv/environments-v2/`).
  uv run robots/nero/scripts/nero_mode_keyboard.py --bus-root agx_nero

  uv run robots/nero/scripts/nero_mode_keyboard.py \\
      --bus-root agx_nero \\
      --channel arm \\
      --channel gripper

Controls:

  q            Quit
  j / k        Select next / previous channel
  space        Cycle mode on selected channel
  f            Set selected channel to free-drive
  c            Set selected channel to command-following
  i            Set selected channel to identifying
  d            Set selected channel to disabled
  1-9          Select channel by index
"""

from __future__ import annotations

import argparse
import ctypes
import importlib
import select
import subprocess
import sys
import termios
import time
import tty
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, ClassVar

REPO_ROOT = Path(__file__).resolve().parents[3]
ICEORYX2_PYTHON_DIR = REPO_ROOT / "third_party" / "iceoryx2" / "iceoryx2-ffi" / "python"


def load_iceoryx2() -> Any:
    """Import iceoryx2 or build the local maturin wheel into the script's env.

    The `--script` ephemeral env from PEP 723 starts with just `maturin`;
    if iceoryx2 isn't already importable (i.e. someone hasn't `uv sync`-ed
    `robots/nero/` yet), we run `maturin develop` against the bundled
    submodule before re-importing.
    """
    try:
        return importlib.import_module("iceoryx2")
    except ModuleNotFoundError:
        if not (ICEORYX2_PYTHON_DIR / "Cargo.toml").is_file():
            raise SystemExit(
                f"iceoryx2 source not found at {ICEORYX2_PYTHON_DIR}. "
                "Initialise the third_party/iceoryx2 submodule first: "
                "`git submodule update --init third_party/iceoryx2`."
            ) from None
        subprocess.run(
            [
                sys.executable,
                "-m",
                "maturin",
                "develop",
                "--manifest-path",
                str(ICEORYX2_PYTHON_DIR / "Cargo.toml"),
            ],
            cwd=ICEORYX2_PYTHON_DIR,
            check=True,
        )
        importlib.invalidate_caches()
        return importlib.import_module("iceoryx2")


# ---------------------------------------------------------------------------
# ctypes mirrors of `rollio-types::messages` (must match
# `rollio_device_nero.ipc.types`). Repeated here so the script stays
# self-contained -- it must not import the device package because it
# typically runs from a different uv environment than the device.
# ---------------------------------------------------------------------------


class DeviceChannelMode(ctypes.c_int):
    DISABLED = 0
    ENABLED = 1
    FREE_DRIVE = 2
    COMMAND_FOLLOWING = 3
    IDENTIFYING = 4

    @staticmethod
    def type_name() -> str:
        return "DeviceChannelMode"


class JointVector15(ctypes.Structure):
    _fields_: ClassVar = [
        ("timestamp_us", ctypes.c_uint64),
        ("len", ctypes.c_uint32),
        ("values", ctypes.c_double * 15),
    ]

    @staticmethod
    def type_name() -> str:
        return "JointVector15"


class ParallelVector2(ctypes.Structure):
    _fields_: ClassVar = [
        ("timestamp_us", ctypes.c_uint64),
        ("len", ctypes.c_uint32),
        ("values", ctypes.c_double * 2),
    ]

    @staticmethod
    def type_name() -> str:
        return "ParallelVector2"


class Pose7(ctypes.Structure):
    _fields_: ClassVar = [
        ("timestamp_us", ctypes.c_uint64),
        ("values", ctypes.c_double * 7),
    ]

    @staticmethod
    def type_name() -> str:
        return "Pose7"


MODE_ORDER = [
    DeviceChannelMode.FREE_DRIVE,
    DeviceChannelMode.COMMAND_FOLLOWING,
    DeviceChannelMode.IDENTIFYING,
    DeviceChannelMode.DISABLED,
]


def mode_label(value: int | None) -> str:
    return {
        DeviceChannelMode.DISABLED: "disabled",
        DeviceChannelMode.ENABLED: "enabled",
        DeviceChannelMode.FREE_DRIVE: "free-drive",
        DeviceChannelMode.COMMAND_FOLLOWING: "command-following",
        DeviceChannelMode.IDENTIFYING: "identifying",
        None: "unknown",
    }.get(value, f"unknown({value})")


# ---------------------------------------------------------------------------
# Topic name helpers (mirror of `rollio_device_nero.ipc.services`)
# ---------------------------------------------------------------------------


def channel_mode_control_service_name(bus_root: str, channel_type: str) -> str:
    return f"{bus_root}/{channel_type}/control/mode"


def channel_mode_info_service_name(bus_root: str, channel_type: str) -> str:
    return f"{bus_root}/{channel_type}/info/mode"


def channel_state_service_name(bus_root: str, channel_type: str, state_kind: str) -> str:
    return f"{bus_root}/{channel_type}/states/{state_kind}"


# `gripper` mirrors the airbot G2 contract on the wire (parallel_*); `arm`
# mirrors the airbot arm contract. Anything else follows the gripper layout
# so a future second EEF type still renders.
ARM_STATE_KINDS = (
    ("joint_position", JointVector15),
    ("joint_velocity", JointVector15),
    ("joint_effort", JointVector15),
    ("end_effector_pose", Pose7),
)

GRIPPER_STATE_KINDS = (
    ("parallel_position", ParallelVector2),
    ("parallel_velocity", ParallelVector2),
    ("parallel_effort", ParallelVector2),
)


@dataclass
class StateSnapshot:
    updated_at: float = 0.0
    lines: list[str] = field(default_factory=list)


@dataclass
class ChannelView:
    channel_type: str
    kind: str
    mode_publisher: Any
    mode_subscriber: Any
    state_subscribers: dict[str, Any]
    reported_mode: int | None = None
    last_sent_mode: int | None = None
    status_message: str = ""
    states: dict[str, StateSnapshot] = field(default_factory=dict)


class RawTerminal:
    def __enter__(self) -> RawTerminal:
        if not sys.stdin.isatty():
            raise SystemExit("This script requires an interactive TTY.")
        self._fd = sys.stdin.fileno()
        self._old = termios.tcgetattr(self._fd)
        tty.setcbreak(self._fd)
        sys.stdout.write("\x1b[?25l")
        sys.stdout.flush()
        return self

    def __exit__(self, exc_type, exc, tb) -> None:
        termios.tcsetattr(self._fd, termios.TCSADRAIN, self._old)
        sys.stdout.write("\x1b[?25h\x1b[0m\n")
        sys.stdout.flush()


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="AGX Nero mode keyboard test UI")
    parser.add_argument(
        "--bus-root",
        required=True,
        help="Device bus_root to inspect/control (matches the device's run config).",
    )
    parser.add_argument(
        "--channel",
        action="append",
        dest="channels",
        default=[],
        help="Channel type to monitor/control. Repeat for multiple channels. Default: arm gripper",
    )
    parser.add_argument(
        "--refresh-ms",
        type=int,
        default=100,
        help="UI refresh/poll interval in milliseconds.",
    )
    return parser.parse_args()


def open_channel_view(iox2: Any, node: Any, bus_root: str, channel_type: str) -> ChannelView:
    mode_service = (
        node.service_builder(
            iox2.ServiceName.new(channel_mode_control_service_name(bus_root, channel_type))
        )
        .publish_subscribe(DeviceChannelMode)
        .open_or_create()
    )
    info_service = (
        node.service_builder(
            iox2.ServiceName.new(channel_mode_info_service_name(bus_root, channel_type))
        )
        .publish_subscribe(DeviceChannelMode)
        .open_or_create()
    )
    state_kinds = ARM_STATE_KINDS if channel_type == "arm" else GRIPPER_STATE_KINDS
    state_subscribers: dict[str, Any] = {}
    for state_kind, payload_type in state_kinds:
        service = (
            node.service_builder(
                iox2.ServiceName.new(channel_state_service_name(bus_root, channel_type, state_kind))
            )
            .publish_subscribe(payload_type)
            .open_or_create()
        )
        state_subscribers[state_kind] = service.subscriber_builder().create()
    return ChannelView(
        channel_type=channel_type,
        kind="arm" if channel_type == "arm" else "eef",
        mode_publisher=mode_service.publisher_builder().create(),
        mode_subscriber=info_service.subscriber_builder().create(),
        state_subscribers=state_subscribers,
        states={name: StateSnapshot() for name, _ in state_kinds},
    )


def read_key() -> str | None:
    ready, _, _ = select.select([sys.stdin], [], [], 0.0)
    if not ready:
        return None
    return sys.stdin.read(1)


def send_mode(channel: ChannelView, mode_value: int) -> None:
    channel.mode_publisher.send_copy(DeviceChannelMode(mode_value))
    channel.last_sent_mode = mode_value
    channel.status_message = f"sent {mode_label(mode_value)}"


def cycle_mode(current: int | None) -> int:
    if current not in MODE_ORDER:
        return MODE_ORDER[0]
    index = MODE_ORDER.index(current)
    return MODE_ORDER[(index + 1) % len(MODE_ORDER)]


def drain_mode_updates(channel: ChannelView) -> None:
    while True:
        sample = channel.mode_subscriber.receive()
        if sample is None:
            return
        payload = sample.payload().contents
        channel.reported_mode = int(payload.value)
        channel.status_message = f"reported {mode_label(channel.reported_mode)}"


def format_vector(values: list[float]) -> str:
    return "[" + ", ".join(f"{value:+.4f}" for value in values) + "]"


def drain_state_updates(channel: ChannelView) -> None:
    for state_kind, subscriber in channel.state_subscribers.items():
        while True:
            sample = subscriber.receive()
            if sample is None:
                break
            payload = sample.payload().contents
            if isinstance(payload, JointVector15):
                active = min(int(payload.len), 15)
                values = [payload.values[i] for i in range(active)]
                lines = [
                    f"t={payload.timestamp_us}us",
                    format_vector(values),
                ]
            elif isinstance(payload, ParallelVector2):
                active = min(int(payload.len), 2)
                values = [payload.values[i] for i in range(active)]
                lines = [
                    f"t={payload.timestamp_us}us",
                    format_vector(values),
                ]
            else:
                values = [payload.values[i] for i in range(7)]
                lines = [
                    f"t={payload.timestamp_us}us",
                    format_vector(values),
                ]
            channel.states[state_kind] = StateSnapshot(
                updated_at=time.monotonic(),
                lines=lines,
            )


def render(bus_root: str, channels: list[ChannelView], selected_index: int) -> None:
    sys.stdout.write("\x1b[2J\x1b[H")
    print("AGX Nero Channel Keyboard Test")
    print(f"bus_root: {bus_root}")
    print(
        "keys: j/k select, space cycle, f free-drive, c command-following, "
        "i identifying, d disabled, q quit"
    )
    print()
    for index, channel in enumerate(channels, start=1):
        marker = ">" if selected_index == index - 1 else " "
        print(
            f"{marker} {index}. {channel.channel_type:<8} "
            f"kind={channel.kind:<3} "
            f"mode={mode_label(channel.reported_mode):<18} "
            f"last-sent={mode_label(channel.last_sent_mode):<18} "
            f"status={channel.status_message}"
        )
        for state_kind, snapshot in channel.states.items():
            age = (
                "never"
                if snapshot.updated_at == 0.0
                else f"{time.monotonic() - snapshot.updated_at:.2f}s ago"
            )
            print(f"    {state_kind:<18} ({age})")
            if snapshot.lines:
                for line in snapshot.lines:
                    print(f"      {line}")
            else:
                print("      (no samples)")
        print()
    sys.stdout.flush()


def main() -> int:
    args = parse_args()
    iox2 = load_iceoryx2()
    channels = args.channels or ["arm", "gripper"]
    iox2.set_log_level_from_env_or(iox2.LogLevel.Warn)
    node = iox2.NodeBuilder.new().create(iox2.ServiceType.Ipc)
    channel_views = [open_channel_view(iox2, node, args.bus_root, channel) for channel in channels]
    selected_index = 0
    refresh = iox2.Duration.from_millis(max(args.refresh_ms, 10))

    with RawTerminal():
        while True:
            key = read_key()
            if key == "q":
                return 0
            if key == "j":
                selected_index = (selected_index + 1) % len(channel_views)
            elif key == "k":
                selected_index = (selected_index - 1) % len(channel_views)
            elif key and key.isdigit():
                idx = int(key) - 1
                if 0 <= idx < len(channel_views):
                    selected_index = idx
            elif key == " ":
                channel = channel_views[selected_index]
                send_mode(channel, cycle_mode(channel.reported_mode or channel.last_sent_mode))
            elif key == "f":
                send_mode(channel_views[selected_index], DeviceChannelMode.FREE_DRIVE)
            elif key == "c":
                send_mode(channel_views[selected_index], DeviceChannelMode.COMMAND_FOLLOWING)
            elif key == "i":
                send_mode(channel_views[selected_index], DeviceChannelMode.IDENTIFYING)
            elif key == "d":
                send_mode(channel_views[selected_index], DeviceChannelMode.DISABLED)

            for channel in channel_views:
                drain_mode_updates(channel)
                drain_state_updates(channel)

            render(args.bus_root, channel_views, selected_index)

            try:
                node.wait(refresh)
            except iox2.NodeWaitFailure:
                return 0


if __name__ == "__main__":
    raise SystemExit(main())
