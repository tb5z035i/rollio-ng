from __future__ import annotations

from pathlib import Path

import pytest

from rollio_airbot_play import backend as backend_module
from rollio_airbot_play.backend import (
    ProbeDevice,
    capabilities_for_probe_id,
    parse_probe_id,
    probe_devices,
    require_probe_device,
    validate_probe_id,
)
from rollio_airbot_play.config import (
    AirbotRuntimeConfig,
    ConfigError,
    load_runtime_config,
    parse_runtime_config,
)
from rollio_airbot_play.gravity import GravityModelUnavailableError, load_gravity_model
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
        self.sent_torques: list[list[float]] = []
        self.closed = False

    def read_state(self) -> JointStateSnapshot:
        return self.snapshot

    def send_joint_targets(self, joint_targets: list[float]) -> None:
        self.sent_targets.append(list(joint_targets))

    def send_gravity_compensation(self, torques: list[float]) -> None:
        self.sent_torques.append(list(torques))

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


class FakeVendorArm:
    def __init__(self) -> None:
        self.set_param_calls: list[tuple[str, object]] = []
        self.mit_calls: list[
            tuple[list[float], list[float], list[float], list[float], list[float]]
        ] = []
        self.pvt_calls: list[tuple[list[float], list[float], list[float]]] = []

    def init(self, io_context: object, interface: str, frequency_hz: int) -> bool:
        del io_context, interface, frequency_hz
        return True

    def enable(self) -> None:
        return None

    def state(self) -> object:
        class State:
            is_valid = True
            pos = [0.0] * 6
            vel = [0.0] * 6
            eff = [0.0] * 6

        return State()

    def pvt(
        self, positions: list[float], velocities: list[float], accelerations: list[float]
    ) -> None:
        self.pvt_calls.append((list(positions), list(velocities), list(accelerations)))

    def mit(
        self,
        positions: list[float],
        velocities: list[float],
        efforts: list[float],
        kp: list[float],
        kd: list[float],
    ) -> None:
        self.mit_calls.append(
            (list(positions), list(velocities), list(efforts), list(kp), list(kd))
        )

    def set_param(self, name: str, value: object) -> None:
        self.set_param_calls.append((name, value))

    def disable(self) -> None:
        return None

    def uninit(self) -> None:
        return None


class FakeVendorPlayFactory:
    def __init__(self, arm: FakeVendorArm) -> None:
        self._arm = arm

    def create(self, *args: object) -> FakeVendorArm:
        del args
        return self._arm


class FakeVendorExecutor:
    def get_io_context(self) -> object:
        return object()


class FakeVendorModule:
    class MotorType:
        OD = "od"
        DM = "dm"
        NA = "na"

    class EEFType:
        NA = "na"

    class MotorControlMode:
        MIT = "mit"
        PVT = "pvt"

    def __init__(self) -> None:
        self.arm = FakeVendorArm()
        self.Play = FakeVendorPlayFactory(self.arm)

    def create_asio_executor(self, workers: int) -> FakeVendorExecutor:
        del workers
        return FakeVendorExecutor()


