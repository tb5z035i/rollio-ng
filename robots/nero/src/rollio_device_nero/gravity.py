"""Pinocchio gravity-compensation model for the AGX Nero arm.

Provides a single `NeroModel` wrapper that:

  * Loads the bundled `nero_description.urdf` from
    `importlib.resources.files("rollio_device_nero.assets")`. No absolute
    paths are required and Pinocchio happily ignores the URDF's
    `package://...` mesh tags because we never call `buildGeometryModel`.
  * Optionally appends the four-body AGX gripper sub-tree as added body
    inertia on `joint7`, mirroring `attach_gripper_payload` from
    `external/reference/nero-demo/gravity_compensation.py`. With both
    finger joints locked at q = 0 (closed gripper) the lumped COM shifts
    by < 1 mm at full stroke, so this fixed-pose lumping is a very good
    approximation for the gravity term.
  * Exposes `gravity_torques(q)` (RNEA with v=0, a=0), `forward_kinematics`
    (link7 SE3) and `frame_jacobian` (link7 spatial Jacobian in the local
    frame) helpers used by the runtime + IK modules.
"""

from __future__ import annotations

from importlib import resources
from pathlib import Path
from typing import Any

import numpy as np

from .config import TAU_MAX

# The URDF gives `joint7` (the last revolute joint) a frame whose origin
# sits ON its rotation axis. That means rotating only joint7 leaves the
# translation of `data.oMi[joint7]` unchanged in base coordinates -- a
# poor "end-effector pose" because dragging the wrist by hand would not
# be visible in the published Pose7. We therefore add an OPERATIONAL
# pinocchio frame at the chosen tool tip; the +0.032 m component along
# joint7's local x is perpendicular to the rotation axis, so the tip's
# translation now changes as joint7 rotates.
_TIP_JOINT_NAME: str = "joint7"
_TIP_FRAME_NAME: str = "agx_nero_tool_tip"

# Tool flange placement on joint7. Source: `M_l7_flange` in
# `external/reference/nero-demo/gravity_compensation.py`, originally from
# `nero_with_gripper_flange_description.xacro`.
TOOL_FLANGE_TRANSLATION: tuple[float, float, float] = (0.032, 0.0, -0.0235)
TOOL_FLANGE_RPY: tuple[float, float, float] = (-1.5708, 0.0, -1.5708)

# AGX gripper assembly length, measured by hand from the gripper-flange
# face (where the gripper bolts onto the arm) to the fingertip plane.
# The gripper's local z-axis points outward from the arm, so the TCP
# is `M_l7_flange * SE3(xyz=(0, 0, GRIPPER_TCP_DEPTH_M))`. This is
# treated as a fixed constant -- it does NOT track the live gripper
# opening width; the parallel-gripper midpoint stays on the centerline
# regardless of how open the jaws are.
GRIPPER_TCP_DEPTH_M: float = 0.1413

_NERO_URDF_RESOURCE: str = "nero_description.urdf"

_TAU_MAX_NP: np.ndarray = np.asarray(TAU_MAX, dtype=float)


def _load_pinocchio() -> Any:
    try:
        import pinocchio
    except Exception as exc:  # pragma: no cover - depends on host install
        raise RuntimeError(
            "pinocchio is required for AGX Nero gravity compensation; install "
            "it with `pip install pin` or via uv sync inside robots/nero."
        ) from exc
    return pinocchio


def _packaged_urdf_path() -> Path:
    """Resolve the bundled URDF as a real filesystem path.

    `pin.buildModelFromUrdf` takes a `str` path; with editable installs the
    asset already lives on disk, but for installed wheels we rely on
    `as_file` to materialise the resource into a temporary path. Both
    behaviours are handled by the resources API.
    """
    asset = resources.files("rollio_device_nero.assets").joinpath(_NERO_URDF_RESOURCE)
    if isinstance(asset, Path):
        return asset
    # `MultiplexedPath` / `Traversable` from a zip wheel. `as_file()` would
    # require a context manager, but our consumers want a long-lived path,
    # so use `str(asset)` to trigger the underlying resource extraction.
    return Path(str(asset))


# ---------------------------------------------------------------------------
# AGX gripper inertia (lifted from gravity_compensation.py)
# ---------------------------------------------------------------------------


def _se3_from_urdf(pin: Any, xyz: tuple[float, ...], rpy: tuple[float, ...]) -> Any:
    """Build a pinocchio SE3 from URDF-style (xyz, rpy) origin tags."""
    R = pin.rpy.rpyToMatrix(*rpy)
    return pin.SE3(R, np.asarray(xyz, dtype=float))


