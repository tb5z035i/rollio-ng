from __future__ import annotations

from pathlib import Path
from typing import Protocol


class GravityModelUnavailableError(RuntimeError):
    """Raised when the gravity-comp backend cannot be loaded."""


class GravityModel(Protocol):
    def inverse_dynamics(
        self,
        q: list[float],
        qd: list[float],
        qdd: list[float],
    ) -> list[float]: ...


def load_gravity_model(model_path: Path) -> GravityModel:
    if not model_path.exists():
        raise FileNotFoundError(f"gravity model path does not exist: {model_path}")

    try:
        from airbot_ng.kdl.pinocchio import PinocchioModel
    except Exception as exc:  # pragma: no cover - depends on optional host install
        raise GravityModelUnavailableError(
            "Pinocchio gravity backend is unavailable; install airbot_ng.kdl.pinocchio support"
        ) from exc

    return PinocchioModel(str(model_path))


def compute_gravity_torques(
    model: GravityModel,
    positions: list[float],
    scales: list[float],
) -> list[float]:
    torques = list(
        model.inverse_dynamics(
            positions,
            [0.0] * len(positions),
            [0.0] * len(positions),
        )
    )
    return [float(torque) * float(scale) for torque, scale in zip(torques, scales, strict=False)]
