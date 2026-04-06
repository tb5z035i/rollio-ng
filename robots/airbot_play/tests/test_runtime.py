from __future__ import annotations

from pathlib import Path

import pytest

from rollio_airbot_play import backend as backend_module
from rollio_airbot_play.backend import (
    BackendUnavailableError,
    parse_probe_id,
    probe_devices,
)
from rollio_airbot_play.config import AirbotRuntimeConfig, ConfigError, parse_runtime_config
from rollio_airbot_play.messages import (
    CommandMode,
    ControlEvent,
    ControlEventTag,
    JointStateSnapshot,
    RobotCommand,
)
from rollio_airbot_play.runtime import AirbotRuntime


class FakeBackend:
    def __init__(self, snapshot: JointStateSnapshot) -> None:
        self.snapshot = snapshot
        self.sent_targets: list[list[float]] = []
        self.sent_torques: list[tuple[list[float], list[float], list[float]]] = []
        self.closed = False

    def read_state(self) -> JointStateSnapshot:
        return self.snapshot

    def send_joint_targets(self, joint_targets: list[float]) -> None:
        self.sent_targets.append(list(joint_targets))

    def send_gravity_compensation(
        self,
        torques: list[float],
        *,
        kp: list[float],
        kd: list[float],
    ) -> None:
        self.sent_torques.append((list(torques), list(kp), list(kd)))

    def close(self) -> None:
        self.closed = True


class FakeIpc:
    def __init__(self) -> None:
        self.events: list[ControlEvent] = []
        self.command: RobotCommand | None = None
        self.published: list[tuple[int, int, JointStateSnapshot]] = []
        self.closed = False

    def poll_control_events(self) -> list[ControlEvent]:
        events = list(self.events)
        self.events.clear()
        return events

    def poll_latest_command(self) -> RobotCommand | None:
        command = self.command
        self.command = None
        return command

    def publish_state(
        self,
        *,
        timestamp_ns: int,
        dof: int,
        snapshot: JointStateSnapshot,
    ) -> None:
        self.published.append((timestamp_ns, dof, snapshot))

    def close(self) -> None:
        self.closed = True


class FakeModel:
    def inverse_dynamics(
        self,
        q: list[float],
        qd: list[float],
        qdd: list[float],
    ) -> list[float]:
        return [float(idx + 1) for idx in range(len(q))]


def make_config(mode: str = "free-drive", model_path: Path | None = None) -> AirbotRuntimeConfig:
    return AirbotRuntimeConfig(
        name="airbot",
        driver="airbot-play",
        device_id="airbot_0",
        dof=6,
        mode=mode,
        control_frequency_hz=250.0,
        interface="can0",
        product_variant="play-e2",
        end_effector=None,
        model_path=model_path,
        gravity_comp_torque_scales=[0.5] * 6,
        mit_kp=[1.0] * 6,
        mit_kd=[0.1] * 6,
    )


def test_free_drive_step_publishes_state_and_gravity_torques(tmp_path: Path) -> None:
    config = make_config(model_path=tmp_path / "play.urdf")
    backend = FakeBackend(
        JointStateSnapshot(
            positions=[0.0] * 6,
            velocities=[0.0] * 6,
            efforts=[0.0] * 6,
        )
    )
    ipc = FakeIpc()

    runtime = AirbotRuntime(
        config=config,
        backend=backend,
        ipc=ipc,
        gravity_model_loader=lambda _path: FakeModel(),
    )

    result = runtime.step_once()

    assert result.running is True
    assert len(backend.sent_torques) == 1
    assert backend.sent_torques[0][0] == [0.5, 1.0, 1.5, 2.0, 2.5, 3.0]
    assert len(ipc.published) == 1


def test_command_following_step_sends_joint_targets(tmp_path: Path) -> None:
    config = make_config(mode="command-following", model_path=tmp_path / "play.urdf")
    backend = FakeBackend(
        JointStateSnapshot(
            positions=[0.0] * 6,
            velocities=[0.0] * 6,
            efforts=[0.0] * 6,
        )
    )
    ipc = FakeIpc()

    command = RobotCommand()
    command.mode = int(CommandMode.JOINT)
    command.num_joints = 6
    for idx in range(6):
        command.joint_targets[idx] = 1.0
    ipc.command = command

    runtime = AirbotRuntime(
        config=config,
        backend=backend,
        ipc=ipc,
        gravity_model_loader=lambda _path: FakeModel(),
    )

    runtime.step_once()

    assert backend.sent_targets == [[1.0] * 6]
    assert len(ipc.published) == 1


def test_mode_switch_event_updates_runtime_mode(tmp_path: Path) -> None:
    config = make_config(model_path=tmp_path / "play.urdf")
    backend = FakeBackend(
        JointStateSnapshot(
            positions=[0.0] * 6,
            velocities=[0.0] * 6,
            efforts=[0.0] * 6,
        )
    )
    ipc = FakeIpc()

    event = ControlEvent()
    event.tag = int(ControlEventTag.MODE_SWITCH)
    event.payload.target_mode = 1
    ipc.events = [event]

    command = RobotCommand()
    command.mode = int(CommandMode.JOINT)
    command.num_joints = 6
    for idx in range(6):
        command.joint_targets[idx] = 0.25
    ipc.command = command

    runtime = AirbotRuntime(
        config=config,
        backend=backend,
        ipc=ipc,
        gravity_model_loader=lambda _path: FakeModel(),
    )

    result = runtime.step_once()

    assert result.mode == "command-following"
    assert backend.sent_targets == [[0.25] * 6]
    assert backend.sent_torques == []


def test_shutdown_event_stops_runtime(tmp_path: Path) -> None:
    config = make_config(model_path=tmp_path / "play.urdf")
    backend = FakeBackend(
        JointStateSnapshot(
            positions=[0.0] * 6,
            velocities=[0.0] * 6,
            efforts=[0.0] * 6,
        )
    )
    ipc = FakeIpc()

    event = ControlEvent()
    event.tag = int(ControlEventTag.SHUTDOWN)
    ipc.events = [event]

    runtime = AirbotRuntime(
        config=config,
        backend=backend,
        ipc=ipc,
        gravity_model_loader=lambda _path: FakeModel(),
    )

    result = runtime.step_once()

    assert result.running is False
    assert ipc.published == []
    assert backend.sent_torques == []


def test_parse_runtime_config_requires_model_path_for_free_drive() -> None:
    with pytest.raises(ConfigError):
        parse_runtime_config(
            {
                "name": "airbot",
                "type": "robot",
                "driver": "airbot-play",
                "id": "airbot_0",
                "dof": 6,
                "mode": "free-drive",
                "interface": "can0",
                "product_variant": "play-e2",
            }
        )


def test_invalid_probe_id_is_rejected() -> None:
    with pytest.raises(RuntimeError):
        parse_probe_id("invalid")


def test_probe_devices_is_empty_when_vendor_bindings_are_missing(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setattr(
        backend_module,
        "_load_vendor_module",
        lambda: (_ for _ in ()).throw(BackendUnavailableError("missing")),
    )
    monkeypatch.setattr(backend_module, "list_can_interfaces", lambda: ["can0"])
    assert probe_devices() == []
