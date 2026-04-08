from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path
from typing import Any

try:
    import tomllib  # type: ignore[attr-defined]
except ModuleNotFoundError:  # pragma: no cover - Python 3.10 fallback
    import tomli as tomllib  # type: ignore[no-redef]


_PACKAGE_ROOT = Path(__file__).resolve().parent


class ConfigError(RuntimeError):
    """Raised when the AIRBOT driver configuration is invalid."""


@dataclass(slots=True)
class AirbotRuntimeConfig:
    name: str
    driver: str
    device_id: str
    dof: int
    mode: str
    control_frequency_hz: float
    interface: str
    product_variant: str
    end_effector: str | None
    model_path: Path | None
    gravity_comp_torque_scales: list[float]
    mit_kp: list[float]
    mit_kd: list[float]

    @property
    def control_period_s(self) -> float:
        return 1.0 / self.control_frequency_hz


def load_runtime_config(*, config: Path | None, config_inline: str | None) -> AirbotRuntimeConfig:
    if (config is None) == (config_inline is None):
        raise ConfigError("run requires exactly one of --config or --config-inline")

    config_base_dir: Path | None = None
    if config is not None:
        config_path = config.resolve()
        data = tomllib.loads(config_path.read_text(encoding="utf-8"))
        config_base_dir = config_path.parent
    else:
        data = tomllib.loads(config_inline or "")

    return parse_runtime_config(data, config_base_dir=config_base_dir)


def parse_runtime_config(
    data: dict[str, Any], *, config_base_dir: Path | None = None
) -> AirbotRuntimeConfig:
    name = _required_string(data, "name")
    driver = _required_string(data, "driver")
    device_type = _required_string(data, "type")
    device_id = _required_string(data, "id")
    interface = _required_string(data, "interface")
    product_variant = _required_string(data, "product_variant")
    dof = _required_int(data, "dof")
    mode = _required_string(data, "mode")

    if device_type != "robot":
        raise ConfigError(f'expected type = "robot", got "{device_type}"')
    if driver != "airbot-play":
        raise ConfigError(f'expected driver = "airbot-play", got "{driver}"')
    if dof <= 0 or dof > 16:
        raise ConfigError(f"dof must be between 1 and 16, got {dof}")
    if mode not in {"free-drive", "command-following"}:
        raise ConfigError(f"unsupported mode: {mode}")

    model_path_raw = data.get("model_path")
    model_path = (
        _resolve_model_path(Path(model_path_raw), config_base_dir=config_base_dir)
        if isinstance(model_path_raw, str) and model_path_raw
        else None
    )
    if mode == "free-drive" and model_path is None:
        raise ConfigError("free-drive mode requires model_path for gravity compensation")

    gravity_comp_torque_scales = _joint_array(
        data.get("gravity_comp_torque_scales"),
        dof,
        default=1.0,
        field_name="gravity_comp_torque_scales",
    )
    mit_kp = _joint_array(data.get("mit_kp"), dof, default=0.0, field_name="mit_kp")
    mit_kd = _joint_array(data.get("mit_kd"), dof, default=0.0, field_name="mit_kd")

    return AirbotRuntimeConfig(
        name=name,
        driver=driver,
        device_id=device_id,
        dof=dof,
        mode=mode,
        control_frequency_hz=float(data.get("control_frequency_hz", 250.0)),
        interface=interface,
        product_variant=product_variant,
        end_effector=_optional_string(data, "end_effector"),
        model_path=model_path,
        gravity_comp_torque_scales=gravity_comp_torque_scales,
        mit_kp=mit_kp,
        mit_kd=mit_kd,
    )


def _resolve_model_path(model_path: Path, *, config_base_dir: Path | None) -> Path:
    expanded_path = model_path.expanduser()
    if expanded_path.is_absolute():
        return expanded_path

    candidates: list[Path] = []
    if config_base_dir is not None:
        candidates.append(config_base_dir / expanded_path)
    candidates.append(Path.cwd() / expanded_path)

    packaged_model_path = _resolve_packaged_model_path(expanded_path)
    if packaged_model_path is not None:
        candidates.append(packaged_model_path)

    for candidate in candidates:
        if candidate.exists():
            return candidate

    return candidates[0]


def _resolve_packaged_model_path(model_path: Path) -> Path | None:
    parts = [part for part in model_path.parts if part not in ("", ".")]
    if not parts or parts[0] != "models" or any(part == ".." for part in parts):
        return None
    return _PACKAGE_ROOT.joinpath(*parts)


def _required_string(data: dict[str, Any], key: str) -> str:
    value = data.get(key)
    if not isinstance(value, str) or not value.strip():
        raise ConfigError(f"{key} must be a non-empty string")
    return value


def _optional_string(data: dict[str, Any], key: str) -> str | None:
    value = data.get(key)
    if value is None:
        return None
    if not isinstance(value, str) or not value.strip():
        raise ConfigError(f"{key} must be a non-empty string when provided")
    return value


def _required_int(data: dict[str, Any], key: str) -> int:
    value = data.get(key)
    if not isinstance(value, int):
        raise ConfigError(f"{key} must be an integer")
    return value


def _joint_array(
    raw_value: Any,
    dof: int,
    *,
    default: float,
    field_name: str,
) -> list[float]:
    if raw_value is None:
        return [default] * dof
    if not isinstance(raw_value, list):
        raise ConfigError(f"{field_name} must be an array")
    if len(raw_value) != dof:
        raise ConfigError(f"{field_name} must contain exactly {dof} values")

    values: list[float] = []
    for value in raw_value:
        if not isinstance(value, int | float):
            raise ConfigError(f"{field_name} must contain only numbers")
        values.append(float(value))
    return values
