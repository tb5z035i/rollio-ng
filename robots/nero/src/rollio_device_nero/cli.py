"""argparse entry point for `rollio-device-nero`.

Mirrors the `Cli` enum in `rollio-device-airbot-play`
([robots/airbot_play_rust/src/bin/device.rs](../../airbot_play_rust/src/bin/device.rs))
so that `rollio setup` / `rollio collect` discover the AGX Nero with no
controller-side adapter.

Subcommands:

  probe                  list candidate Nero arms (CAN interfaces)
  validate <id>          confirm <id> is a Nero by running connect+enable
  query    <id>          emit a `DeviceQueryResponse` for setup
  run      --config      enter the per-channel control loop driven by IPC
"""

from __future__ import annotations

import argparse
import json
import sys
from collections.abc import Sequence
from pathlib import Path

from . import DEVICE_LABEL, DRIVER_NAME
from .config import ConfigError, load_runtime_config
from .probe import (
    DEFAULT_PROBE_TIMEOUT_MS,
    probe_devices,
    validate_device,
)
from .query import build_device_query_response


def main(argv: Sequence[str] | None = None) -> int:
    parser = _build_parser()
    args = parser.parse_args(argv)

    try:
        if args.command == "probe":
            return _run_probe(args)
        if args.command == "validate":
            return _run_validate(args)
        if args.command == "query":
            return _run_query(args)
        if args.command == "run":
            return _run_device(args)
    except (ConfigError, RuntimeError, FileNotFoundError) as exc:
        print(f"rollio-device-agx-nero: {exc}", file=sys.stderr)
        return 1

    parser.error(f"unknown command: {args.command}")
    return 1


def _build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="rollio-device-agx-nero",
        description=f"{DEVICE_LABEL} device driver on the Rollio Sprint Extra A contract",
    )
    subparsers = parser.add_subparsers(dest="command", required=True)

    probe = subparsers.add_parser("probe", help="list candidate Nero arms")
    probe.add_argument("--timeout-ms", type=int, default=DEFAULT_PROBE_TIMEOUT_MS)
    probe.add_argument("--json", action="store_true", help="emit JSON instead of text")

    validate = subparsers.add_parser(
        "validate", help="confirm <id> is a Nero by running connect+enable"
    )
    validate.add_argument("id", help="device id (CAN interface name, e.g. can0)")
    validate.add_argument(
        "--channel-type",
        dest="channel_types",
        action="append",
        default=[],
        help="optional channel filter (arm or gripper); may be repeated",
    )
    validate.add_argument("--timeout-ms", type=int, default=DEFAULT_PROBE_TIMEOUT_MS)
    validate.add_argument("--json", action="store_true")

    query = subparsers.add_parser("query", help="emit a DeviceQueryResponse for <id>")
    query.add_argument("id")
    query.add_argument("--timeout-ms", type=int, default=DEFAULT_PROBE_TIMEOUT_MS)
    query.add_argument("--json", action="store_true")

    run = subparsers.add_parser("run", help="enter the per-channel control loop")
    group = run.add_mutually_exclusive_group(required=True)
    group.add_argument("--config", type=Path, help="path to a TOML config file")
    group.add_argument("--config-inline", help="inline TOML config string")
    run.add_argument(
        "--dry-run",
        action="store_true",
        help="parse the config and exit (no hardware access)",
    )

    return parser


# ---------------------------------------------------------------------------
# Subcommand implementations
# ---------------------------------------------------------------------------


def _run_probe(args: argparse.Namespace) -> int:
    devices = probe_devices(timeout_ms=args.timeout_ms)
    if args.json:
        ids = [d.device_id for d in devices if d.feedback_observed]
        print(json.dumps(ids))
    else:
        if not devices:
            print("no AGX Nero candidates detected (no CAN interfaces)")
            return 0
        for device in devices:
            tag = "ok" if device.feedback_observed else "no feedback"
            print(f"{DEVICE_LABEL} ({device.device_id}) [{tag}]")
    return 0


def _run_validate(args: argparse.Namespace) -> int:
    valid_channels = _accept_channel_types(args.channel_types)
    valid = valid_channels and validate_device(args.id, timeout_ms=args.timeout_ms)
    if args.json:
        print(
            json.dumps(
                {
                    "valid": bool(valid),
                    "id": args.id,
                    "channel_types": list(args.channel_types),
                }
            )
        )
    elif valid:
        print(f"{args.id} is valid")
    else:
        print(f"{args.id} is invalid")
    return 0 if valid else 1


def _run_query(args: argparse.Namespace) -> int:
    # The user spec ties device-id to a CAN interface, but the controller
    # may invoke `query` *before* the iface is up (e.g. setup wizard rendering
    # value limits for visualizer). We therefore do NOT touch the hardware
    # here; we just emit the static capability sheet keyed on `id`.
    response = build_device_query_response(args.id)
    if args.json:
        print(json.dumps(response))
    else:
        print(f"{response['driver']}")
        for device in response["devices"]:
            print(f"  {device['device_label']} ({device['id']})")
            for channel in device["channels"]:
                print(f"    - {channel['channel_type']} [robot]")
    return 0


def _run_device(args: argparse.Namespace) -> int:
    config = load_runtime_config(config=args.config, config_inline=args.config_inline)
    if args.dry_run:
        # Useful for CI smoke tests: parse the config but skip CAN/IPC.
        return 0
    # Lazy import: pyAgxArm / pinocchio / iceoryx2 are only required for `run`.
    from .runtime.device import run_device

    return run_device(config)


def _accept_channel_types(channel_types: list[str]) -> bool:
    if not channel_types:
        return True
    accepted = {"arm", "gripper"}
    return all(value in accepted for value in channel_types)


__all__ = ["main"]