def _gripper_sub_bodies(pin: Any) -> list[tuple[str, float, np.ndarray, np.ndarray, Any]]:
    """Return [(name, mass, com_in_body, I_at_com_in_body, body_in_link7), ...]."""
    M_l7_flange = _se3_from_urdf(pin, (0.032, 0.0, -0.0235), (-1.5708, 0.0, -1.5708))
    M_flange_base = _se3_from_urdf(pin, (0.0, 0.0, 0.0055), (0.0, 0.0, 0.0))
    M_base_l1 = _se3_from_urdf(pin, (0.0, 0.0, 0.1358), (1.5707963, 0.0, 3.1415926))
    M_base_l2 = _se3_from_urdf(pin, (0.0, 0.0, 0.1358), (1.5707963, 0.0, 0.0))

    M_l7_base = M_l7_flange * M_flange_base
    M_l7_l1 = M_l7_base * M_base_l1
    M_l7_l2 = M_l7_base * M_base_l2

    return [
        (
            "gripper_flange",
            0.04771096,
            np.array([0.0, 0.0, -0.00032]),
            np.array(
                [
                    [2.697e-05, 0.0, 0.0],
                    [0.0, 4.311e-05, 0.0],
                    [0.0, 0.0, 2.479e-05],
                ]
            ),
            M_l7_flange,
        ),
        (
            "gripper_base",
            0.45,
            np.array([-0.000183807162235591, 8.05033155577911e-05, 0.0321436689908876]),
            np.array(
                [
                    [0.00092934, 0.00000034, -0.00000738],
                    [0.00000034, 0.00071447, 0.00000005],
                    [-0.00000738, 0.00000005, 0.00039442],
                ]
            ),
            M_l7_base,
        ),
        (
            "gripper_link1",
            0.025,
            np.array([0.00065123185041968, -0.0491929869131989, 0.00972258769184025]),
            np.array(
                [
                    [0.00007371, -0.00000113, 0.00000021],
                    [-0.00000113, 0.00000781, -0.00001372],
                    [0.00000021, -0.00001372, 0.0000747],
                ]
            ),
            M_l7_l1,
        ),
        (
            "gripper_link2",
            0.025,
            np.array([0.00065123185041968, -0.0491929869131989, 0.00972258769184025]),
            np.array(
                [
                    [0.00007371, -0.00000113, 0.00000021],
                    [-0.00000113, 0.00000781, -0.00001372],
                    [0.00000021, -0.00001372, 0.0000747],
                ]
            ),
            M_l7_l2,
        ),
    ]


def _attach_gripper_payload(pin: Any, model: Any) -> dict[str, Any]:
    """Append the AGX gripper sub-tree as added body inertia on link7."""
    link7_id = model.getJointId("joint7")
    total_mass = 0.0
    weighted_com = np.zeros(3)
    for _name, mass, com_local, inertia_local, placement in _gripper_sub_bodies(pin):
        body_inertia = pin.Inertia(mass, com_local, inertia_local)
        model.appendBodyToJoint(link7_id, body_inertia, placement)
        com_in_link7 = placement.translation + placement.rotation @ com_local
        total_mass += mass
        weighted_com += mass * com_in_link7
    return {
        "total_mass": float(total_mass),
        "com_in_link7": (weighted_com / total_mass if total_mass > 0 else weighted_com),
    }


# ---------------------------------------------------------------------------
# Model wrapper
# ---------------------------------------------------------------------------


