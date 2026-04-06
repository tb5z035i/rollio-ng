from __future__ import annotations

import argparse
import json
import sys
from collections.abc import Sequence
from pathlib import Path

from .backend import (
    ProbeDevice,
    VendorAirbotBackend,
    capabilities_for_probe_id,
    probe_devices,
    validate_probe_id,
)
from .config import ConfigError, load_runtime_config
from .ipc import Iceoryx2IpcAdapter
from .runtime import AirbotRuntime


def main(argv: Sequence[str] | None = None) -> int:
    parser = _build_parser()
    args = parser.parse_args(argv)

    try:
        if args.command == "probe":
            print(json.dumps([_serialize_probe_device(device) for device in probe_devices()]))
            return 0
        if args.command == "validate":
            validate_probe_id(args.id)
            print(json.dumps({"valid": True, "id": args.id}))
            return 0
        if args.command == "capabilities":
            print(json.dumps(capabilities_for_probe_id(args.id)))
            return 0
        if args.command == "run":
            config = load_runtime_config(
                config=Path(args.config) if args.config else None,
                config_inline=args.config_inline,
            )
            backend = VendorAirbotBackend(config)
            ipc = Iceoryx2IpcAdapter(config.name)
            runtime = AirbotRuntime(config=config, backend=backend, ipc=ipc)
            try:
                runtime.run()
            finally:
                runtime.close()
            return 0
    except (ConfigError, RuntimeError, FileNotFoundError) as exc:
        print(f"rollio-robot-airbot-play: {exc}", file=sys.stderr)
        return 1

    parser.error(f"unknown command: {args.command}")
    return 1


def _build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="rollio-robot-airbot-play")
    subparsers = parser.add_subparsers(dest="command", required=True)

    subparsers.add_parser("probe")

    validate_parser = subparsers.add_parser("validate")
    validate_parser.add_argument("id")

    capabilities_parser = subparsers.add_parser("capabilities")
    capabilities_parser.add_argument("id")

    run_parser = subparsers.add_parser("run")
    run_group = run_parser.add_mutually_exclusive_group(required=True)
    run_group.add_argument("--config")
    run_group.add_argument("--config-inline")

    return parser


def _serialize_probe_device(device: ProbeDevice) -> dict[str, object]:
    return {
        "id": device.device_id,
        "driver": device.driver,
        "interface": device.interface,
        "product_variant": device.product_variant,
    }
