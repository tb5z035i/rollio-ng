from __future__ import annotations

from collections.abc import Sequence
from importlib import import_module
from pathlib import Path

_ARM_JOINT_NAMES = ("joint1", "joint2", "joint3", "joint4", "joint5", "joint6")


class PinocchioModel:
    def __init__(
        self,
        urdf_path: str | Path,
        *,
        arm_joint_names: Sequence[str] = _ARM_JOINT_NAMES,
    ) -> None:
        pin = _load_pinocchio()
        full_model = pin.buildModelFromUrdf(str(urdf_path))

        locked_joint_ids = [
            joint_id
            for joint_id in range(1, full_model.njoints)
            if full_model.names[joint_id] not in arm_joint_names
        ]

        if locked_joint_ids:
            self._model = pin.buildReducedModel(
                full_model, locked_joint_ids, pin.neutral(full_model)
            )
        else:
            self._model = full_model

        self._pin = pin
        self._data = self._model.createData()
        self.nq = int(self._model.nq)

    def inverse_dynamics(
        self,
        q: list[float],
        qd: list[float],
        qdd: list[float],
    ) -> list[float]:
        np = _load_numpy()
        torques = self._pin.rnea(
            self._model,
            self._data,
            np.asarray(q[: self.nq], dtype=float),
            np.asarray(qd[: self.nq], dtype=float),
            np.asarray(qdd[: self.nq], dtype=float),
        )
        return [float(value) for value in torques]


def _load_pinocchio():
    try:
        return import_module("pinocchio")
    except Exception as exc:  # pragma: no cover - depends on optional host install
        raise ImportError("Pinocchio is not installed. Install it with: pip install pin") from exc


def _load_numpy():
    try:
        return import_module("numpy")
    except Exception as exc:  # pragma: no cover - depends on optional host install
        raise ImportError("NumPy is required for the Pinocchio gravity backend") from exc