class NeroModel:
    """Pinocchio Model + Data + tool-flange operational frame for the AGX Nero arm."""

    def __init__(
        self,
        *,
        with_gripper: bool = True,
        urdf_path: str | Path | None = None,
        tip_offset: Any | None = None,
    ) -> None:
        """Build the Pinocchio Model + Data and register the tool-tip frame.

        Tip-frame defaults:
          * `with_gripper=True`  -> AGX gripper TCP (flange + GRIPPER_TCP_DEPTH_M
            along the gripper's outward z-axis). This is the natural endpoint
            for teleop -- the midpoint between the jaws at the fingertip plane.
          * `with_gripper=False` -> bare tool flange. Useful when the arm is
            running standalone without the gripper attached.

        `tip_offset` (a `pin.SE3`) overrides the default placement entirely.
        """
        self._pin = _load_pinocchio()
        path = Path(urdf_path) if urdf_path is not None else _packaged_urdf_path()
        if not path.is_file():
            raise FileNotFoundError(f"URDF not found: {path}")

        model = self._pin.buildModelFromUrdf(str(path))
        if model.nq != 7 or model.nv != 7:
            raise RuntimeError(
                f"Expected a 7-DOF Nero model, got nq={model.nq}, nv={model.nv}. "
                "Make sure the bundled `nero_description.urdf` is the fixed-base description."
            )

        self.gripper_summary: dict[str, Any] | None = None
        if with_gripper:
            self.gripper_summary = _attach_gripper_payload(self._pin, model)

        self._tip_joint_id: int = int(model.getJointId(_TIP_JOINT_NAME))
        if tip_offset is None:
            self._tip_offset = (
                _default_gripper_tcp_se3(self._pin)
                if with_gripper
                else _default_tool_flange_se3(self._pin)
            )
        else:
            self._tip_offset = tip_offset
        self._tip_frame_id: int = int(
            model.addFrame(
                self._pin.Frame(
                    _TIP_FRAME_NAME,
                    self._tip_joint_id,
                    self._tip_offset,
                    self._pin.FrameType.OP_FRAME,
                ),
                False,  # don't fold tip's (zero) inertia into joint7
            )
        )

        self._model = model
        self._data = model.createData()

    # ----- accessors -----

    @property
    def model(self) -> Any:
        return self._model

    @property
    def data(self) -> Any:
        return self._data

    @property
    def nq(self) -> int:
        return int(self._model.nq)

    @property
    def nv(self) -> int:
        return int(self._model.nv)

    @property
    def tip_joint_id(self) -> int:
        return self._tip_joint_id

    @property
    def tip_frame_id(self) -> int:
        return self._tip_frame_id

    @property
    def tip_offset(self) -> Any:
        return self._tip_offset

    # ----- physics helpers -----

    def gravity_torques(self, q: np.ndarray) -> np.ndarray:
        """Compute g(q) (length 7, N*m) using RNEA with v=0, a=0."""
        zeros = np.zeros(self.nv)
        q_arr = np.ascontiguousarray(q, dtype=float)[: self.nq]
        return np.asarray(self._pin.rnea(self._model, self._data, q_arr, zeros, zeros))

    def gravity_torques_clipped(self, q: np.ndarray) -> np.ndarray:
        """Same as `gravity_torques` but clipped to per-joint TAU_MAX."""
        return np.clip(self.gravity_torques(q), -_TAU_MAX_NP, _TAU_MAX_NP)

    def forward_kinematics(self, q: np.ndarray) -> Any:
        """Run pinocchio FK and return the SE3 placement of the tool flange.

        Uses the operational frame registered at construction time; this
        is offset from joint7's rotation axis so dragging joint7 changes
        the translation of the returned SE3 (unlike `data.oMi[joint7]`).
        """
        q_arr = np.ascontiguousarray(q, dtype=float)[: self.nq]
        self._pin.framesForwardKinematics(self._model, self._data, q_arr)
        return self._data.oMf[self._tip_frame_id]

    def frame_jacobian(self, q: np.ndarray) -> np.ndarray:
        """Body Jacobian of the tool-flange frame in flange-LOCAL coords (6 x nv).

        Pairs naturally with `pin.log6(oMf.actInv(target))` for IK error
        computation; both error and Jacobian columns share the same frame.
        """
        q_arr = np.ascontiguousarray(q, dtype=float)[: self.nq]
        return np.asarray(
            self._pin.computeFrameJacobian(
                self._model,
                self._data,
                q_arr,
                self._tip_frame_id,
                self._pin.ReferenceFrame.LOCAL,
            )
        )

    def end_effector_pose7(self, q: np.ndarray) -> list[float]:
        """Return [x, y, z, qx, qy, qz, qw] (matches `Pose7` wire format)."""
        oMf = self.forward_kinematics(q)
        translation = np.asarray(oMf.translation, dtype=float).reshape(3)
        # pinocchio's Quaternion uses xyzw external order.
        quat = self._pin.Quaternion(oMf.rotation)
        return [
            float(translation[0]),
            float(translation[1]),
            float(translation[2]),
            float(quat.x),
            float(quat.y),
            float(quat.z),
            float(quat.w),
        ]


def _default_tool_flange_se3(pin: Any) -> Any:
    """Bare tool flange (no gripper). Matches the gripper xacro placement."""
    return _se3_from_urdf(pin, TOOL_FLANGE_TRANSLATION, TOOL_FLANGE_RPY)


def _default_gripper_tcp_se3(pin: Any) -> Any:
    """AGX gripper TCP -- flange + `GRIPPER_TCP_DEPTH_M` along gripper z.

    The TCP frame inherits the flange frame's orientation so the +z axis
    points outward from the arm (toward the workspace) -- consistent with
    the convention IK targets typically use for grasp poses.
    """
    flange = _default_tool_flange_se3(pin)
    tcp_in_flange = _se3_from_urdf(pin, (0.0, 0.0, GRIPPER_TCP_DEPTH_M), (0.0, 0.0, 0.0))
    return flange * tcp_in_flange


__all__ = [
    "GRIPPER_TCP_DEPTH_M",
    "TOOL_FLANGE_RPY",
    "TOOL_FLANGE_TRANSLATION",
    "NeroModel",
]