def make_config(mode: str = "free-drive", model_path: Path | None = None) -> AirbotRuntimeConfig:
    return AirbotRuntimeConfig(
        name="airbot",
        driver="airbot-play",
        device_id="SN12345678",
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


def test_local_pinocchio_model_locks_non_arm_joints(monkeypatch: pytest.MonkeyPatch) -> None:
    from rollio_airbot_play import pinocchio_model as pinocchio_model_module

    class FakeFullModel:
        njoints = 9
        names = [
            "universe",
            "joint1",
            "joint2",
            "joint3",
            "joint4",
            "joint5",
            "joint6",
            "e2_joint",
            "e2_left_joint",
        ]

    class FakeReducedModel:
        nq = 6

        def createData(self) -> object:
            return object()

    class FakeFullModelWithData(FakeFullModel):
        nq = 8

        def createData(self) -> object:
            return object()

    class FakePin:
        def __init__(self) -> None:
            self.locked_joint_ids: list[int] | None = None
            self.urdf_path: str | None = None

        def buildModelFromUrdf(self, urdf_path: str) -> FakeFullModelWithData:
            self.urdf_path = urdf_path
            return FakeFullModelWithData()

        def neutral(self, full_model: FakeFullModelWithData) -> str:
            return "neutral"

        def buildReducedModel(
            self,
            full_model: FakeFullModelWithData,
            locked_joint_ids: list[int],
            q_neutral: str,
        ) -> FakeReducedModel:
            self.locked_joint_ids = list(locked_joint_ids)
            assert q_neutral == "neutral"
            return FakeReducedModel()

        def rnea(
            self,
            model: FakeReducedModel,
            data: object,
            q: list[float],
            qd: list[float],
            qdd: list[float],
        ) -> list[float]:
            assert q == [0.0, 1.0, 2.0, 3.0, 4.0, 5.0]
            assert qd == [0.0] * 6
            assert qdd == [0.0] * 6
            return [1, 2, 3, 4, 5, 6]

    class FakeNumpy:
        @staticmethod
        def asarray(values: list[float], dtype: type[float] = float) -> list[float]:
            return [dtype(value) for value in values]

    fake_pin = FakePin()
    monkeypatch.setattr(pinocchio_model_module, "_load_pinocchio", lambda: fake_pin)
    monkeypatch.setattr(pinocchio_model_module, "_load_numpy", lambda: FakeNumpy)

    model = pinocchio_model_module.PinocchioModel("play_e2.urdf")

    assert fake_pin.urdf_path == "play_e2.urdf"
    assert fake_pin.locked_joint_ids == [7, 8]
    assert model.inverse_dynamics([0, 1, 2, 3, 4, 5], [0] * 6, [0] * 6) == [
        1.0,
        2.0,
        3.0,
        4.0,
        5.0,
        6.0,
    ]


def test_load_gravity_model_points_to_pin_dependency(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    from rollio_airbot_play import pinocchio_model as pinocchio_model_module

    model_path = tmp_path / "play_e2.urdf"
    model_path.write_text("<robot name='play_e2'/>", encoding="utf-8")

    def missing_pinocchio() -> None:
        raise ImportError("pinocchio is missing")

    monkeypatch.setattr(pinocchio_model_module, "_load_pinocchio", missing_pinocchio)

    with pytest.raises(GravityModelUnavailableError, match="install the 'pin' Python package"):
        load_gravity_model(model_path)


def test_load_runtime_config_resolves_model_path_relative_to_config_file(tmp_path: Path) -> None:
    model_dir = tmp_path / "models"
    model_dir.mkdir()
    local_model_path = model_dir / "play_e2.urdf"
    local_model_path.write_text('<robot name="local-play-e2"/>', encoding="utf-8")

    config_path = tmp_path / "airbot.toml"
    config_path.write_text(
        """
name = "airbot"
type = "robot"
driver = "airbot-play"
id = "SN12345678"
dof = 6
mode = "free-drive"
interface = "can0"
product_variant = "play-e2"
model_path = "./models/play_e2.urdf"
""".strip(),
        encoding="utf-8",
    )

    config = load_runtime_config(config=config_path, config_inline=None)

    assert config.model_path == local_model_path


def test_parse_runtime_config_falls_back_to_packaged_play_e2_model(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    monkeypatch.chdir(tmp_path)

    config = parse_runtime_config(
        {
            "name": "airbot",
            "type": "robot",
            "driver": "airbot-play",
            "id": "SN12345678",
            "dof": 6,
            "mode": "free-drive",
            "interface": "can0",
            "product_variant": "play-e2",
            "model_path": "./models/play_e2.urdf",
        }
    )

    assert config.model_path is not None
    assert config.model_path.name == "play_e2.urdf"
    assert config.model_path.exists()
    assert (config.model_path.parent / "meshes" / "base_link.STL").exists()


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
    assert backend.sent_torques[0] == [0.5, 1.0, 1.5, 2.0, 2.5, 3.0]
    assert len(ipc.published) == 1


def test_vendor_backend_free_drive_uses_gravity_only_mit_and_caches_mode(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    fake_vendor = FakeVendorModule()
    monkeypatch.setattr(backend_module, "_load_vendor_module", lambda: fake_vendor)

    backend = backend_module.VendorAirbotBackend(make_config(model_path=tmp_path / "play.urdf"))

    assert fake_vendor.arm.set_param_calls == [
        ("arm.control_mode", fake_vendor.MotorControlMode.MIT)
    ]

    backend.send_gravity_compensation([1.0, 2.0, 3.0, 4.0, 5.0, 6.0])

    assert fake_vendor.arm.mit_calls == [
        (
            [0.0] * 6,
            [0.0] * 6,
            [1.0, 2.0, 3.0, 4.0, 5.0, 6.0],
            [0.0] * 6,
            [0.0] * 6,
        )
    ]
    assert fake_vendor.arm.set_param_calls == [
        ("arm.control_mode", fake_vendor.MotorControlMode.MIT)
    ]

    backend.send_gravity_compensation([0.5] * 6)
    assert len(fake_vendor.arm.set_param_calls) == 1

    backend.send_joint_targets([0.25] * 6)
    assert fake_vendor.arm.set_param_calls[-1] == (
        "arm.control_mode",
        fake_vendor.MotorControlMode.PVT,
    )
    mode_switch_count = len(fake_vendor.arm.set_param_calls)

    backend.send_joint_targets([0.1] * 6)
    assert len(fake_vendor.arm.set_param_calls) == mode_switch_count


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
                "id": "SN12345678",
                "dof": 6,
                "mode": "free-drive",
                "interface": "can0",
                "product_variant": "play-e2",
            }
        )


def test_invalid_probe_id_is_rejected() -> None:
    with pytest.raises(RuntimeError):
        parse_probe_id("airbot-play@can0")


def test_probe_devices_use_serial_numbers_as_ids(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setattr(backend_module, "is_python_can_available", lambda: True)
    monkeypatch.setattr(backend_module, "scan_can_interfaces", lambda: ["can0", "can1"])
    monkeypatch.setattr(
        backend_module,
        "query_airbot_serial",
        lambda interface, timeout=0.5: {"can0": "SN12345678", "can1": None}[interface],
    )

    assert probe_devices() == [
        ProbeDevice(
            device_id="SN12345678",
            interface="can0",
            product_variant="play-e2",
        )
    ]


def test_validate_and_capabilities_resolve_serial_number_ids(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setattr(
        backend_module,
        "probe_devices",
        lambda: [
            ProbeDevice(
                device_id="SN12345678",
                interface="can0",
                product_variant="play-e2",
            )
        ],
    )

    validate_probe_id("SN12345678")
    assert require_probe_device("SN12345678").interface == "can0"
    assert capabilities_for_probe_id("SN12345678") == {
        "id": "SN12345678",
        "driver": "airbot-play",
        "dof": 6,
        "supported_modes": ["free-drive", "command-following"],
        "transport": "can",
        "interface": "can0",
        "product_variant": "play-e2",
        "serial_number": "SN12345678",
    }


def test_probe_devices_is_empty_when_python_can_is_missing(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setattr(backend_module, "is_python_can_available", lambda: False)
    assert probe_devices() == []
